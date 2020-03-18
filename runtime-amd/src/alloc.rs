
use alloc_wg::alloc::*;

use super::*;

#[derive(Clone, Debug)]
pub struct LapAlloc {
  pub(crate) id: AcceleratorId,
  pub(crate) device_agent: Agent,
  pub(crate) pool: MemoryPoolAlloc,
  /// Granting devices access is a really expensive syscall (more than a mmap), so this
  /// is here to avoid that where possible.
  /// None by default to avoid an allocation if eg a LapVec is constructed but
  /// never pushed to.
  pub(crate) accessible: Option<Arc<Vec<Agent>>>,
}
impl LapAlloc {
  pub fn alloc_pool(&self) -> &MemoryPoolAlloc {
    &self.pool
  }
  pub fn pool(&self) -> &MemoryPool {
    &self.pool
  }
}
impl From<Arc<HsaAmdGpuAccel>> for LapAlloc {
  fn from(accel: Arc<HsaAmdGpuAccel>) -> Self {
    accel.fine_lap_node_alloc(0)
  }
}
impl AllocRef for LapAlloc {
  type Error = HsaError;

  fn alloc(&mut self, layout: NonZeroLayout)
    -> Result<NonNull<u8>, Self::Error>
  {
    let out_ptr = self.pool.alloc(layout)?;

    if let Some(ref accessible) = self.accessible {
      // Handle cases where `LapAllocAccess::add_access` is called before allocation.
      // We don't need to check accessibility: it was already checked in
      // LapAllocAccess::add_access.
      let pool_ptr = unsafe {
        MemoryPoolPtr::from_ptr(self.pool().clone(), out_ptr)
      };
      pool_ptr.grant_agents_access(&accessible)?;
    }

    Ok(out_ptr)
  }

  #[inline]
  fn usable_size(&self, layout: NonZeroLayout) -> (usize, usize) {
    self.pool.usable_size(layout)
  }
}
impl DeallocRef for LapAlloc {
  type BuildAlloc = Self;
  fn get_build_alloc(&mut self) -> Self::BuildAlloc {
    self.clone()
  }
  unsafe fn dealloc(&mut self, ptr: NonNull<u8>,
                    layout: NonZeroLayout) {
    self.pool.dealloc(ptr, layout)
  }
}
impl BuildAllocRef for LapAlloc {
  type Ref = Self;
  unsafe fn build_alloc_ref(&mut self,
                            _ptr: NonNull<u8>,
                            _layout: Option<NonZeroLayout>)
    -> Self::Ref
  {
    self.clone()
  }
}
impl ReallocRef for LapAlloc { }
impl Abort for LapAlloc { }

#[doc(hidden)] #[inline(always)]
fn add_access(build_alloc: &mut LapAlloc,
              device: &HsaAmdGpuAccel,
              pool_ptr: Option<MemoryPoolPtr<[u8]>>)
  -> Result<(), HsaError>
{
  let already_accessible = build_alloc
    .accessible
    .iter()
    .flat_map(|i| i.iter() )
    .any(|a| a == device.agent() );
  if already_accessible {
    // nothing to do.
    return Ok(());
  }

  // add this device to the accessible array: We need to do this now for later
  // allocations (in the case of a LapVec).
  if build_alloc.accessible.is_none() {
    build_alloc.accessible = Some(Arc::default());
  }
  let accessible = Arc::make_mut(build_alloc.accessible.as_mut().unwrap());
  accessible.push(device.agent().clone());

  let pool = &build_alloc.pool;
  let aa = pool.agent_access(device.agent())?;
  if aa.never_allowed() {
    error!("pool {:?} not sharable with this device ({:?})",
           pool, device.id());
    return Err(HsaError::General);
  } else if aa.default_disallowed() {
    if let Some(pool_ptr) = pool_ptr {
      pool_ptr.grant_agents_access(&accessible)?;
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
  fn build_alloc_mut(&mut self) -> &mut LapAlloc;

  /// TODO: it would be nice to allow granting new devices access (which is expensive)
  /// concurrently. This is safe because only removal is concurrent-unsafe.
  fn add_access(&mut self, device: &HsaAmdGpuAccel) -> Result<(), HsaError> {
    let ptr = unsafe { self.pool_ptr() };
    add_access(self.build_alloc_mut(), device, ptr)
  }
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
  fn build_alloc_mut(&mut self) -> &mut LapAlloc {
    self.build_alloc_mut()
  }

  fn add_access(&mut self, device: &HsaAmdGpuAccel) -> Result<(), HsaError> {
    if std::mem::size_of::<T>() == 0 {
      // These are never actually allocated, so we can just say they're
      // always accessible.
      return Ok(());
    }

    let ptr = unsafe { self.pool_ptr() };
    add_access(self.build_alloc_mut(), device, ptr)
  }
}
impl<T> LapAllocAccess for LapBox<T>
  where T: ?Sized,
{
  fn build_alloc_mut(&mut self) -> &mut LapAlloc {
    self.build_alloc_mut()
  }

  fn add_access(&mut self, device: &HsaAmdGpuAccel) -> Result<(), HsaError> {
    if std::mem::size_of_val(&**self) == 0 {
      // These are never actually allocated, so we can just say they're
      // always accessible: no delayed allocations for boxes.
      return Ok(());
    }

    let ptr = unsafe { self.pool_ptr() };
    add_access(self.build_alloc_mut(), device, ptr)
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::utils::test::*;

  #[test]
  fn lap_vec_late_alloc() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    // now alloc:
    m.resize(1, 0u32);

    let accessible = m.build_alloc()
      .accessible
      .as_ref()
      .unwrap()
      .len();
    assert_eq!(accessible, 1);
  }

  #[test]
  fn lap_box_zero_sized_alloc() {
    let dev = device();

    let mut m = LapBox::new_in((), dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    assert!(m.build_alloc().accessible.is_none());
  }
}
