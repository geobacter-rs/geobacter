
use std::error::Error;
use std::mem;
use std::ptr::{NonNull, slice_from_raw_parts_mut, };
use std::rc::Rc;
use std::sync::Arc;

use hsa_rt::ext::amd::{MemoryPoolPtr, };
use hsa_rt::signal::SignalRef;

use crate::{HsaAmdGpuAccel, HsaError, AcceleratorId, };
use crate::alloc::*;
use crate::boxed::RawPoolBox;
use crate::module::{Deps, CallError, };
use crate::signal::*;

pub trait BoxPoolPtr {
  #[doc(hidden)]
  unsafe fn pool_ptr(&self) -> Option<MemoryPoolPtr<[u8]>>;
}
impl<T> BoxPoolPtr for MemoryPoolPtr<[T]> {
  #[doc(hidden)]
  unsafe fn pool_ptr(&self) -> Option<MemoryPoolPtr<[u8]>> {
    if self.len() == 0 {
      None
    } else {
      Some(self.into_bytes())
    }
  }
}
impl<T> BoxPoolPtr for LapBox<T>
  where T: ?Sized,
{
  #[doc(hidden)]
  unsafe fn pool_ptr(&self) -> Option<MemoryPoolPtr<[u8]>> {
    if mem::size_of_val(&**self) == 0 { return None; }

    let ptr = NonNull::new(&**self as *const T as *mut T)?;
    let pool = *self.build_alloc().pool();

    Some(MemoryPoolPtr::from_ptr(pool, ptr).into_bytes())
  }
}
impl<T> BoxPoolPtr for LapVec<T> {
  #[doc(hidden)]
  unsafe fn pool_ptr(&self) -> Option<MemoryPoolPtr<[u8]>> {
    if self.capacity() == 0 || mem::size_of::<T>() == 0 { return None; }

    let ptr = slice_from_raw_parts_mut(self.as_ptr() as *mut T, self.capacity());
    let ptr = NonNull::new(ptr)?;
    let pool = *self.build_alloc().pool();

    Some(MemoryPoolPtr::from_ptr(pool, ptr).into_bytes())
  }
}

/// A host to device memory transfer. Can be used as a queue dependency.
/// You don't construct this type directly; an implementation of `H2DMemcpyGroup`
/// will do it for you.
#[derive(Clone, Debug)]
#[must_use]
pub struct H2DMemoryTransfer<S, H, D, R = ()>
  where S: SignalHandle,
        H: ?Sized,
        R: Deps,
{
  deps: R,
  transfer: S,
  dst: D,
  src: H,
}
pub type H2DDeviceMemTransfer<H, D, R> = H2DMemoryTransfer<Arc<DeviceSignal>, H, D, R>;
pub type H2DDeviceLapBoxMemTransfer<T, R> =
  <LapBox<T> as H2DMemcpyGroup<Arc<DeviceSignal>, R>>::Transfer;
pub type H2DDeviceRLapBoxMemTransfer<'a, T, R> =
  <&'a LapBox<T> as H2DMemcpyGroup<Arc<DeviceSignal>, R>>::Transfer;
pub type H2DDeviceLapVecMemTransfer<T, R> =
  <LapVec<T> as H2DMemcpyGroup<Arc<DeviceSignal>, R>>::Transfer;
pub type H2DDeviceRLapVecMemTransfer<'a, T, R> =
  <&'a LapVec<T> as H2DMemcpyGroup<Arc<DeviceSignal>, R>>::Transfer;

pub type H2DGlobalMemTransfer<H, D, R> = H2DMemoryTransfer<Arc<GlobalSignal>, H, D, R>;
pub type H2DGlobalLapBoxMemTransfer<T, R> =
  <LapBox<T> as H2DMemcpyGroup<Arc<GlobalSignal>, R>>::Transfer;
pub type H2DGlobalRLapBoxMemTransfer<'a, T, R> =
  <&'a LapBox<T> as H2DMemcpyGroup<Arc<GlobalSignal>, R>>::Transfer;
pub type H2DGlobalLapVecMemTransfer<T, R> =
  <LapVec<T> as H2DMemcpyGroup<Arc<GlobalSignal>, R>>::Transfer;
pub type H2DGlobalRLapVecMemTransfer<'a, T, R> =
  <&'a LapVec<T> as H2DMemcpyGroup<Arc<GlobalSignal>, R>>::Transfer;

