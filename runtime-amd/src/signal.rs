
use std::ops::*;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{fence, Ordering, };

use crate::module::{Deps, CallError, };

use hsa_rt::error::Error as HsaError;
use hsa_rt::signal::{Signal, SignalRef, ConditionOrdering, WaitState, };

use grt_core::AcceleratorId;

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
    // quick check so we might be able to avoid a fence:
    if self.signal_ref().load_relaxed() == 0 {
      // XXX we might need a fence anyway!!!
      return Ok(());
    }
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

/// A borrow which will decrement the associated signal on drop. Use this to start
/// transfers/kernels when the host finishes a mutable borrow.
pub struct SignaledBorrow<T, S>(S, T)
  where T: ?Sized,
        S: SignalHandle;
impl<T, S> SignaledBorrow<T, S>
  where S: SignalHandle,
{
  pub fn new(value: T, signal: S) -> Self {
    SignaledBorrow(signal, value)
  }
}
unsafe impl<T, S> Deps for SignaledBorrow<T, S>
  where T: Deps + ?Sized,
        S: DeviceConsumable,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    // XXX should iter_deps be called on the inner value too??
    f(&self.0)
  }
}
impl<T, S> Deref for SignaledBorrow<T, S>
  where T: ?Sized,
        S: SignalHandle,
{
  type Target = T;
  fn deref(&self) -> &Self::Target {
    &self.1
  }
}
impl<T, S> DerefMut for SignaledBorrow<T, S>
  where T: ?Sized,
        S: SignalHandle,
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.1
  }
}
impl<T, S, I> Index<I> for SignaledBorrow<T, S>
  where T: Index<I> + ?Sized,
        S: SignalHandle,
{
  type Output = T::Output;
  fn index(&self, idx: I) -> &Self::Output {
    Index::index(&**self, idx)
  }
}

impl<T, S, I> IndexMut<I> for SignaledBorrow<T, S>
  where T: IndexMut<I> + ?Sized,
        S: SignalHandle,
{
  fn index_mut(&mut self, idx: I) -> &mut Self::Output {
    IndexMut::index_mut(&mut **self, idx)
  }
}
impl<T, S> Drop for SignaledBorrow<T, S>
  where T: ?Sized,
        S: SignalHandle,
{
  fn drop(&mut self) {
    self.0.signal_ref().subtract_screlease(1);
  }
}
/// An object which will force the host to wait on the signal when deref-ed.
/// Use this to wait for transfers/kernels to finish before reading
#[derive(Clone, Copy)]
pub struct SignaledDeref<T, S>(S, T)
  where T: ?Sized,
        S: HostConsumable;

impl<T, S> SignaledDeref<T, S>
  where T: ?Sized,
        S: HostConsumable,
{
  pub fn new(value: T, signal: S) -> Self
    where T: Sized,
  {
    SignaledDeref(signal, value)
  }

  pub unsafe fn unchecked_ref(&self) -> &T {
    &self.1
  }
  pub unsafe fn unchecked_mut(&mut self) -> &mut T {
    &mut self.1
  }

  pub fn try_unwrap(self, spin: bool) -> Result<(T, S), Value>
    where T: Sized,
  {
    self.0.wait_for_zero(spin)?;

    let Self(s, t) = self;
    Ok((t, s))
  }
  pub fn unwrap(self, spin: bool) -> (T, S)
    where T: Sized,
  {
    self.try_unwrap(spin)
      .expect("non-zero signal result")
  }

  pub fn try_as_ref(&self, spin: bool) -> Result<&T, Value> {
    self.0.wait_for_zero(spin)?;

    Ok(&self.1)
  }
  pub fn try_as_mut(&mut self, spin: bool) -> Result<&mut T, Value> {
    self.0.wait_for_zero(spin)?;

    Ok(&mut self.1)
  }
}

impl<T, S> Deref for SignaledDeref<T, S>
  where T: ?Sized,
        S: HostConsumable,
{
  type Target = T;
  fn deref(&self) -> &Self::Target {
    self.try_as_ref(false)
      .expect("non-zero signal result")
  }
}
impl<T, S> DerefMut for SignaledDeref<T, S>
  where T: ?Sized,
        S: HostConsumable,
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.try_as_mut(false)
      .expect("non-zero signal result")
  }
}
impl<T, S, I> Index<I> for SignaledDeref<T, S>
  where T: Index<I> + ?Sized,
        S: HostConsumable,
{
  type Output = T::Output;
  fn index(&self, idx: I) -> &Self::Output {
    Index::index(&**self, idx)
  }
}

impl<T, S, I> IndexMut<I> for SignaledDeref<T, S>
  where T: IndexMut<I> + ?Sized,
        S: HostConsumable,
{
  fn index_mut(&mut self, idx: I) -> &mut Self::Output {
    IndexMut::index_mut(&mut **self, idx)
  }
}
