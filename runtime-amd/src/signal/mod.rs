
//! Note: sometime Soon(TM) this will undergo a large refactor, in order to remove
//! many foot guns relating to direct use of the associated SignalRefs.

use std::convert::TryInto;
use std::geobacter::platform::platform;
use std::ops::*;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{fence, Ordering, };
use std::time::Duration;

use crate::HsaAmdGpuAccel;
use crate::module::{Deps, CallError, };

use hsa_rt::error::Error as HsaError;
use hsa_rt::signal::{Signal, SignalRef, ConditionOrdering, WaitState};

use grt_core::AcceleratorId;

pub use hsa_rt::signal::{Value, SignalLoad, SignalStore, SignalSilentStore, SignalExchange,
                         SignalCas, SignalBinops, SignalHostWait};

pub mod completion;
pub mod deps;
pub mod gpu;

pub trait SignalHandle {
  fn signal_ref(&self) -> SignalRef;

  fn as_host_consumable(&self) -> Option<&dyn HostConsumable>;
}
impl<'a, T> SignalHandle for &'a T
  where T: SignalHandle + ?Sized,
{
  #[inline(always)]
  fn signal_ref(&self) -> SignalRef { (**self).signal_ref() }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    (**self).as_host_consumable()
  }
}
impl<'a, T> SignalHandle for &'a mut T
  where T: SignalHandle + ?Sized,
{
  #[inline(always)]
  fn signal_ref(&self) -> SignalRef { (**self).signal_ref() }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    (**self).as_host_consumable()
  }
}
impl<T> SignalHandle for Rc<T>
  where T: SignalHandle + ?Sized,
{
  #[inline(always)]
  fn signal_ref(&self) -> SignalRef { (**self).signal_ref() }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    (**self).as_host_consumable()
  }
}
impl<T> SignalHandle for Arc<T>
  where T: SignalHandle + ?Sized,
{
  #[inline(always)]
  fn signal_ref(&self) -> SignalRef { (**self).signal_ref() }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    (**self).as_host_consumable()
  }
}
/// Interface for safely resetting a signal to some initial state.
/// This is separate because reset doesn't use interior mutability, and
/// thus taking a mutable reference to a immutable reference would allow
/// one to reset the signal, possibly while in use, breaking signal dependency
/// chains.
/// You probably shouldn't implement this yourself.
pub trait ResettableSignal {
  /// Must only return Some(..) when self is internally unique.
  fn resettable_get_mut(&mut self, f: impl FnOnce(SignalRef)) -> bool;
}
impl<T> ResettableSignal for T
  where T: SignalHandle,
{
  #[inline(always)]
  default fn resettable_get_mut(&mut self, f: impl FnOnce(SignalRef)) -> bool {
    f(self.signal_ref());
    true
  }
}
impl<T> ResettableSignal for Rc<T>
  where T: SignalHandle,
{
  #[inline(always)]
  fn resettable_get_mut(&mut self, f: impl FnOnce(SignalRef)) -> bool {
    Rc::get_mut(self)
      .map(|this| f(this.signal_ref()) )
      .is_some()
  }
}
impl<T> ResettableSignal for Arc<T>
  where T: SignalHandle,
{
  #[inline(always)]
  fn resettable_get_mut(&mut self, f: impl FnOnce(SignalRef)) -> bool {
    Arc::get_mut(self)
      .map(|this| f(this.signal_ref()) )
      .is_some()
  }
}

#[derive(Debug, Eq, PartialEq)]
pub struct HostSignal(pub(crate) Signal);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSignalRef<'a>(pub(crate) SignalRef<'a>);

impl HostSignal {
  #[inline(always)]
  pub fn as_ref(&self) -> HostSignalRef {
    HostSignalRef(self.0.as_ref())
  }
}
impl Deref for HostSignal {
  type Target = Signal;
  #[inline(always)]
  fn deref(&self) -> &Signal { &self.0 }
}
impl SignalHandle for HostSignal {
  #[inline(always)]
  fn signal_ref(&self) -> SignalRef { self.0.as_ref() }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    Some(self)
  }
}
impl<'a> Deref for HostSignalRef<'a> {
  type Target = SignalRef<'a>;
  #[inline(always)]
  fn deref(&self) -> &SignalRef<'a> { &self.0 }
}
impl<'a> SignalHandle for HostSignalRef<'a> {
  #[inline(always)]
  fn signal_ref(&self) -> SignalRef { self.0 }
  #[inline(always)]
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> { Some(self) }
}

#[derive(Debug, Eq, PartialEq)]
pub struct DeviceSignal(pub(crate) Signal, pub(crate) AcceleratorId);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceSignalRef<'a>(pub(crate) SignalRef<'a>, pub(crate) AcceleratorId);

