
use std::ptr::{NonNull, slice_from_raw_parts_mut, };

use hsa_rt::ext::amd::{MemoryPoolPtr, };

use crate::{HsaAmdGpuAccel, HsaError, };
use crate::alloc::*;

pub trait CopyDataObject {
  #[doc(hidden)]
  unsafe fn pool_copy_region(&self) -> Option<MemoryPoolPtr<[u8]>>;
  /// Does not change any memory in the pool; but we need to be sure that there are
  /// no other uses of this region.
  fn set_accessible(&mut self, accels: &[&HsaAmdGpuAccel])
    -> Result<(), HsaError>
  {
    let pool_ptr = match unsafe { self.pool_copy_region() } {
      None => { return Ok(()); },
      Some(ptr) => ptr,
    };
    if accels.len() == 0 {
      pool_ptr.grant_agents_access(&[])
    } else if accels.len() == 1 {
      pool_ptr.grant_agent_access(accels[0].agent())
    } else {
      let agents: Vec<_> = accels.iter()
        .map(|a| a.agent().clone() )
        .collect();
      pool_ptr.grant_agents_access(&agents)
    }
  }
}
impl<'a, T> CopyDataObject for &'a T
  where T: ?Sized + CopyDataObject + Unpin + 'a,
{
  #[doc(hidden)]
  unsafe fn pool_copy_region(&self) -> Option<MemoryPoolPtr<[u8]>> {
    (&**self).pool_copy_region()
  }
}
impl<'a, T> CopyDataObject for &'a mut T
  where T: ?Sized + CopyDataObject + Unpin + 'a,
{
  #[doc(hidden)]
  unsafe fn pool_copy_region(&self) -> Option<MemoryPoolPtr<[u8]>> {
    (&**self).pool_copy_region()
  }
}
impl<T> CopyDataObject for MemoryPoolPtr<[T]>
  where T: Unpin,
{
  #[doc(hidden)]
  unsafe fn pool_copy_region(&self) -> Option<MemoryPoolPtr<[u8]>> {
    if self.len() == 0 {
      None
    } else {
      Some(self.into_bytes())
    }
  }
}
impl<T> CopyDataObject for LapBox<T>
  where T: ?Sized,
{
  #[doc(hidden)]
  unsafe fn pool_copy_region(&self) -> Option<MemoryPoolPtr<[u8]>> {
    let ptr = NonNull::new(&**self as *const T as *mut T)?;
    let pool = *self.build_alloc().pool();

    Some(MemoryPoolPtr::from_ptr(pool, ptr).into_bytes())
  }
}
impl<T> CopyDataObject for LapVec<T> {
  #[doc(hidden)]
  unsafe fn pool_copy_region(&self) -> Option<MemoryPoolPtr<[u8]>> {
    let ptr = slice_from_raw_parts_mut(self.as_ptr() as *mut T, self.len());
    let ptr = NonNull::new(ptr)?;
    let pool = *self.build_alloc().pool();

    Some(MemoryPoolPtr::from_ptr(pool, ptr).into_bytes())
  }
}
