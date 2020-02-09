
use hsa_rt::ext::amd::{MemoryPoolPtr, };

use crate::{HsaAmdGpuAccel, HsaError, };

pub trait CopyDataObject {
  #[doc(hidden)]
  unsafe fn pool_copy_region(&self) -> Option<MemoryPoolPtr<[u8]>>;
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
