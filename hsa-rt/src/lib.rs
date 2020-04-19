#![feature(negative_impls)]
#![feature(core_intrinsics)]
#![feature(unsize)]
#![feature(coerce_unsized)]
#![feature(allocator_api)]
#![feature(alloc_layout_extra)]

// For images:
#![feature(repr_simd)]
#![feature(geobacter)]

use std::cmp::{self, PartialEq, PartialOrd, Ord, };
use std::sync::atomic::{AtomicUsize, Ordering};

pub extern crate hsa_rt_sys as ffi;
#[cfg(feature = "serde")]
extern crate serde;
extern crate log;
#[cfg(feature = "alloc-wg")]
extern crate alloc_wg;
extern crate num_traits;

macro_rules! check_err {
  ($call:expr) => {
    {
      #[allow(unused_unsafe)]
      $crate::error::Error::from_status(unsafe {
        $call
      })
    }
  };
  ($call:expr => $result:expr) => {
    {
      #[allow(unused_unsafe)]
      $crate::error::Error::from_status(unsafe {
        $call
      })
        .map(move |_| { $result })
    }
  }
}

pub mod utils;

pub mod error;
pub mod agent;
pub mod alloc;
pub mod code_object;
pub mod executable;
pub mod mem;
pub mod queue;
pub mod signal;
pub mod ext;

/// The AMD HSA impl uses a lock before actually ref counting the
/// runtime singleton. For speed, we maintain a separate ref counter.
static GLOBAL_REFCOUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Eq, Hash)]
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
  pub fn is_live() -> bool {
    GLOBAL_REFCOUNT.load(Ordering::Relaxed) > 0
  }
  pub fn check_live() -> Result<(), &'static str> {
    if !ApiContext::is_live() {
      Err("all HSA context refs were dropped")
    } else {
      Ok(())
    }
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
impl PartialEq for ApiContext {
  #[inline(always)]
  fn eq(&self, _rhs: &ApiContext) -> bool { true }
}
impl Ord for ApiContext {
  #[inline(always)]
  fn cmp(&self, _rhs: &ApiContext) -> cmp::Ordering {
    cmp::Ordering::Equal
  }
}
impl PartialOrd for ApiContext {
  #[inline(always)]
  fn partial_cmp(&self, rhs: &ApiContext) -> Option<cmp::Ordering> {
    Some(self.cmp(rhs))
  }
}
unsafe impl Sync for ApiContext { }
unsafe impl Send for ApiContext { }

pub trait ContextRef {
  fn context(&self) -> &ApiContext;
}