impl<S, H, D, R> H2DMemoryTransfer<S, H, D, R>
  where S: SignalHandle,
        H: ?Sized,
        R: Deps,
{
  pub fn src(&self) -> &H { &self.src }
  pub fn dst(&self) -> &D { &self.dst }
}
impl<'a, S, H, D, R> H2DMemoryTransfer<&'a S, H, D, R>
  where S: Clone + SignalHandle,
        R: Deps,
{
  pub fn cloned_signal(self) -> H2DMemoryTransfer<S, H, D, R> {
    use std::mem::forget;
    use std::ptr::*;

    unsafe {
      let out = H2DMemoryTransfer {
        deps: read(&self.deps),
        transfer: self.transfer.clone(),
        dst: read(&self.dst),
        src: read(&self.src),
      };

      forget(self);

      out
    }
  }
}
impl<S, H, D, R> Drop for H2DMemoryTransfer<S, H, D, R>
  where S: SignalHandle,
        H: ?Sized,
        R: Deps,
{
  fn drop(&mut self) {
    if self.signal_ref().load_relaxed() == 0 { return; }

    if let Some(host) = self.transfer.as_host_consumable() {
      if let Err(code) = host.wait_for_zero(false) {
        log::error!("got negative signal in mem transfer drop: {}", code);
      }
    } else {
      assert_eq!(self.signal_ref().load_scacquire(), 0);
    }
  }
}
unsafe impl<S, H, D, R> Deps for H2DMemoryTransfer<S, H, D, R>
  where S: SignalHandle + Deps,
        H: ?Sized,
        R: Deps,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    // `self.transfer` already depends on `self.deps`.
    self.transfer.iter_deps(f)
  }
}
impl<S, H, D, R> SignalHandle for H2DMemoryTransfer<S, H, D, R>
  where S: SignalHandle,
        H: ?Sized,
        R: Deps,
{
  fn signal_ref(&self) -> &SignalRef { self.transfer.signal_ref() }
  #[doc(hidden)]
  #[inline(always)]
  unsafe fn mark_consumed(&self) {
    self.transfer.mark_consumed()
  }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    self.transfer.as_host_consumable()
  }
}
impl<S, H, D, R> DeviceConsumable for H2DMemoryTransfer<S, H, D, R>
  where S: DeviceConsumable,
        H: ?Sized,
        R: Deps,
{
  fn usable_on_device(&self, id: AcceleratorId) -> bool {
    self.transfer.usable_on_device(id)
  }
}
impl<S, H, D, R> HostConsumable for H2DMemoryTransfer<S, H, D, R>
  where S: HostConsumable,
        H: ?Sized,
        R: Deps,
{ }

pub trait H2DMemcpyObject {
  type RemoteBox;

  unsafe fn alloc_for_dev(&self, device: &Arc<HsaAmdGpuAccel>)
    -> Result<Self::RemoteBox, HsaError>;

