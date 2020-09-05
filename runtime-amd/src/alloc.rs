
use super::*;

use alloc_wg::alloc::*;

pub struct LapAlloc {
  pub(crate) id: AcceleratorId,
  pub(crate) device_agent: Agent,
  pub(crate) pool: MemoryPoolAlloc,
  /// Granting devices access is a really expensive syscall (more than a mmap), so this
  /// is here to avoid that where possible.
  /// None by default to avoid an allocation if eg a LapVec is constructed but
  /// never pushed to.
  /// XXX Fix alloc-wg (again)
  pub(crate) accessible: UnsafeCell<Option<Arc<SmallVec<[Agent; 32]>>>>,
}
impl LapAlloc {
  pub fn alloc_pool(&self) -> &MemoryPoolAlloc {
    &self.pool
  }
  pub fn pool(&self) -> &MemoryPool {
    &self.pool
  }

  pub fn is_accessible_to(&self, dev: &HsaAmdGpuAccel) -> bool {
    self.accessible()
      .iter()
      .flat_map(|i| i.iter() )
      .any(|a| a == dev.agent() )
  }

  /// `pool_ptr` *must* be an allocation of `self`.
  pub unsafe fn add_access(&mut self, dev: &HsaAmdGpuAccel,
                           pool_ptr: Option<MemoryPoolPtr<[u8]>>)
    -> Result<(), HsaError>
  {
    add_access(self, dev, pool_ptr)
  }

  pub fn accessible(&self) -> Option<&[Agent]> {
    unsafe {
      (&*self.accessible.get())
        .as_ref()
        .map(|agents| &agents[..] )
    }
  }
  fn accessible_mut(&self) -> &mut Arc<SmallVec<[Agent; 32]>> {
    unsafe {
      (&mut *self.accessible.get())
        .as_mut()
        .unwrap()
    }
  }

  unsafe fn update_access_grants(&mut self, ptr: NonNull<u8>,
                                 layout: Layout) -> Result<(), HsaError> {
    if let Some(accessible) = self.accessible() {
      // Handle cases where `LapAllocAccess::add_access` is called before allocation.
      // We don't need to check accessibility: it was already checked in
      // LapAllocAccess::add_access.
      let pool_ptr = MemoryPoolPtr::from_ptr(self.pool().clone(), ptr);
      let r = pool_ptr.grant_agents_access(&accessible);
      // if grant access fails, we need to dealloc before returning
      if let Err(err) = r {
        self.dealloc(ptr, layout);
        return Err(err);
      }
    }

    Ok(())
  }
  unsafe fn expect_update_access_grants(&mut self, ptr: NonNull<u8>, layout: Layout) {
    if let Err(err) = self.update_access_grants(ptr, layout) {
      // XXX an error here doesn't result in any sort of allocation rollback;
      // the pool will have already de-allocated the original allocation, so
      // returning an error here could result in the use of freed memory!
      panic!("failed to grant agents access to new grown allocation: {:?}", err);
    }
  }
}
impl From<Arc<HsaAmdGpuAccel>> for LapAlloc {
  fn from(accel: Arc<HsaAmdGpuAccel>) -> Self {
    accel.fine_lap_node_alloc(0)
  }
}
impl Clone for LapAlloc {
  fn clone(&self) -> Self {
    let accessible = unsafe {
      (&*self.accessible.get()).clone()
    };
    LapAlloc {
      id: self.id,
      device_agent: self.device_agent.clone(),
      pool: self.pool.clone(),
      accessible: UnsafeCell::new(accessible),
    }
  }
}
impl fmt::Debug for LapAlloc {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.debug_struct("LapAlloc")
      .field("id", &self.id)
      .field("device_agent", &self.device_agent)
      .field("pool", &self.pool)
      .field("accessible", &self.accessible())
      .finish()
  }
}
/// Mutable borrow rules prevent concurrent mutable borrows of the accessible array.
unsafe impl Send for LapAlloc { }
unsafe impl Sync for LapAlloc { }

unsafe impl AllocRef for LapAlloc {
  fn alloc(&mut self, layout: Layout, init: AllocInit)
    -> Result<MemoryBlock, AllocErr>
  {
    let out = self.pool.alloc(layout, init)?;
    let bytes = layout.size();
    if bytes == 0 {
      return Ok(out);
    }

    unsafe {
      self.update_access_grants(out.ptr, layout)
        .ok().ok_or(AllocErr)?;
    }

    Ok(out)
  }
  unsafe fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
    self.pool.dealloc(ptr, layout)
  }

  unsafe fn grow(&mut self, ptr: NonNull<u8>,
                 layout: Layout, new_size: usize,
                 placement: ReallocPlacement,
                 init: AllocInit)
    -> Result<MemoryBlock, AllocErr>
  {
    let new_ptr = self.pool.grow(ptr, layout, new_size,
                                 placement, init)?;
    if new_ptr.ptr == ptr {
      // We grew in place; no need to grant access
      return Ok(new_ptr);
    }

    self.expect_update_access_grants(new_ptr.ptr, layout);

    Ok(new_ptr)
  }

  #[inline(always)]
  unsafe fn shrink(&mut self, ptr: NonNull<u8>,
                   layout: Layout,
                   new_size: usize,
                   placement: ReallocPlacement)
    -> Result<MemoryBlock, AllocErr>
  {
    // We don't need to update the access list here
    let new = self.pool.shrink(ptr, layout, new_size,
                               placement)?;
    if new.ptr == ptr {
      // in place; no need to grant access
      return Ok(new);
    }

    if new_size != 0 {
      self.expect_update_access_grants(new.ptr, layout);
    }

    Ok(new)
  }
}

