
use std::marker::PhantomData;
use std::ptr::null;

use crate::agent::Agent;
use crate::error::Error;
use crate::ffi;
use utils::uninit;

pub use std::sync::atomic::Ordering;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Signal(pub(crate) ffi::hsa_signal_t);
pub type Value = ffi::hsa_signal_value_t;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
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
  pub fn new(initial: Value, consumers: &[Agent])
    -> Result<Self, Error>
  {
    let len = consumers.len();
    let consumers_ptr = if len != 0 {
      consumers.as_ptr() as *const ffi::hsa_agent_t
    } else {
      null()
    };
    let mut out: ffi::hsa_signal_t = unsafe { uninit() };
    let out = check_err!(ffi::hsa_signal_create(initial,
                                                len as _,
                                                consumers_ptr,
                                                &mut out as *mut _) => out)?;
    Ok(Signal(out))
  }
  pub fn new_global(initial: Value) -> Result<Self, Error> {
    Self::new(initial, &[])
  }

  #[inline(always)]
  pub fn as_ref(&self) -> SignalRef {
    SignalRef(self.0, PhantomData)
  }

  #[inline(always)]
  pub fn store_ext(&self, val: Value, order: Ordering, silent: bool) {
    match (order, silent) {
      (Ordering::Relaxed, false) => self.store_relaxed(val),
      (_, false) => self.store_screlease(val),
      (Ordering::Relaxed, true) => self.silent_store_relaxed(val),
      (_, true) => self.silent_store_screlease(val),
    }
  }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct SignalRef<'a>(pub(crate) ffi::hsa_signal_t, pub(crate) PhantomData<&'a Signal>);
impl<'a> SignalRef<'a> {
  #[inline(always)]
  pub fn store_ext(&self, val: Value, order: Ordering, silent: bool) {
    match (order, silent) {
      (Ordering::Relaxed, false) => self.store_relaxed(val),
      (_, false) => self.store_screlease(val),
      (Ordering::Relaxed, true) => self.silent_store_relaxed(val),
      (_, true) => self.silent_store_screlease(val),
    }
  }
}
pub trait SignalHsaHandle {
  fn as_hndl(&self) -> ffi::hsa_signal_t;
}
impl SignalHsaHandle for Signal {
  #[inline(always)]
  fn as_hndl(&self) -> ffi::hsa_signal_t { self.0 }
}
impl<'a> SignalHsaHandle for SignalRef<'a> {
  #[inline(always)]
  fn as_hndl(&self) -> ffi::hsa_signal_t { self.0 }
}

macro_rules! impl_l {
  ($($f:ident, $ffi:ident, $ordering:ident,)*) => (
    pub trait SignalLoad: SignalHsaHandle {
      $(
      fn $f(&self) -> Value;
      )*
      #[inline(always)]
      fn load(&self, ordering: Ordering) -> Value {
        match ordering {
          $(Ordering::$ordering => self.$f(),)*
          _ => self.load_scacquire(),
        }
      }
    }
    impl<'a> SignalLoad for SignalRef<'a> {
      $(
      #[inline(always)]
      fn $f(&self) -> Value {
        unsafe {
          ffi::$ffi(self.0)
        }
      }
      )*
    }
    impl SignalLoad for Signal {
      $(
      #[inline(always)]
      fn $f(&self) -> Value {
        unsafe {
          ffi::$ffi(self.0)
        }
      }
      )*
    }
  )
}
impl_l!(
  load_scacquire, hsa_signal_load_scacquire, Acquire,
  load_relaxed, hsa_signal_load_relaxed, Relaxed,
);

