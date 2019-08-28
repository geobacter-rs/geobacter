
use std::ops::{Deref, };
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{fence, Ordering, };

use hsa_rt::error::Error as HsaError;
use hsa_rt::signal::{Signal, SignalRef, ConditionOrdering, WaitState, };

use lrt_core::AcceleratorId;

pub use hsa_rt::signal::Value;

pub trait SignalHandle {
  fn signal_ref(&self) -> &SignalRef;

  /// Do not call this on your own.
  /// Used to tell some types that their resources are no longer in
  /// by the accelerator. For example, when barrier packets complete
  /// this is used to tell the deps that they are free to free resources.
  #[doc = "hidden"]
  unsafe fn mark_consumed(&self);

  fn as_host_consumable(&self) -> Option<&dyn HostConsumable>;
}
impl<'a, T> SignalHandle for &'a T
  where T: SignalHandle + ?Sized,
{
  fn signal_ref(&self) -> &SignalRef { (**self).signal_ref() }
  unsafe fn mark_consumed(&self) { (**self).mark_consumed() }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    (**self).as_host_consumable()
  }
}
impl<T> SignalHandle for Rc<T>
  where T: SignalHandle + ?Sized,
{
  fn signal_ref(&self) -> &SignalRef { (**self).signal_ref() }
  unsafe fn mark_consumed(&self) { (**self).mark_consumed() }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    (**self).as_host_consumable()
  }
}
impl<T> SignalHandle for Arc<T>
  where T: SignalHandle + ?Sized,
{
  fn signal_ref(&self) -> &SignalRef { (**self).signal_ref() }
  unsafe fn mark_consumed(&self) { (**self).mark_consumed() }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    (**self).as_host_consumable()
  }
}
impl Deref for dyn SignalHandle {
  type Target = SignalRef;
  fn deref(&self) -> &SignalRef { self.signal_ref() }
}

#[derive(Debug, Eq, PartialEq)]
pub struct HostSignal(pub(crate) Signal);
impl Deref for HostSignal {
  type Target = SignalRef;
  fn deref(&self) -> &SignalRef {
    &*self.0
  }
}
impl SignalHandle for HostSignal {
  fn signal_ref(&self) -> &SignalRef { &**self }
  unsafe fn mark_consumed(&self) { }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    Some(self)
  }
}

#[derive(Debug, Eq, PartialEq)]
pub struct DeviceSignal(pub(crate) Signal, pub(crate) AcceleratorId);
impl Deref for DeviceSignal {
  type Target = SignalRef;
  fn deref(&self) -> &SignalRef {
    &*self.0
  }
}
impl SignalHandle for DeviceSignal {
  fn signal_ref(&self) -> &SignalRef { &**self }
  unsafe fn mark_consumed(&self) { }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> { None }
}

/// A signal handle which any device on this system can consume (wait on).
#[derive(Debug, Eq, PartialEq)]
pub struct GlobalSignal(pub(crate) Signal);

impl GlobalSignal {
  pub fn new(initial: Value) -> Result<Self, HsaError> {
    Signal::new(initial, &[])
      .map(GlobalSignal)
  }
}
impl Deref for GlobalSignal {
  type Target = SignalRef;
  fn deref(&self) -> &SignalRef {
    &*self.0
  }
}
impl SignalHandle for GlobalSignal {
  fn signal_ref(&self) -> &SignalRef { &**self }
  unsafe fn mark_consumed(&self) { }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    Some(self)
  }
}

/// A signal which may be waited on by accelerator devices.
pub trait DeviceConsumable: SignalHandle {
  fn usable_on_device(&self, id: AcceleratorId) -> bool;
}
impl<'a, T> DeviceConsumable for &'a T
  where T: DeviceConsumable + ?Sized,
{
  default fn usable_on_device(&self, id: AcceleratorId) -> bool {
    (&**self).usable_on_device(id)
  }
}
impl<T> DeviceConsumable for Rc<T>
  where T: DeviceConsumable + ?Sized,
{
  fn usable_on_device(&self, id: AcceleratorId) -> bool {
    (&**self).usable_on_device(id)
  }
}
impl<T> DeviceConsumable for Arc<T>
  where T: DeviceConsumable + ?Sized,
{
  fn usable_on_device(&self, id: AcceleratorId) -> bool {
    (&**self).usable_on_device(id)
  }
}

impl DeviceConsumable for DeviceSignal {
  fn usable_on_device(&self, id: AcceleratorId) -> bool {
    self.1 == id
  }
}
impl DeviceConsumable for GlobalSignal {
  fn usable_on_device(&self, _: AcceleratorId) -> bool {
    // we're usable on all devices
    true
  }
}
impl<'a> DeviceConsumable for &'a DeviceSignal {
  fn usable_on_device(&self, id: AcceleratorId) -> bool {
    self.1 == id
  }
}
impl<'a> DeviceConsumable for &'a GlobalSignal {
  fn usable_on_device(&self, _: AcceleratorId) -> bool {
    // we're usable on all devices
    true
  }
}