  /// This assumes the signal is already setup properly.
  unsafe fn unchecked_memcopy_with_signal<S, D>(self,
                                                device: &Arc<HsaAmdGpuAccel>,
                                                deps: D,
                                                signal: S)
    -> Result<H2DMemoryTransfer<S, Self, Self::RemoteBox, D>, Box<dyn Error>>
    where Self: Sized,
          S: SignalHandle,
          D: Deps;
}
impl<'a, T> H2DMemcpyObject for &'a T
  where T: H2DMemcpyObject + BoxPoolPtr + Deps,
        T::RemoteBox: BoxPoolPtr,
{
  type RemoteBox = T::RemoteBox;

  #[inline(always)]
  unsafe fn alloc_for_dev(&self, device: &Arc<HsaAmdGpuAccel>)
    -> Result<Self::RemoteBox, HsaError>
  {
    (&**self).alloc_for_dev(device)
  }
  unsafe fn unchecked_memcopy_with_signal<S, D>(self,
                                                device: &Arc<HsaAmdGpuAccel>,
                                                deps: D,
                                                signal: S)
    -> Result<H2DMemoryTransfer<S, Self, Self::RemoteBox, D>, Box<dyn Error>>
    where Self: Sized,
          S: SignalHandle,
          D: Deps,
  {
    let mut alloc = self.alloc_for_dev(device)?;

    device.unchecked_async_copy_into(self, &mut alloc,
                                     &(&deps, self), &signal)?;

    Ok(H2DMemoryTransfer {
      deps,
      transfer: signal,
      src: self,
      dst: alloc,
    })
  }
}
impl<T> H2DMemcpyObject for LapBox<T>
  where T: Sized + Copy + Deps,
{
  type RemoteBox = RawPoolBox<T>;

  #[inline(always)]
  unsafe fn alloc_for_dev(&self, device: &Arc<HsaAmdGpuAccel>)
    -> Result<Self::RemoteBox, HsaError>
  {
    let pool = *device.device_pool();
    let b = RawPoolBox::new_uninit(pool)?;
    Ok(b)
  }
  unsafe fn unchecked_memcopy_with_signal<S, D>(self,
                                                device: &Arc<HsaAmdGpuAccel>,
                                                deps: D,
                                                signal: S)
    -> Result<H2DMemoryTransfer<S, Self, Self::RemoteBox, D>, Box<dyn Error>>
    where Self: Sized,
          S: SignalHandle,
          D: Deps,
  {
    let mut alloc = self.alloc_for_dev(device)?;

    device.unchecked_async_copy_into(&self, &mut alloc, &(&deps, &self), &signal)?;

    Ok(H2DMemoryTransfer {
      deps,
      transfer: signal,
      src: self,
      dst: alloc,
    })
  }
}
impl<T> H2DMemcpyObject for LapBox<[T]>
  where T: Sized + Copy + Deps,
{
  type RemoteBox = RawPoolBox<[T]>;

  #[inline(always)]
  unsafe fn alloc_for_dev(&self, device: &Arc<HsaAmdGpuAccel>)
    -> Result<Self::RemoteBox, HsaError>
  {
    let pool = device.device_pool()
      .allocator()?;
    let b = RawPoolBox::new_uninit_slice(pool, self.len())?;
    Ok(b)
  }
  unsafe fn unchecked_memcopy_with_signal<S, D>(self,
                                                device: &Arc<HsaAmdGpuAccel>,
                                                deps: D,
                                                signal: S)
    -> Result<H2DMemoryTransfer<S, Self, Self::RemoteBox, D>, Box<dyn Error>>
    where Self: Sized,
          S: SignalHandle,
          D: Deps,
  {
    let mut alloc = self.alloc_for_dev(device)?;

    device.unchecked_async_copy_into(&self, &mut alloc,
                                     &(&deps, &self), &signal)?;

    Ok(H2DMemoryTransfer {
      deps,
      transfer: signal,
      src: self,
      dst: alloc,
    })
  }
}
impl<T> H2DMemcpyObject for LapVec<T>
  where T: Sized + Copy + Deps,
{
  type RemoteBox = RawPoolBox<[T]>;

  #[inline(always)]
  unsafe fn alloc_for_dev(&self, device: &Arc<HsaAmdGpuAccel>)
    -> Result<Self::RemoteBox, HsaError>
  {
    let pool = device.device_pool()
        .allocator()?;
    let b = RawPoolBox::new_uninit_slice(pool, self.len())?;
    Ok(b)
  }
  unsafe fn unchecked_memcopy_with_signal<S, D>(self,
                                                device: &Arc<HsaAmdGpuAccel>,
                                                deps: D,
                                                signal: S)
    -> Result<H2DMemoryTransfer<S, Self, Self::RemoteBox, D>, Box<dyn Error>>
    where Self: Sized,
          S: SignalHandle,
          D: Deps,
  {
    let mut alloc = self.alloc_for_dev(device)?;

    device.unchecked_async_copy_into(&self, &mut alloc,
                                     &(&deps, &self), &signal)?;

    Ok(H2DMemoryTransfer {
      deps,
      transfer: signal,
      src: self,
      dst: alloc,
    })
  }
}

pub trait H2DMemcpyGroup<S, D>
  where S: SignalHandle + ResettableSignal + Clone,
        D: Deps,
{
  type Transfer;

  fn signal_len(&self) -> Value;

  unsafe fn unchecked_memcopy(self, device: &Arc<HsaAmdGpuAccel>,
                              deps: D, signal: S)
    -> Result<Self::Transfer, Box<dyn Error>>;

  fn memcopy(self, device: &Arc<HsaAmdGpuAccel>, deps: D, signal: &mut S)
    -> Result<Self::Transfer, Box<dyn Error>>
    where Self: Sized,
  {
    signal.reset(device, self.signal_len())?;
    unsafe {
      self.unchecked_memcopy(device, deps, signal.clone())
    }
  }
}