macro_rules! impl_s {
  ($($f:ident, $ffi:ident, $ordering:ident,)*) => (
    pub trait SignalStore: SignalHsaHandle {
      $(
      fn $f(&self, val: Value);
      )*
      #[inline(always)]
      fn store(&self, val: Value, ordering: Ordering) {
        match ordering {
          $(Ordering::$ordering => self.$f(val),)*
          _ => self.store_screlease(val),
        }
      }
    }
    impl<'a> SignalStore for SignalRef<'a> {
      $(
      #[inline(always)]
      fn $f(&self, val: Value) {
        unsafe {
          ffi::$ffi(self.0, val)
        }
      }
      )*
    }
    impl SignalStore for Signal {
      $(
      #[inline(always)]
      fn $f(&self, val: Value) {
        unsafe {
          ffi::$ffi(self.0, val)
        }
      }
      )*
    }
  )
}
impl_s!(
  store_relaxed, hsa_signal_store_relaxed, Relaxed,
  store_screlease, hsa_signal_store_screlease, Release,
);
macro_rules! impl_ss {
  ($($f:ident, $ffi:ident, $ordering:ident,)*) => (
    pub trait SignalSilentStore: SignalHsaHandle {
      $(
      fn $f(&self, val: Value);
      )*
      #[inline(always)]
      fn silent_store(&self, val: Value, ordering: Ordering) {
        match ordering {
          $(Ordering::$ordering => self.$f(val),)*
          _ => self.silent_store_screlease(val),
        }
      }
    }
    impl<'a> SignalSilentStore for SignalRef<'a> {
      $(
      #[inline(always)]
      fn $f(&self, val: Value) {
        unsafe {
          ffi::$ffi(self.0, val)
        }
      }
      )*
    }
    impl SignalSilentStore for Signal {
      $(
      #[inline(always)]
      fn $f(&self, val: Value) {
        unsafe {
          ffi::$ffi(self.0, val)
        }
      }
      )*
    }
  )
}
impl_ss!(
  silent_store_relaxed, hsa_signal_silent_store_relaxed, Relaxed,
  silent_store_screlease, hsa_signal_silent_store_screlease, Release,
);

macro_rules! impl_exchange {
  ($($f:ident, $ffi:ident, $ordering:ident,)*) => (
    pub trait SignalExchange: SignalHsaHandle {
      $(
      fn $f(&self, val: Value) -> Value;
      )*
      #[inline(always)]
      fn exchange(&self, val: Value, ordering: Ordering) -> Value {
        match ordering {
          $(Ordering::$ordering => self.$f(val),)*
          _ => self.exchange_scacq_screl(val),
        }
      }
    }
    impl<'a> SignalExchange for SignalRef<'a> {
      $(
      #[inline(always)]
      fn $f(&self, val: Value) -> Value {
        unsafe {
          ffi::$ffi(self.0, val)
        }
      }
      )*
    }
    impl SignalExchange for Signal {
      $(
      #[inline(always)]
      fn $f(&self, val: Value) -> Value {
        unsafe {
          ffi::$ffi(self.0, val)
        }
      }
      )*
    }
  )
}
impl_exchange!(
  exchange_scacq_screl, hsa_signal_exchange_scacq_screl, AcqRel,
  exchange_scacquire, hsa_signal_exchange_scacquire, Acquire,
  exchange_relaxed, hsa_signal_exchange_relaxed, Relaxed,
  exchange_screlease, hsa_signal_exchange_screlease, Release,
);

macro_rules! impl_cas {
  ($($f:ident, $ffi:ident, $ordering:ident,)*) => (
    pub trait SignalCas: SignalHsaHandle {
      $(
      fn $f(&self, expected: Value, new: Value) -> Value;
      )*
      #[inline(always)]
      fn compare_and_swap(&self, expected: Value, new: Value, ordering: Ordering) -> Value {
        match ordering {
          $(Ordering::$ordering => self.$f(expected, new),)*
          _ => self.cas_scacq_screl(expected, new),
        }
      }
    }
    impl<'a> SignalCas for SignalRef<'a> {
      $(
      #[inline(always)]
      fn $f(&self, expected: Value, new: Value) -> Value {
        unsafe {
          ffi::$ffi(self.0, expected, new)
        }
      }
      )*
    }
    impl SignalCas for Signal {
      $(
      #[inline(always)]
      fn $f(&self, expected: Value, new: Value) -> Value {
        unsafe {
          ffi::$ffi(self.0, expected, new)
        }
      }
      )*
    }
  )
}
impl_cas!(
  cas_scacq_screl, hsa_signal_cas_scacq_screl, AcqRel,
  cas_scacquire, hsa_signal_cas_scacquire, Acquire,
  cas_relaxed, hsa_signal_cas_relaxed, Relaxed,
  cas_screlease, hsa_signal_cas_screlease, Release,
);