impl DeviceSignal {
  #[inline(always)]
  pub fn as_ref(&self) -> DeviceSignalRef {
    DeviceSignalRef(self.0.as_ref(), self.1)
  }
}

impl Deref for DeviceSignal {
  type Target = Signal;
  #[inline(always)]
  fn deref(&self) -> &Signal { &self.0 }
}
impl SignalHandle for DeviceSignal {
  #[inline(always)]
  fn signal_ref(&self) -> SignalRef { self.0.as_ref() }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> { None }
}
impl<'a> Deref for DeviceSignalRef<'a> {
  type Target = SignalRef<'a>;
  #[inline(always)]
  fn deref(&self) -> &SignalRef<'a> { &self.0 }
}
impl<'a> SignalHandle for DeviceSignalRef<'a> {
  #[inline(always)]
  fn signal_ref(&self) -> SignalRef { self.0 }
  #[inline(always)]
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> { None }
}

/// A signal handle which any device on this system can consume (wait on).
#[derive(Debug, Eq, PartialEq)]
pub struct GlobalSignal(pub(crate) Signal);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlobalSignalRef<'a>(pub(crate) SignalRef<'a>);

impl GlobalSignal {
  pub fn new(initial: Value) -> Result<Self, HsaError> {
    Signal::new(initial, &[])
      .map(GlobalSignal)
  }
  #[inline(always)]
  pub fn as_ref(&self) -> GlobalSignalRef {
    GlobalSignalRef(self.0.as_ref())
  }
}
impl Deref for GlobalSignal {
  type Target = Signal;
  #[inline(always)]
  fn deref(&self) -> &Signal { &self.0 }
}
impl SignalHandle for GlobalSignal {
  #[inline(always)]
  fn signal_ref(&self) -> SignalRef { self.0.as_ref() }
  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    Some(self)
  }
}
impl<'a> Deref for GlobalSignalRef<'a> {
  type Target = SignalRef<'a>;
  #[inline(always)]
  fn deref(&self) -> &SignalRef<'a> { &self.0 }
}
impl<'a> SignalHandle for GlobalSignalRef<'a> {
  #[inline(always)]
  fn signal_ref(&self) -> SignalRef { self.0 }
  #[inline(always)]
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
impl<'a> DeviceConsumable for DeviceSignalRef<'a> {
  #[inline(always)]
  fn usable_on_device(&self, id: AcceleratorId) -> bool {
    self.1 == id
  }
}
impl<'a> DeviceConsumable for GlobalSignalRef<'a> {
  #[inline(always)]
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

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WakeCondition {
  Equal(Value),
  NotEqual(Value),
  Less(Value),
  GreaterEqual(Value),
}
impl WakeCondition {
  #[inline(always)]
  fn into_pair(self) -> (ConditionOrdering, Value) {
    match self {
      WakeCondition::Equal(v) => (ConditionOrdering::Equal, v),
      WakeCondition::NotEqual(v) => (ConditionOrdering::NotEqual, v),
      WakeCondition::Less(v) => (ConditionOrdering::Less, v),
      WakeCondition::GreaterEqual(v) => (ConditionOrdering::GreaterEqual, v),
    }
  }
  #[inline(always)]
  pub fn satisfied(&self, v: Value) -> bool {
    match self {
      WakeCondition::Equal(expected) if v == *expected => true,
      WakeCondition::NotEqual(expected) if v != *expected => true,
      WakeCondition::Less(expected) if v < *expected => true,
      WakeCondition::GreaterEqual(expected) if v >= *expected => true,
      _ => false,
    }
  }
}

#[inline(always)]
fn wait_for_condition_relaxed(s: SignalRef, spin: bool, cond: WakeCondition,
                              timeout: Option<Duration>) -> Result<Value, Value>
{
  let timeout = timeout.and_then(|timeout| {
    timeout.as_nanos()
      .try_into()
      .ok()
  });

  let wait = if spin {
    WaitState::Active
  } else {
    WaitState::Blocked
  };

  let (condition, compare) = cond.into_pair();

  let val = s.wait_relaxed(condition, compare, timeout,
                           wait);
  log::debug!("completion signal wakeup: {}", val);
  if cond.satisfied(val) {
    Ok(val)
  } else {
    Err(val)
  }
}

/// A signal which may be waited on by the host.
pub trait HostConsumable: SignalHandle {
  /// Returns Err() only when timeout is Some().
  unsafe fn wait_for_condition_relaxed(&self, spin: bool, cond: WakeCondition,
                                       timeout: Option<Duration>)
                                       -> Result<Value, Value>
  {
    wait_for_condition_relaxed(self.signal_ref(), spin, cond, timeout)
  }
  #[inline]
  fn wait_for_condition(&self, spin: bool, cond: WakeCondition,
                        timeout: Option<Duration>)
                        -> Result<Value, Value>
  {
    let r = unsafe { self.wait_for_condition_relaxed(spin, cond, timeout) };
    fence(Ordering::Acquire);
    r
  }

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
    let cond = WakeCondition::Less(1);
    loop {
      let r = wait_for_condition_relaxed(self.signal_ref(), spin, cond, None);
      let got = match r {
        Ok(got) => got,
        Err(_) => continue,
      };
      return if got < 0 {
        Err(got)
      } else {
        Ok(())
      };
    }
  }
  unsafe fn wait_for_zero_timeout_relaxed(&self, spin: bool,
                                          timeout: Duration)
    -> Result<(), Value>
  {
    let cond = WakeCondition::Less(1);
    loop {
      let got = wait_for_condition_relaxed(self.signal_ref(), spin, cond,
                                         Some(timeout))?;
      return if got < 0 {
        Err(got)
      } else {
        Ok(())
      };
    }
  }

