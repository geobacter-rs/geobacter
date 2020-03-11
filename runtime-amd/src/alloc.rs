
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

/// No device removal.
pub trait AllocAccess: BoxPoolPtr {
  fn build_alloc_mut(&mut self) -> &mut LapAlloc;

  /// TODO: it would be nice to allow granting new devices access (which is expensive)
  /// concurrently. This is safe because only removal is concurrent-unsafe.
  fn add_access(&mut self, device: &HsaAmdGpuAccel) -> Result<(), HsaError> {
    let already_accessible = self.build_alloc_mut()
      .accessible
      .iter()
      .flat_map(|i| i.iter() )
      .any(|a| a == device.agent() );
    if already_accessible {
      // nothing to do.
      return Ok(());
    }

    let pool_ptr = unsafe { self.pool_ptr() };
    if let None = pool_ptr { return Ok(()); }
    let pool_ptr = pool_ptr.unwrap();
    let pool = pool_ptr.pool();

    let alloc = self.build_alloc_mut();
    if alloc.accessible.is_none() {
      alloc.accessible = Some(Arc::default());
    }
    let accessible = Arc::make_mut(alloc.accessible.as_mut().unwrap());
    accessible.push(device.agent().clone());

    let aa = pool.agent_access(device.agent())?;
    if aa.never_allowed() {
      error!("pool {:?} not sharable with this device ({:?})",
             pool, device.id());
      return Err(HsaError::General);
    } else if aa.default_disallowed() {
      pool_ptr.grant_agents_access(&accessible)?;
    } else if aa.default_allowed() {
      // nothing to do
    } else {
      unreachable!("{:?}", aa);
    }

    Ok(())
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

impl<T> AllocAccess for LapVec<T> {
  fn build_alloc_mut(&mut self) -> &mut LapAlloc {
    self.build_alloc_mut()
  }
}
impl<T> AllocAccess for LapBox<T>
  where T: ?Sized,
{
  fn build_alloc_mut(&mut self) -> &mut LapAlloc {
    self.build_alloc_mut()
  }
}