/// A signal which may be waited on by the host.
pub trait HostConsumable: SignalHandle {
  /// The signal can be set to negative numbers to indicate
  /// an error. Err(..) will be returned in this case.
  /// If `spin` is true, this thread will spin-wait, else
  /// this thread could block.
  ///
  /// # Safety
  ///
  /// This version does not execute a system wide acquire fence
  /// before returning. Without such a fence, reads made locally could
  /// be stale (ie not the same value written by the remote device when it
  /// completed this signal).
  /// If unsure, use `self.wait_for_zero`, below.
  unsafe fn wait_for_zero_relaxed(&self, spin: bool) -> Result<(), Value> {
    let r;

    loop {
      let wait = if spin {
        WaitState::Active
      } else {
        WaitState::Blocked
      };

      let val = self.signal_ref()
        .wait_relaxed(ConditionOrdering::Less,
                      1, None, wait);
      log::debug!("completion signal wakeup: {}", val);
      if val == 0 {
        r = Ok(());
        break;
      }
      if val < 0 {
        r = Err(val);
        break;
      }
    }

    // only mark ourselves as consumed *after* we've finished waiting.
    self.mark_consumed();

    r
  }
  /// The signal can be set to negative numbers to indicate
  /// an error. Err(..) will be returned in this case.
  /// If `active` is true, this thread will spin-wait, else
  /// this thread could block.
  ///
  /// This function waits for an acquire memory fence before
  /// returning.
  fn wait_for_zero(&self, spin: bool) -> Result<(), Value> {
    let r = unsafe { self.wait_for_zero_relaxed(spin) };
    fence(Ordering::Acquire);
    r
  }
}

impl HostConsumable for HostSignal { }
impl HostConsumable for GlobalSignal { }
impl<'a, T> HostConsumable for &'a T
  where T: HostConsumable + ?Sized,
{ }
impl<T> HostConsumable for Rc<T>
  where T: HostConsumable + ?Sized,
{ }
impl<T> HostConsumable for Arc<T>
  where T: HostConsumable + ?Sized,
{ }

/// A type which owns data which cannot be safely used until the
/// associated signals is complete.
pub trait DepSignal {
  type Resource;

  unsafe fn peek_resource<F, R>(&self, f: F) -> R
    where F: FnOnce(&Self::Resource) -> R;
  unsafe fn peek_mut_resource(&mut self) -> &mut Self::Resource;
  unsafe fn unwrap_resource(self) -> Self::Resource;

  /// Note: panics if the signal goes < 0
  /// TODO create try version
  fn wait_for_resource(self, spin: bool) -> Self::Resource
    where Self: HostConsumable + Sized,
  {
    self.wait_for_zero(spin)
      .expect("unexpected negative signal value");

    unsafe { self.unwrap_resource() }
  }

  fn map<F, R>(self, f: F) -> MapDepSignal<Self, F>
    where Self: HostConsumable + Sized,
          F: FnOnce(Self::Resource) -> R,
  {
    MapDepSignal(self, f)
  }
}

impl DepSignal for DeviceSignal {
  type Resource = ();

  unsafe fn peek_resource<F, R>(&self, f: F) -> R
    where F: FnOnce(&Self::Resource) -> R,
  {
    f(&())
  }
  unsafe fn peek_mut_resource(&mut self) -> &mut Self::Resource {
    static mut S: () = ();
    &mut S
  }

  unsafe fn unwrap_resource(self) -> Self::Resource {
    ()
  }
}
impl DepSignal for HostSignal {
  type Resource = ();

  unsafe fn peek_resource<F, R>(&self, f: F) -> R
    where F: FnOnce(&Self::Resource) -> R,
  {
    f(&())
  }
  unsafe fn peek_mut_resource(&mut self) -> &mut Self::Resource {
    static mut S: () = ();
    &mut S
  }

  unsafe fn unwrap_resource(self) -> Self::Resource {
    ()
  }
}
impl DepSignal for GlobalSignal {
  type Resource = ();

  unsafe fn peek_resource<F, R>(&self, f: F) -> R
    where F: FnOnce(&Self::Resource) -> R,
  {
    f(&())
  }
  unsafe fn peek_mut_resource(&mut self) -> &mut Self::Resource {
    static mut S: () = ();
    &mut S
  }

  unsafe fn unwrap_resource(self) -> Self::Resource {
    ()
  }
}

pub struct MapDepSignal<D, F>(D, F);
impl<D, F> SignalHandle for MapDepSignal<D, F>
  where D: SignalHandle,
{
  fn signal_ref(&self) -> &SignalRef { self.0.signal_ref() }
  unsafe fn mark_consumed(&self) { self.0.mark_consumed() }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    self.0.as_host_consumable()
  }
}
impl<D, F> HostConsumable for MapDepSignal<D, F>
  where D: HostConsumable,
{ }
impl<D, F, R> DepSignal for MapDepSignal<D, F>
  where D: DepSignal,
        F: FnOnce(D::Resource) -> R,
{
  type Resource = R;
  unsafe fn peek_resource<F2, R2>(&self, _f: F2) -> R2
    where F2: FnOnce(&Self::Resource) -> R2,
  {
    unimplemented!();
  }
  unsafe fn peek_mut_resource(&mut self) -> &mut Self::Resource { unimplemented!() }
  unsafe fn unwrap_resource(self) -> R {
    let dep = self.0.unwrap_resource();
    (self.1)(dep)
  }
}