  /// The signal can be set to negative numbers to indicate
  /// an error. Err(..) will be returned in this case.
  /// If `active` is true, this thread will spin-wait, else
  /// this thread could block.
  ///
  /// This function waits for an acquire memory fence before
  /// returning.
  #[inline]
  fn wait_for_zero(&self, spin: bool) -> Result<(), Value> {
    let r = unsafe { self.wait_for_zero_relaxed(spin) };
    fence(Ordering::Acquire);
    r
  }
  #[inline]
  fn wait_for_zero_timeout(&self, spin: bool, timeout: Duration)
    -> Result<(), Value>
  {
    let r = unsafe { self.wait_for_zero_timeout_relaxed(spin, timeout) };
    fence(Ordering::Acquire);
    r
  }
}

impl HostConsumable for HostSignal { }
impl<'a> HostConsumable for HostSignalRef<'a> { }
impl HostConsumable for GlobalSignal { }
impl<'a> HostConsumable for GlobalSignalRef<'a> { }
impl<'a, T> HostConsumable for &'a T
  where T: HostConsumable + ?Sized,
{ }
impl<T> HostConsumable for Rc<T>
  where T: HostConsumable + ?Sized,
{ }
impl<T> HostConsumable for Arc<T>
  where T: HostConsumable + ?Sized,
{ }

#[inline(always)]
fn reset_impl<T>(this: &mut T, device: &Arc<HsaAmdGpuAccel>, initial: Value)
  -> Result<(), HsaError>
  where T: SignalFactory,
{
  let r = this.resettable_get_mut(|signal| {
    signal.silent_store_relaxed(initial);
  });
  if r {
    return Ok(());
  }

  *this = T::new(device, initial)?;
  Ok(())
}

pub trait SignalFactory: ResettableSignal + SignalHandle {
  fn new(device: &Arc<HsaAmdGpuAccel>, initial: Value) -> Result<Self, HsaError>
    where Self: Sized;

  fn reset(&mut self, device: &Arc<HsaAmdGpuAccel>, initial: Value)
    -> Result<(), HsaError>;
}
impl SignalFactory for GlobalSignal {
  fn new(_: &Arc<HsaAmdGpuAccel>, initial: Value) -> Result<Self, HsaError> {
    GlobalSignal::new(initial)
  }

  #[inline(always)]
  fn reset(&mut self, device: &Arc<HsaAmdGpuAccel>, initial: Value)
    -> Result<(), HsaError>
  {
    reset_impl(self, device, initial)
  }
}
impl SignalFactory for DeviceSignal {
  fn new(device: &Arc<HsaAmdGpuAccel>, initial: Value) -> Result<Self, HsaError> {
    device.new_device_signal(initial)
  }

  #[inline(always)]
  fn reset(&mut self, device: &Arc<HsaAmdGpuAccel>, initial: Value)
    -> Result<(), HsaError>
  {
    reset_impl(self, device, initial)
  }
}
impl SignalFactory for HostSignal {
  fn new(device: &Arc<HsaAmdGpuAccel>, initial: Value) -> Result<Self, HsaError> {
    device.new_host_signal(initial)
  }

