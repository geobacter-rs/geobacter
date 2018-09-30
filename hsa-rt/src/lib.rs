#![feature(thread_local)]
#![feature(optin_builtin_traits)]

use std::sync::atomic::{AtomicUsize, ATOMIC_USIZE_INIT, Ordering};

extern crate hsa_rt_sys as ffi;
#[macro_use]
extern crate serde_derive;
extern crate serde;

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
/// runtime singleton. For speed, we use a thread local refcount
/// and only upref `GLOBAL_REFCOUNT` once per thread.
static GLOBAL_REFCOUNT: AtomicUsize = ATOMIC_USIZE_INIT;
#[thread_local]
static mut LOCAL_REFCOUNT: usize = 0;

pub struct SendableApiContext;
impl SendableApiContext {
  pub fn try_into(self) -> Result<ApiContext, error::Error> {
    ApiContext::try_upref()
  }
}
impl Into<ApiContext> for SendableApiContext {
  fn into(self) -> ApiContext {
    ApiContext::upref()
  }
}

impl !Sync for SendableApiContext { }
unsafe impl Send for SendableApiContext { }

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ApiContext;
impl ApiContext {
  pub fn try_upref() -> Result<Self, error::Error> {
    // On the first upref, call `hsa_init()`.
    // This ensures the runtime remains up if the context is dropped
    // on all thread except one.
    let refcount = unsafe { &mut LOCAL_REFCOUNT };
    if *refcount == 0 {
      if GLOBAL_REFCOUNT.fetch_add(1, Ordering::AcqRel) == 0 {
        check_err!(ffi::hsa_init())?;
      }
    }

    *refcount += 1;

    Ok(ApiContext)
  }
  pub fn upref() -> Self {
    ApiContext::try_upref()
      .expect("HSA api initialization failed")
  }

  pub fn sendable(&self) -> SendableApiContext {
    SendableApiContext
  }
}
impl Default for ApiContext {
  fn default() -> Self { Self::upref() }
}
impl Drop for ApiContext {
  fn drop(&mut self) {
    let refcount = unsafe { &mut LOCAL_REFCOUNT };
    *refcount -= 1;
    if *refcount == 0 {
      if GLOBAL_REFCOUNT.fetch_sub(1, Ordering::AcqRel) == 1 {
        unsafe {
          ffi::hsa_shut_down();
        }
        // ignore result.
      }
    }
  }
}
impl Clone for ApiContext {
  fn clone(&self) -> Self {
    // always create a new context
    ApiContext::upref()
  }
}
impl !Sync for ApiContext { }
impl !Send for ApiContext { }
