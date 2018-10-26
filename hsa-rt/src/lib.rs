#![feature(thread_local)]
#![feature(optin_builtin_traits)]
#![feature(core_intrinsics)]

use std::sync::atomic::{AtomicUsize, ATOMIC_USIZE_INIT, Ordering};

pub extern crate hsa_rt_sys as ffi;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate ndarray as nd;
#[macro_use]
extern crate log;

macro_rules! check_err {
  ($call:expr) => {
    {
      $crate::error::Error::from_status(unsafe {
        $call
      })
    }
  };
  ($call:expr => $result:expr) => {
    {
      $crate::error::Error::from_status(unsafe {
        $call
      })
        .map(move |_| { $result })
    }
  }
}

pub mod error;
pub mod agent;
pub mod code_object;
pub mod executable;
pub mod mem;
pub mod queue;
pub mod signal;
pub mod ext;

/// The AMD HSA impl uses a lock before actually ref counting the
/// runtime singleton. For speed, we maintain a separate ref counter.
static GLOBAL_REFCOUNT: AtomicUsize = ATOMIC_USIZE_INIT;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ApiContext;
impl ApiContext {
  pub fn try_upref() -> Result<Self, error::Error> {
    if GLOBAL_REFCOUNT.fetch_add(1, Ordering::AcqRel) == 0 {
      check_err!(ffi::hsa_init())?;
    }

    Ok(ApiContext)
  }
  pub fn upref() -> Self {
    ApiContext::try_upref()
      .expect("HSA api initialization failed")
  }
}
impl Default for ApiContext {
  fn default() -> Self { Self::upref() }
}
impl Drop for ApiContext {
  fn drop(&mut self) {
    if GLOBAL_REFCOUNT.fetch_sub(1, Ordering::AcqRel) == 1 {
      unsafe {
        ffi::hsa_shut_down();
      }
      // ignore result.
    }
  }
}
impl Clone for ApiContext {
  fn clone(&self) -> Self {
    GLOBAL_REFCOUNT.fetch_add(1, Ordering::AcqRel);
    ApiContext
  }
}
unsafe impl Sync for ApiContext { }
unsafe impl Send for ApiContext { }

pub trait ContextRef {
  fn context(&self) -> &ApiContext;
}