  #[inline(always)]
  fn reset(&mut self, device: &Arc<HsaAmdGpuAccel>, initial: Value)
    -> Result<(), HsaError>
  {
    reset_impl(self, device, initial)
  }
}
impl<S> SignalFactory for Rc<S>
  where S: SignalFactory,
{
  fn new(device: &Arc<HsaAmdGpuAccel>, initial: Value) -> Result<Self, HsaError> {
    let s = S::new(device, initial)?;
    Ok(Rc::new(s))
  }

  #[inline(always)]
  fn reset(&mut self, device: &Arc<HsaAmdGpuAccel>, initial: Value)
    -> Result<(), HsaError>
  {
    reset_impl(self, device, initial)
  }
}
impl<S> SignalFactory for Arc<S>
  where S: SignalFactory,
{
  fn new(device: &Arc<HsaAmdGpuAccel>, initial: Value) -> Result<Self, HsaError> {
    let s = S::new(device, initial)?;
    Ok(Arc::new(s))
  }

  #[inline(always)]
  fn reset(&mut self, device: &Arc<HsaAmdGpuAccel>, initial: Value)
    -> Result<(), HsaError>
  {
    reset_impl(self, device, initial)
  }
}

/// An object which will force the host to wait on the signal when deref-ed.
/// Use this to wait for transfers/kernels to finish before reading.
/// Also usable in your kernel's argument structure, which will cause
/// the GPU command processor to wait on the signal before launching any wave
/// (assuming `Deps` is implemented correctly). This structure won't wait on
/// the signal in that case.
#[derive(Clone, Copy, Debug)]
pub struct SignaledDeref<T, S>(S, T)
  where T: ?Sized,
        S: SignalHandle;

impl<T, S> SignaledDeref<T, S>
  where S: SignalHandle,
{
  pub fn new(value: T, signal: S) -> Self
    where T: Sized,
  {
    SignaledDeref(signal, value)
  }

  pub unsafe fn unchecked_unwrap(self) -> (T, S) {
    let Self(s, t) = self;
    (t, s)
  }
  pub fn try_unwrap(self, spin: bool) -> Result<(T, S), Value>
    where T: Sized,
  {
    if platform().is_host() {
      self.0.as_host_consumable()
        .expect("signal is not host consumable")
        .wait_for_zero(spin)?;
    }

    let Self(s, t) = self;
    Ok((t, s))
  }
  pub fn unwrap(self, spin: bool) -> (T, S)
    where T: Sized,
  {
    self.try_unwrap(spin)
      .expect("non-zero signal result")
  }
}
impl<T, S> SignaledDeref<T, S>
  where T: ?Sized,
        S: SignalHandle,
{
  pub unsafe fn unchecked_ref(&self) -> &T {
    &self.1
  }
  pub unsafe fn unchecked_mut(&mut self) -> &mut T {
    &mut self.1
  }
  pub fn try_get_ref(&self, spin: bool) -> Result<&T, Value> {
    if platform().is_host() /* TODO && T::DEREF_WAIT */ {
      self.0.as_host_consumable()
        .expect("signal is not host consumable")
        .wait_for_zero(spin)?;
    }

    Ok(&self.1)
  }
  pub fn try_get_mut(&mut self, spin: bool) -> Result<&mut T, Value> {
    if platform().is_host() {
      self.0.as_host_consumable()
        .expect("signal is not host consumable")
        .wait_for_zero(spin)?;
    }

    Ok(&mut self.1)
  }
}
impl<'a, T, S> SignaledDeref<T, &'a S>
  where S: SignalHandle,
{
  pub fn clone_signal(self) -> SignaledDeref<T, S>
    where S: Clone,
  {
    let (v, s) = unsafe { self.unchecked_unwrap() };
    SignaledDeref::new(v, s.clone())
  }
}
impl<T, S> Deref for SignaledDeref<T, S>
  where T: ?Sized,
        S: SignalHandle,
{
  type Target = T;
  fn deref(&self) -> &Self::Target {
    if !platform().is_host() {
      unsafe { self.unchecked_ref() }
    } else {
      self.try_get_ref(false)
        .expect("non-zero signal result")
    }
  }
}
impl<T, S> DerefMut for SignaledDeref<T, S>
  where T: ?Sized,
        S: SignalHandle,
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    if !platform().is_host() {
      unsafe { self.unchecked_mut() }
    } else {
      self.try_get_mut(false)
        .expect("non-zero signal result")
    }
  }
}
impl<T, S, I> Index<I> for SignaledDeref<T, S>
  where T: Index<I> + ?Sized,
        S: SignalHandle,
{
  type Output = <T as Index<I>>::Output;
  fn index(&self, idx: I) -> &Self::Output {
    Index::index(&**self, idx)
  }
}

impl<T, S, I> IndexMut<I> for SignaledDeref<T, S>
  where T: IndexMut<I> + ?Sized,
        S: SignalHandle,
{
  fn index_mut(&mut self, idx: I) -> &mut Self::Output {
    IndexMut::index_mut(&mut **self, idx)
  }
}
unsafe impl<T, S> Deps for SignaledDeref<T, S>
  where T: Deps + ?Sized,
        S: DeviceConsumable,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    self.1.iter_deps(f)?;
    f(&self.0)
  }
}
