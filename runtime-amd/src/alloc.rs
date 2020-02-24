use std::ptr::slice_from_raw_parts_mut;

use alloc_wg::alloc::*;

use super::*;

#[derive(Clone, Debug)]
pub struct LapAlloc {
  pub(crate) id: AcceleratorId,
  pub(crate) device_agent: Agent,
  pub(crate) pool: MemoryPoolAlloc,
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
    unsafe {
      let out_ptr = self.pool.alloc(layout)?;
      let ptr = slice_from_raw_parts_mut(out_ptr.as_ptr(), layout.size().get());
      let ptr = NonNull::new_unchecked(ptr);
      let pool_ptr = MemoryPoolPtr::from_ptr(self.pool.pool(), ptr);

      // TODO? We have to share this now; we don't get another opportunity to do this.
      // This next part takes the majority of the time in this function (like ~4/5 of
      // all time spent).
      let aa = self.pool.pool().agent_access(&self.device_agent)?;
      if aa.never_allowed() {
        error!("pool {:?} not sharable with this device ({:?})",
               self.pool, self.id);
        self.pool.dealloc(out_ptr, layout);
        return Err(HsaError::General);
      } else if aa.default_disallowed() {
        match pool_ptr.grant_agent_access(&self.device_agent) {
          Ok(()) => { },
          Err(e) => {
            self.pool.dealloc(out_ptr, layout);
            return Err(e);
          },
        }
      } else if aa.default_allowed() {
        // nothing to do here
      } else {
        warn!("unknown default access: {:?}", aa);
        self.pool.dealloc(out_ptr, layout);
        return Err(HsaError::General);
      }

      Ok(out_ptr)
    }
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
