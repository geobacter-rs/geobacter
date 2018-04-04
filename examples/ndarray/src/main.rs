#![feature(plugin)]

#![plugin(hsa_rustc_plugin)]

extern crate ndarray as nd;
extern crate hsa_core;
extern crate runtime;
extern crate env_logger;

use runtime::context::Context;
use runtime::accelerators::AmdRadeon;

fn vector_fill(mut into: nd::ArrayViewMut2<f64>,
               value: f64) {
  into.fill(value);
}

fn fill(mut into: nd::Array2<f64>) -> nd::Array2<f64> {
  vector_fill(into.view_mut(), 5.0f64);
  into
}
fn alloc_fill() -> nd::Array2<f64> {
  let v = nd::Array2::zeros((1000, 1000));
  fill(v)
}

pub fn main() {
  env_logger::init();
  let ctxt = Context::new()
    .expect("create context");

  let accel = AmdRadeon::new();
  ctxt.add_accelerator(accel);

  let info = hsa_core::kernel_info::kernel_info_for(&alloc_fill);
  ctxt._compile_function(info.id);
}