macro_rules! impl_binop {
  ($($f:ident, $ffi:ident,)*) => (
    pub trait SignalBinops: SignalHsaHandle {
      $(
      fn $f(&self, val: Value);
      )*
    }
    impl<'a> SignalBinops for SignalRef<'a> {
      $(
      #[inline(always)]
      fn $f(&self, val: Value) {
        unsafe {
          ffi::$ffi(self.0, val)
        }
      }
      )*
    }
    impl SignalBinops for Signal {
      $(
      #[inline(always)]
      fn $f(&self, val: Value) {
        unsafe {
          ffi::$ffi(self.0, val)
        }
      }
      )*
    }
  )
}
impl_binop!(
  add_scacq_screl, hsa_signal_add_scacq_screl,
  add_scacquire, hsa_signal_add_scacquire,
  add_relaxed, hsa_signal_add_relaxed,
  add_screlease, hsa_signal_add_screlease,

  subtract_scacq_screl, hsa_signal_subtract_scacq_screl,
  subtract_scacquire, hsa_signal_subtract_scacquire,
  subtract_relaxed, hsa_signal_subtract_relaxed,
  subtract_screlease, hsa_signal_subtract_screlease,

  and_scacq_screl, hsa_signal_and_scacq_screl,
  and_scacquire, hsa_signal_and_scacquire,
  and_relaxed, hsa_signal_and_relaxed,
  and_screlease, hsa_signal_and_screlease,

  or_scacq_screl, hsa_signal_or_scacq_screl,
  or_scacquire, hsa_signal_or_scacquire,
  or_relaxed, hsa_signal_or_relaxed,
  or_screlease, hsa_signal_or_screlease,

  xor_scacq_screl, hsa_signal_xor_scacq_screl,
  xor_scacquire, hsa_signal_xor_scacquire,
  xor_relaxed, hsa_signal_xor_relaxed,
  xor_screlease, hsa_signal_xor_screlease,
);

pub trait SignalHostWait: SignalHsaHandle {
  /// Like `wait_relaxed`, but executes an acquire fence after the
  /// provided condition is satisfied.
  #[inline(always)]
  fn wait_scacquire(&self, condition: ConditionOrdering,
                    compare: Value, timeout_hint: Option<u64>,
                    wait_state_hint: WaitState) -> Value {
    let timeout_hint = timeout_hint.unwrap_or(u64::max_value());
    unsafe {
      ffi::hsa_signal_wait_scacquire(self.as_hndl(), condition.into(),
                                     compare, timeout_hint,
                                     wait_state_hint.into())
    }
  }
  #[inline(always)]
  fn wait_relaxed(&self, condition: ConditionOrdering,
                  compare: Value, timeout_hint: Option<u64>,
                  wait_state_hint: WaitState) -> Value {
    let timeout_hint = timeout_hint.unwrap_or(u64::max_value());
    unsafe {
      ffi::hsa_signal_wait_relaxed(self.as_hndl(), condition.into(),
                                   compare, timeout_hint,
                                   wait_state_hint.into())
    }
  }

  #[inline(always)]
  fn wait(&self, condition: ConditionOrdering,
          compare: Value, timeout_hint: Option<u64>,
          wait_state_hint: WaitState, ordering: Ordering) -> Value
  {
    match ordering {
      Ordering::Relaxed => self.wait_relaxed(condition, compare,
                                             timeout_hint, wait_state_hint),
      _ => self.wait_scacquire(condition, compare, timeout_hint, wait_state_hint),
    }
  }
}
impl<'a> SignalHostWait for SignalRef<'a> { }
impl SignalHostWait for Signal { }

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
    -> Result<SignalGroup, Error>
  {
    let signals_len = signals.len();
    let consumers_len = consumers.len();

    let signals_ptr = signals.as_ptr() as *const ffi::hsa_signal_t;
    let consumers_ptr = consumers.as_ptr() as *const ffi::hsa_agent_t;

    let mut out: ffi::hsa_signal_group_t = unsafe { uninit() };
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