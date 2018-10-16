
use std::error::Error;
use std::ops::Deref;

use ffi;
use agent::Agent;

pub use std::sync::atomic::Ordering;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Signal(pub(crate) ffi::hsa_signal_t);
pub type Value = ffi::hsa_signal_value_t;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ConditionOrdering {
  Equal,
  NotEqual,
  Less,
  GreaterEqual,
}
impl Into<ffi::hsa_signal_condition_t> for ConditionOrdering {
  fn into(self) -> ffi::hsa_signal_condition_t {
    match self {
      ConditionOrdering::Equal =>
        ffi::hsa_signal_condition_t_HSA_SIGNAL_CONDITION_EQ,
      ConditionOrdering::NotEqual =>
        ffi::hsa_signal_condition_t_HSA_SIGNAL_CONDITION_NE,
      ConditionOrdering::Less =>
        ffi::hsa_signal_condition_t_HSA_SIGNAL_CONDITION_LT,
      ConditionOrdering::GreaterEqual =>
        ffi::hsa_signal_condition_t_HSA_SIGNAL_CONDITION_GTE,
    }
  }
}
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum WaitState {
  Blocked,
  Active,
}
impl Into<ffi::hsa_wait_state_t> for WaitState {
  fn into(self) -> ffi::hsa_wait_state_t {
    match self {
      WaitState::Blocked => ffi::hsa_wait_state_t_HSA_WAIT_STATE_BLOCKED,
      WaitState::Active => ffi::hsa_wait_state_t_HSA_WAIT_STATE_ACTIVE,
    }
  }
}

impl Signal {
  pub fn new(initial: Value,
             consumers: &[Agent])
    -> Result<Self, Box<Error>>
  {
    let len = consumers.len();
    let consumers_ptr = consumers.as_ptr() as *const ffi::hsa_agent_t;
    let mut out: ffi::hsa_signal_t = unsafe { ::std::mem::uninitialized() };
    let out = check_err!(ffi::hsa_signal_create(initial,
                                                len as _,
                                                consumers_ptr,
                                                &mut out as *mut _) => out)?;
    Ok(Signal(out))
  }
}
impl Deref for Signal {
  type Target = SignalRef;
  fn deref(&self) -> &SignalRef {
    unsafe {
      ::std::mem::transmute(self)
    }
  }
}
impl AsRef<Signal> for Signal {
  fn as_ref(&self) -> &Signal {
    self
  }
}

#[repr(transparent)]
pub struct SignalRef(pub(crate) ffi::hsa_signal_t);
impl SignalRef {
  pub fn load_scacquire(&self) -> Value {
    unsafe {
      ffi::hsa_signal_load_scacquire(self.0)
    }
  }
  pub fn load_relaxed(&self) -> Value {
    unsafe {
      ffi::hsa_signal_load_relaxed(self.0)
    }
  }
  pub fn load(&self, order: Ordering) -> Value {
    match order {
      Ordering::Relaxed => self.load_relaxed(),
      _ => self.load_scacquire(),
    }
  }
  pub fn store_relaxed(&self, val: Value) {
    unsafe {
      ffi::hsa_signal_store_relaxed(self.0, val);
    }
  }
  pub fn store_screlease(&self, val: Value) {
    unsafe {
      ffi::hsa_signal_store_screlease(self.0, val);
    }
  }
  pub fn silent_store_relaxed(&self, val: Value) {
    unsafe {
      ffi::hsa_signal_silent_store_relaxed(self.0, val);
    }
  }
  pub fn silent_store_screlease(&self, val: Value) {
    unsafe {
      ffi::hsa_signal_silent_store_screlease(self.0, val);
    }
  }
  pub fn store(&self, val: Value, order: Ordering, silent: bool) {
    match (order, silent) {
      (Ordering::Relaxed, false) => self.store_relaxed(val),
      (_, false) => self.store_screlease(val),
      (Ordering::Relaxed, true) => self.silent_store_relaxed(val),
      (_, true) => self.silent_store_screlease(val),
    }
  }
}

macro_rules! impl_exchange {
  ($f:ident, $ffi:ident) => (
    impl SignalRef {
      pub fn $f(&self, val: Value) -> Value {
        unsafe {
          ffi::$ffi(self.0, val)
        }
      }
    }
  )
}
impl_exchange!(exchange_scacq_screl, hsa_signal_exchange_scacq_screl);
impl_exchange!(exchange_scacquire, hsa_signal_exchange_scacquire);
impl_exchange!(exchange_relaxed, hsa_signal_exchange_relaxed);
impl_exchange!(exchange_screlease, hsa_signal_exchange_screlease);

