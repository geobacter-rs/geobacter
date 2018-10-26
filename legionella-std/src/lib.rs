#![feature(core_intrinsics, intrinsics)]

extern crate hsa_core;
extern crate hsa_rt;
extern crate ndarray as nd;
extern crate core;

pub mod workitem;

extern "rust-intrinsic" {
  fn __legionella_dispatch_ptr() -> *const u8;
}

pub struct DispatchPacket(pub(crate) &'static hsa_rt::ffi::hsa_kernel_dispatch_packet_t);

pub fn dispatch_packet() -> DispatchPacket {
  DispatchPacket(unsafe {
    let ptr = __legionella_dispatch_ptr();
    let ptr = ptr as *const hsa_rt::ffi::hsa_kernel_dispatch_packet_t;
    ptr.as_ref()
      .expect("no dispatch packet!")
  })
}

impl DispatchPacket {
  // pub fn host_queue(&self) -> &'static
}