/// `build_alloc` *must* be unique
#[inline(always)]
fn add_access(build_alloc: &LapAlloc,
              device: &HsaAmdGpuAccel,
              pool_ptr: Option<MemoryPoolPtr<[u8]>>)
  -> Result<(), HsaError>
{
  let already_accessible = build_alloc
    .accessible()
    .iter()
    .flat_map(|i| i.iter() )
    .any(|a| a == device.agent() );
  if already_accessible {
    // nothing to do.
    return Ok(());
  }

  // add this device to the accessible array: We need to do this now for later
  // allocations (in the case of a LapVec).
  if build_alloc.accessible().is_none() {
    // ensure our owner runtime/device is in the list:
    let mut list = SmallVec::new();
    list.push(build_alloc.device_agent.clone());
    unsafe {
      *build_alloc.accessible.get() = Some(Arc::new(list));
    }
  }
  let accessible = Arc::make_mut(build_alloc.accessible_mut());
  if device.agent() != &build_alloc.device_agent {
    accessible.push(device.agent().clone());
  }

  let pool = &build_alloc.pool;
  let aa = match pool.agent_access(device.agent()) {
    Ok(v) => v,
    Err(err) => {
      accessible.pop();
      return Err(err);
    },
  };
  if aa.never_allowed() {
    error!("pool {:?} not sharable with this device ({:?})",
           pool, device.id());
    return Err(HsaError::General);
  } else if aa.default_disallowed() {
    if let Some(pool_ptr) = pool_ptr {
      match pool_ptr.grant_agents_access(&accessible) {
        Ok(()) => { },
        Err(err) => {
          accessible.pop();
          return Err(err);
        },
      }
    }
  } else if aa.default_allowed() {
    // nothing to do
  } else {
    unreachable!("{:?}", aa);
  }

  Ok(())
}

/// No device removal.
pub trait LapAllocAccess: BoxPoolPtr {
  fn is_accessible_to(&self, dev: &HsaAmdGpuAccel) -> bool;

  /// TODO: it would be nice to allow granting new devices access (which is expensive)
  /// concurrently. This is safe because only removal is concurrent-unsafe.
  fn add_access(&mut self, device: &HsaAmdGpuAccel) -> Result<(), HsaError>;
}

/// A Vec-esk type allocated from a CPU visible memory pool.
/// Locality is defined as memory accessible to processor which
/// is running the code.
///
/// This type is safe to dereference.
///
/// Lap: LocallyAccessiblePool
pub type LapVec<T> = alloc_wg::vec::Vec<T, LapAlloc>;
/// A Box-esk type allocated from a CPU visible memory pool.
/// Locality is defined as memory accessible to processor which
/// is running the code.
///
/// This type is safe to dereference.
///
/// Lap: LocallyAccessiblePool
pub type LapBox<T> = alloc_wg::boxed::Box<T, LapAlloc>;

impl<T> LapAllocAccess for LapVec<T> {
  fn is_accessible_to(&self, dev: &HsaAmdGpuAccel) -> bool {
    if std::mem::size_of::<T>() == 0 {
      // These are never actually allocated, so we can just say they're
      // always accessible.
      return true;
    }

    self.alloc_ref()
      .accessible()
      .iter()
      .flat_map(|i| i.iter() )
      .any(|a| a == dev.agent() )
  }

  fn add_access(&mut self, device: &HsaAmdGpuAccel) -> Result<(), HsaError> {
    if std::mem::size_of::<T>() == 0 {
      // These are never actually allocated, so we can just say they're
      // always accessible.
      return Ok(());
    }

    let ptr = unsafe { self.pool_ptr() };
    add_access(self.alloc_ref(), device, ptr)
  }
}
impl<T> LapAllocAccess for LapBox<T>
  where T: ?Sized,
{
  fn is_accessible_to(&self, dev: &HsaAmdGpuAccel) -> bool {
    if std::mem::size_of_val(&**self) == 0 {
      // These are never actually allocated, so we can just say they're
      // always accessible: no delayed allocations for boxes.
      return true;
    }

    self.alloc_ref()
      .accessible()
      .iter()
      .flat_map(|i| i.iter() )
      .any(|a| a == dev.agent() )
  }

  fn add_access(&mut self, device: &HsaAmdGpuAccel) -> Result<(), HsaError> {
    if std::mem::size_of_val(&**self) == 0 {
      // These are never actually allocated, so we can just say they're
      // always accessible: no delayed allocations for boxes.
      return Ok(());
    }

    let ptr = unsafe { self.pool_ptr() };
    add_access(self.alloc_ref(), device, ptr)
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::utils::test::*;

  #[test]
  fn ensure_send_sync() {
    fn check<T: Send + Sync>() { }
    check::<LapAlloc>();
  }

  #[test]
  fn lap_vec_late_alloc() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    // now alloc:
    m.resize(1, 0u32);

    let accessible = m.alloc_ref()
      .accessible()
      .unwrap()
      .len();
    assert_eq!(accessible, 1);
  }

  #[test]
  fn lap_box_zero_sized_alloc() {
    let dev = device();

    let mut m = LapBox::new_in((), dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    assert!(m.alloc_ref().accessible().is_none());
  }
  #[test]
  fn lap_vec_clone_access() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    // now alloc:
    m.resize(1, 0u32);

    let m2 = m.clone();
    assert!(m2.is_accessible_to(&dev));
  }
}
