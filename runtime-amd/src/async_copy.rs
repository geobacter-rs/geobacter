
use hsa_rt::ext::amd::{MemoryPoolPtr, };

pub trait CopyDataObject<T>
  where T: ?Sized + Unpin,
{
  #[doc(hidden)]
  unsafe fn pool_copy_region(&self) -> Option<MemoryPoolPtr<[u8]>>;
}
impl<'a, T, U> CopyDataObject<U> for &'a T
  where T: ?Sized + CopyDataObject<U> + 'a,
        U: ?Sized + Unpin,
{
  #[doc(hidden)]
  unsafe fn pool_copy_region(&self) -> Option<MemoryPoolPtr<[u8]>> {
    (&**self).pool_copy_region()
  }
}
impl<'a, T, U> CopyDataObject<U> for &'a mut T
  where T: ?Sized + CopyDataObject<U> + 'a,
        U: ?Sized + Unpin,
{
  #[doc(hidden)]
  unsafe fn pool_copy_region(&self) -> Option<MemoryPoolPtr<[u8]>> {
    (&**self).pool_copy_region()
  }
}
impl<T> CopyDataObject<[T]> for MemoryPoolPtr<[T]>
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