impl<T, S, D> H2DMemcpyGroup<Arc<S>, D> for T
  where T: H2DMemcpyObject,
        S: SignalHandle + ResettableSignal + SignalFactory,
        D: Deps,
{
  type Transfer = H2DMemoryTransfer<Arc<S>, T, T::RemoteBox, D>;

  fn signal_len(&self) -> Value {
    1
  }

  unsafe fn unchecked_memcopy(self, device: &Arc<HsaAmdGpuAccel>,
                              deps: D, signal: Arc<S>)
    -> Result<Self::Transfer, Box<dyn Error>>
  {
    self.unchecked_memcopy_with_signal(device, deps, signal)
  }
}
impl<T, S, D> H2DMemcpyGroup<Rc<S>, D> for T
  where T: H2DMemcpyObject,
        S: SignalHandle + ResettableSignal + SignalFactory,
        D: Deps,
{
  type Transfer = H2DMemoryTransfer<Rc<S>, T, T::RemoteBox, D>;

  fn signal_len(&self) -> Value {
    1
  }

  unsafe fn unchecked_memcopy(self, device: &Arc<HsaAmdGpuAccel>,
                              deps: D, signal: Rc<S>)
    -> Result<Self::Transfer, Box<dyn Error>>
  {
    self.unchecked_memcopy_with_signal(device, deps, signal)
  }
}
impl<L, R, S, D> H2DMemcpyGroup<Arc<S>, D> for (L, R)
  where L: H2DMemcpyGroup<Arc<S>, D>,
        R: H2DMemcpyGroup<Arc<S>, D>,
        S: SignalHandle + ResettableSignal + SignalFactory,
        D: Clone + Deps,
{
  type Transfer = (L::Transfer, R::Transfer);

  fn signal_len(&self) -> Value {
    self.0.signal_len() + self.1.signal_len()
  }

  unsafe fn unchecked_memcopy(self, device: &Arc<HsaAmdGpuAccel>,
                              deps: D, signal: Arc<S>)
    -> Result<Self::Transfer, Box<dyn Error>>
  {
    Ok((self.0.unchecked_memcopy(device, deps.clone(),
                                 signal.clone())?,
        self.1.unchecked_memcopy(device, deps,
                                 signal)?))
  }
}
impl<L, R, S, D> H2DMemcpyGroup<Rc<S>, D> for (L, R)
  where L: H2DMemcpyGroup<Rc<S>, D>,
        R: H2DMemcpyGroup<Rc<S>, D>,
        S: SignalHandle + ResettableSignal + SignalFactory,
        D: Clone + Deps,
{
  type Transfer = (L::Transfer, R::Transfer);

  fn signal_len(&self) -> Value {
    self.0.signal_len() + self.1.signal_len()
  }

  unsafe fn unchecked_memcopy(self, device: &Arc<HsaAmdGpuAccel>,
                              deps: D, signal: Rc<S>)
    -> Result<Self::Transfer, Box<dyn Error>>
  {
    Ok((self.0.unchecked_memcopy(device, deps.clone(),
                                 signal.clone())?,
        self.1.unchecked_memcopy(device, deps,
                                 signal)?))
  }
}

pub trait MemcpyGroupTuple: Sized {
  fn chain<R>(self, next: R) -> (Self, R) {
    (self, next)
  }
}
impl<L, R> MemcpyGroupTuple for (L, R)
{ }
impl<L> MemcpyGroupTuple for L
{ }

#[cfg(test)]
mod test {
  use alloc_wg::iter::*;

  use crate::*;
  use crate::utils::test::*;
  use crate::signal::*;

  use super::*;

  #[test]
  fn zero_sized_box() {
    let dev = device();

    let b = LapBox::new_in((), dev.fine_lap_node_alloc(0));
    assert!(unsafe { b.pool_ptr().is_none() });
  }
  #[test]
  fn zero_sized_boxed_slice() {
    let dev = device();

    let b: LapVec<u32> = LapVec::new_in(dev.fine_lap_node_alloc(0));
    let b = b.into_boxed_slice();

    assert!(unsafe { b.pool_ptr().is_none() });

    let mut b: LapVec<()> = LapVec::new_in(dev.fine_lap_node_alloc(0));
    b.resize(1, ());
    let b = b.into_boxed_slice();

    assert!(unsafe { b.pool_ptr().is_none() });
  }
  #[test]
  fn zero_sized_vec() {
    let dev = device();

    let v: LapVec<()> = LapVec::with_capacity_in(1, dev.fine_lap_node_alloc(0));
    assert!(unsafe { v.pool_ptr().is_none() });
  }
  #[test]
  fn vec_capacity() {
    let dev = device();

    let mut v: LapVec<u32> = LapVec::new_in(dev.fine_lap_node_alloc(0));
    assert!(unsafe { v.pool_ptr().is_none() });
    v.reserve(1);
    assert!(unsafe { v.pool_ptr().is_some() });
  }

