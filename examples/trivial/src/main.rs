#![feature(plugin)]

#![plugin(hsa_rustc_plugin)]

extern crate runtime;
extern crate hsa_core;

use runtime::context::Context;
use runtime::accelerators::AmdRadeon;
use hsa_core::kernel_id::KernelInfo;

pub struct Test {
  value: u64,
}

pub fn main() {
  let ctxt = Context::new()
    .expect("create context");

  let accel = AmdRadeon::new();
  ctxt.add_accelerator(accel);

  let info = hsa_core::kernel_id::kernel_info_for(&second_function);
  ctxt._compile_function(info.id);
}

pub fn second_function() {

}
pub fn generic_function<A>(arg1: A) -> usize {
  0
}
pub fn nested_call_function() {
  second_function();
}
pub fn gfn_arg<'l, F>(f: &'l F) -> KernelInfo<'l>
  where F: Fn() + 'static,
{
  hsa_core::kernel_id::kernel_info_for(f)
}
pub fn struct_arg(test: Test) -> u64 { test.value }
