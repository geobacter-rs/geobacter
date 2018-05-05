
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

#[derive(Debug)]
pub struct ApiContext;
impl ApiContext {
  pub fn new() -> Self {
    ApiContext::default()
  }
}
impl Default for ApiContext {
  fn default() -> Self {
    check_err!(ffi::hsa_init())
      .expect("api initialization");
    ApiContext
  }
}
impl Drop for ApiContext {
  fn drop(&mut self) {
    unsafe {
      ffi::hsa_shut_down();
    }
    // ignore result.
  }
}
impl Clone for ApiContext {
  fn clone(&self) -> Self {
    ApiContext::default()
  }
}