  #[test]
  fn single() {
    let device = device();

    let mut mem = LapVec::from_iter_in(0usize..4096,
                                       device.fine_lap_node_alloc(0));
    mem.add_access(&device).unwrap();

    let mut signal = Arc::new(GlobalSignal::new(5).unwrap());
    let dep = Arc::new(GlobalSignal::new(1).unwrap());

    let transfer = mem.memcopy(&device, dep.clone(), &mut signal)
      .unwrap();

    assert_eq!(signal.load_scacquire(), 1);
    dep.store_screlease(0);

    transfer.wait_for_zero(false).unwrap();
    assert_eq!(signal.load_scacquire(), 0);
  }

  #[test]
  fn two() {
    let device = device();

    let mut mem1 = LapVec::from_iter_in(0usize..4096,
                                   device.fine_lap_node_alloc(0));
    let mut mem2 = LapVec::from_iter_in(0u32..4096,
                                   device.fine_lap_node_alloc(0));
    mem1.add_access(&device).unwrap();
    mem2.add_access(&device).unwrap();

    let mut signal = Arc::new(GlobalSignal::new(5).unwrap());
    let dep = Arc::new(GlobalSignal::new(1).unwrap());

    let mem = mem1.chain(mem2);

    let (t1, t2) = mem
      .memcopy(&device, dep.clone(), &mut signal)
      .unwrap();

    assert_eq!(signal.signal_ref(), t1.signal_ref());
    assert_eq!(signal.signal_ref(), t2.signal_ref());

    assert_eq!(signal.load_scacquire(), 2);
    dep.store_screlease(0);

    signal.wait_for_zero(false).unwrap();
    assert_eq!(signal.load_scacquire(), 0);
  }

  #[test]
  fn multiple() {
    let device = device();

    let mut mem1 = LapVec::from_iter_in(0usize..4096,
                                        device.fine_lap_node_alloc(0));
    let mut mem2 = LapVec::from_iter_in(0u32..4096,
                                        device.fine_lap_node_alloc(0));
    let mut mem3 = LapVec::from_iter_in(0i32..4096,
                                        device.fine_lap_node_alloc(0));
    mem1.add_access(&device).unwrap();
    mem2.add_access(&device).unwrap();
    mem3.add_access(&device).unwrap();

    let mut signal = Arc::new(GlobalSignal::new(5).unwrap());
    let dep = Arc::new(GlobalSignal::new(1).unwrap());

    let mem = mem1
      .chain(mem2)
      .chain(mem3);

    let ((t1, _t2), _t3) = mem
      .memcopy(&device, dep.clone(), &mut signal)
      .unwrap();

    assert_eq!(signal.load_scacquire(), 3);
    dep.store_screlease(0);

    t1.wait_for_zero(false).unwrap();
    assert_eq!(signal.load_scacquire(), 0);
  }

  #[test]
  fn reset() {
    let device = device();

    let mut mem = LapVec::from_iter_in(0usize..4096,
                                       device.fine_lap_node_alloc(0));
    mem.add_access(&device).unwrap();

    let mut signal = Arc::new(GlobalSignal::new(5).unwrap());
    let signal2 = signal.clone();
    let dep = Arc::new(GlobalSignal::new(1).unwrap());

    let transfer = mem.memcopy(&device, dep.clone(), &mut signal)
      .unwrap();

    assert_eq!(signal2.load_scacquire(), 5);
    assert_ne!(signal2.signal_ref(), transfer.signal_ref());
    assert_eq!(signal.signal_ref(), transfer.signal_ref());

    assert_eq!(signal.load_scacquire(), 1);
    dep.store_screlease(0);

    transfer.wait_for_zero(false).unwrap();
    assert_eq!(signal.load_scacquire(), 0);
  }
}
