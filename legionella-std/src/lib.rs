#![feature(core_intrinsics, intrinsics)]
#![feature(allocator_api)]
#![feature(slice_index_methods)]

extern crate hsa_core;
extern crate hsa_rt;
extern crate ndarray as nd;
extern crate core;
#[macro_use]
extern crate log;

macro_rules! check_err {
  ($call:expr) => {
    {
      ::hsa_rt::error::Error::from_status(unsafe {
        $call
      })
    }
  };
  ($call:expr => $result:expr) => {
    {
      ::hsa_rt::error::Error::from_status(unsafe {
        $call
      })
        .map(move |_| { $result })
    }
  }
}

pub mod mem;
pub mod workitem;

extern "rust-intrinsic" {
  fn __legionella_dispatch_ptr() -> *const u8;
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct DispatchPacket(pub(crate) &'static hsa_rt::ffi::hsa_kernel_dispatch_packet_t);

pub fn dispatch_packet() -> DispatchPacket {
  DispatchPacket(unsafe {
    let ptr = __legionella_dispatch_ptr();
    let ptr = ptr as *const hsa_rt::ffi::hsa_kernel_dispatch_packet_t;
    ptr.as_ref()
      .expect("no dispatch packet!")
  })
}

impl DispatchPacket { }

/// Returns a constant which will be false when a function is recompiled for
/// any accelerator processor. LLVM *should* (if not it's a bug) see the constant
/// and remove any branches etc.
#[inline(always)]
pub fn is_host() -> bool {
  extern "rust-intrinsic" {
    fn __legionella_is_host() -> bool;
  }

  unsafe { __legionella_is_host() }
}