macro_rules! impl_cas {
  ($f:ident, $ffi:ident) => (
    impl SignalRef {
      pub fn $f(&self, expected: Value, new: Value) -> Value {
        unsafe {
          ffi::$ffi(self.0, expected, new)
        }
      }
    }
  )
}
impl_cas!(cas_scacq_screl, hsa_signal_cas_scacq_screl);
impl_cas!(cas_scacquire, hsa_signal_cas_scacquire);
impl_cas!(cas_relaxed, hsa_signal_cas_relaxed);
impl_cas!(cas_screlease, hsa_signal_cas_screlease);

macro_rules! impl_binop {
  ($f:ident, $ffi:ident) => (
    impl SignalRef {
      pub fn $f(&self, val: Value) {
        unsafe {
          ffi::$ffi(self.0, val)
        }
      }
    }
  )
}
impl_binop!(add_scacq_screl, hsa_signal_add_scacq_screl);
impl_binop!(add_scacquire, hsa_signal_add_scacquire);
impl_binop!(add_relaxed, hsa_signal_add_relaxed);
impl_binop!(add_screlease, hsa_signal_add_screlease);

impl_binop!(subtract_scacq_screl, hsa_signal_subtract_scacq_screl);
impl_binop!(subtract_scacquire, hsa_signal_subtract_scacquire);
impl_binop!(subtract_relaxed, hsa_signal_subtract_relaxed);
impl_binop!(subtract_screlease, hsa_signal_subtract_screlease);

impl_binop!(and_scacq_screl, hsa_signal_and_scacq_screl);
impl_binop!(and_scacquire, hsa_signal_and_scacquire);
impl_binop!(and_relaxed, hsa_signal_and_relaxed);
impl_binop!(and_screlease, hsa_signal_and_screlease);

impl_binop!(or_scacq_screl, hsa_signal_or_scacq_screl);
impl_binop!(or_scacquire, hsa_signal_or_scacquire);
impl_binop!(or_relaxed, hsa_signal_or_relaxed);
impl_binop!(or_screlease, hsa_signal_or_screlease);

impl_binop!(xor_scacq_screl, hsa_signal_xor_scacq_screl);
impl_binop!(xor_scacquire, hsa_signal_xor_scacquire);
impl_binop!(xor_relaxed, hsa_signal_xor_relaxed);
impl_binop!(xor_screlease, hsa_signal_xor_screlease);

impl SignalRef {
  pub fn wait_scacquire(&self, condition: ConditionOrdering,
                        compare: Value, timeout_hint: Option<u64>,
                        wait_state_hint: WaitState) -> Value {
    let timeout_hint = timeout_hint.unwrap_or(u64::max_value());
    unsafe {
      ffi::hsa_signal_wait_scacquire(self.0, condition.into(),
                                     compare, timeout_hint,
                                     wait_state_hint.into())
    }
  }
  pub fn wait_relaxed(&self, condition: ConditionOrdering,
                      compare: Value, timeout_hint: Option<u64>,
                      wait_state_hint: WaitState) -> Value {
    let timeout_hint = timeout_hint.unwrap_or(u64::max_value());
    unsafe {
      ffi::hsa_signal_wait_relaxed(self.0, condition.into(),
                                   compare, timeout_hint,
                                   wait_state_hint.into())
    }
  }
}

impl Drop for Signal {
  fn drop(&mut self) {
    unsafe {
      ffi::hsa_signal_destroy(self.0)
    };
  }
}

unsafe impl Send for Signal { }
unsafe impl Sync for Signal { }

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SignalGroup(ffi::hsa_signal_group_t);

impl SignalGroup {
  pub fn new(signals: &[Signal], consumers: &[Agent])
    -> Result<SignalGroup, Box<Error>>
  {
    let signals_len = signals.len();
    let consumers_len = consumers.len();

    let signals_ptr = signals.as_ptr() as *const ffi::hsa_signal_t;
    let consumers_ptr = consumers.as_ptr() as *const ffi::hsa_agent_t;

    let mut out: ffi::hsa_signal_group_t = unsafe { ::std::mem::uninitialized() };
    let out = check_err!(ffi::hsa_signal_group_create(signals_len as _, signals_ptr,
                                                      consumers_len as _, consumers_ptr,
                                                      &mut out as *mut _) => out)?;
    Ok(SignalGroup(out))
  }


}

impl Drop for SignalGroup {
  fn drop(&mut self) {
    unsafe {
      ffi::hsa_signal_group_destroy(self.0)
    };
  }
}