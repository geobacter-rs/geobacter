#![feature(core_intrinsics, intrinsics)]
#![feature(allocator_api)]
#![feature(slice_index_methods)]

extern crate geobacter_core as gcore;
extern crate hsa_rt;

use hsa_rt::ffi::hsa_kernel_dispatch_packet_t;

pub use gcore::{platform, ptr, slice, ref_, param, };
pub use gcore::{host_assert, host_assert_eq, host_assert_ne,
                host_debug_assert, host_debug_assert_eq,
                host_debug_assert_ne, host_unimplemented,
                host_unreachable, };
pub mod dpp;
pub mod sync;
pub mod workitem;

#[derive(Clone, Copy)]
pub struct DispatchPacket(pub(crate) &'static hsa_kernel_dispatch_packet_t);

pub fn dispatch_packet() -> DispatchPacket {
  extern "rust-intrinsic" {
    fn __geobacter_dispatch_ptr() -> *const u8;
  }
  DispatchPacket(unsafe {
    let ptr = __geobacter_dispatch_ptr();
    let ptr: *const hsa_kernel_dispatch_packet_t = ptr as *const _;
    match ptr.as_ref() {
      Some(r) => r,
      // Don't pull in panic code (which will in turn pull in
      // format!, fmt::Debug, and virtual dispatch).
      None => ::std::hint::unreachable_unchecked(),
    }
  })
}

impl DispatchPacket {
  pub fn get() -> Self { dispatch_packet() }
}
