
use std::error::Error;
use std::marker::PhantomData;

use hsa_rt::ext::amd::{MemoryPool, MemoryPoolPtr};
use hsa_rt::signal::SignalRef;

use grt_core::{AcceleratorId, };

use crate::signal::{SignalHandle, DepSignal, DeviceConsumable, HostConsumable};
use crate::{HsaAmdGpuAccel, HsaError};

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

pub trait SourceDataObject<T>: CopyDataObject<T>
  where T: ?Sized + Unpin,
{
  fn src_pool(&self) -> MemoryPool;
}
impl<'a, T, U> SourceDataObject<U> for &'a T
  where T: ?Sized + SourceDataObject<U> + 'a,
        U: ?Sized + Unpin,
{
  fn src_pool(&self) -> MemoryPool { (&**self).src_pool() }
}

pub trait DestDataObject<T>: CopyDataObject<T>
  where T: ?Sized + Unpin,
{
  fn dst_pool(&self) -> MemoryPool;
}
impl<'a, T, U> DestDataObject<U> for &'a mut T
  where T: ?Sized + DestDataObject<U> + 'a,
        U: ?Sized + Unpin,
{
  fn dst_pool(&self) -> MemoryPool { (&**self).dst_pool() }
}

impl HsaAmdGpuAccel {
  /// XXX still unsafe. TODO acquire the src and dst resources from
  /// `DepSignal`s instead of separate resource/deps arguments.
  pub fn preview_async_copy_into<T, Src, Dst, DS, CS>(&self,
                                                      src: Src,
                                                      dst: Dst,
                                                      deps: Vec<DS>,
                                                      completion: CS)
  // XXX Box<dyn Error>
    -> Result<AsyncCopy<T, Src, Dst, DS, CS>, Box<dyn Error>>
    where T: ?Sized + Unpin,
          Src: SourceDataObject<T>,
          Dst: DestDataObject<T>,
          DS: DeviceConsumable,
          CS: SignalHandle,
  {
    // most of the time, there will be at most just two deps: one
    // for the src and one for the dst.
    // TODO

    let src_ptr = unsafe { src.pool_copy_region() };
    if src_ptr.is_none() { return Err(HsaError::InvalidArgument.into()); }
    let src_ptr = src_ptr.unwrap();
    let dst_ptr = unsafe { dst.pool_copy_region() };
    if dst_ptr.is_none() { return Err(HsaError::InvalidArgument.into()); }
    let dst_ptr = dst_ptr.unwrap();

    unsafe {
      // XXX HsaAmdGpuAccel::unchecked_async_copy_into also allocates
      // a deps vec.
      let deps_dyn: Vec<_> = deps.iter()
        .map(|v| v as &dyn DeviceConsumable )
        .collect();
      self.unchecked_async_copy_into(src_ptr, dst_ptr, &deps_dyn,
                                     &completion)?;
    }

    Ok(AsyncCopy {
      deps,
      src_data: src,
      dest_data: dst,
      completion,
      _ugh: PhantomData,
    })
  }
  pub fn preview_async_copy_from<T, Src, Dst, DS, CS>(&self,
                                                      src: Src,
                                                      dst: Dst,
                                                      deps: Vec<DS>,
                                                      completion: CS)
  // XXX Box<dyn Error>
    -> Result<AsyncCopy<T, Src, Dst, DS, CS>, Box<dyn Error>>
    where T: ?Sized + Unpin,
          Src: SourceDataObject<T>,
          Dst: DestDataObject<T>,
          DS: DeviceConsumable,
          CS: SignalHandle,
  {
    // most of the time, there will be at most just two deps: one
    // for the src and one for the dst.
    // TODO

    let src_ptr = unsafe { src.pool_copy_region() };
    if src_ptr.is_none() { return Err(HsaError::InvalidArgument.into()); }
    let src_ptr = src_ptr.unwrap();
    let dst_ptr = unsafe { dst.pool_copy_region() };
    if dst_ptr.is_none() { return Err(HsaError::InvalidArgument.into()); }
    let dst_ptr = dst_ptr.unwrap();

    unsafe {
      // XXX HsaAmdGpuAccel::unchecked_async_copy_into also allocates
      // a deps vec.
      let deps_dyn: Vec<_> = deps.iter()
        .map(|v| v as &dyn DeviceConsumable )
        .collect();
      self.unchecked_async_copy_from(src_ptr, dst_ptr, &deps_dyn,
                                     &completion)?;
    }

    Ok(AsyncCopy {
      deps,
      src_data: src,
      dest_data: dst,
      completion,
      _ugh: PhantomData,
    })
  }
}

/// Helper type to help you ensure all objects used in an async copy
/// remain alive until the copy finishes.
pub struct AsyncCopy<T, Src, Dst, DS, CS>
  where T: ?Sized + Unpin,
        Src: SourceDataObject<T>,
        Dst: DestDataObject<T>,
        DS: DeviceConsumable,
        CS: SignalHandle,
{
  deps: Vec<DS>,
  src_data: Src,
  dest_data: Dst,
  completion: CS,
  _ugh: PhantomData<*const T>,
}

impl<T, Src, Dst, DS, CS> DepSignal for AsyncCopy<T, Src, Dst, DS, CS>
  where T: ?Sized + Unpin,
        Src: SourceDataObject<T>,
        Dst: DestDataObject<T>,
        DS: DeviceConsumable,
        CS: DeviceConsumable,
{
  // what I'd like:
  //type Resource = (Src, Dst);
  // but sadly using that we can't support anything but unwrapping
  // (can't peek). Since src is only required to be read only,
  // this shouldn't constrain things too much.
  type Resource = Dst;

  unsafe fn peek_resource<F, R>(&self, f: F) -> R
    where F: FnOnce(&Self::Resource) -> R,
  {
    f(&self.dest_data)
  }
  unsafe fn peek_mut_resource(&mut self) -> &mut Self::Resource { &mut self.dest_data }
  unsafe fn unwrap_resource(self) -> Self::Resource {
    self.dest_data
  }
}
impl<T, Src, Dst, DS, CS> DeviceConsumable for AsyncCopy<T, Src, Dst, DS, CS>
  where T: ?Sized + Unpin,
        Src: SourceDataObject<T>,
        Dst: DestDataObject<T>,
        DS: DeviceConsumable,
        CS: DeviceConsumable,
{
  fn usable_on_device(&self, id: AcceleratorId) -> bool {
    self.completion.usable_on_device(id)
  }
}
impl<T, Src, Dst, DS, CS> HostConsumable for AsyncCopy<T, Src, Dst, DS, CS>
  where T: ?Sized + Unpin,
        Src: SourceDataObject<T>,
        Dst: DestDataObject<T>,
        DS: DeviceConsumable,
        CS: HostConsumable,
{ }
impl<T, Src, Dst, DS, CS> SignalHandle for AsyncCopy<T, Src, Dst, DS, CS>
  where T: ?Sized + Unpin,
        Src: SourceDataObject<T>,
        Dst: DestDataObject<T>,
        DS: DeviceConsumable,
        CS: SignalHandle,
{
  fn signal_ref(&self) -> &SignalRef {
    self.completion.signal_ref()
  }

  unsafe fn mark_consumed(&self) {
    for dep in self.deps.iter() {
      dep.mark_consumed();
    }
  }

  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    self.completion.as_host_consumable()
  }
}
