#![feature(plugin)]

#![plugin(hsa_rustc_plugin)]

extern crate hsa_core;

use hsa_core::kernel_info::{json_kernel_info_for, KernelInfo,};

pub struct Test {
  value: u64,
}

pub fn main() {
  let info = json_kernel_info_for(&second_function);
  //println!("({}, {})", id, str1);
  let info2 = gfn_arg(second_function);
  assert_eq!(info2, info);

  let info = json_kernel_info_for(&generic_function::<u64>);
  //println!("({}, {})", id, str1);

  let info = json_kernel_info_for(&nested_call_function);
  //println!("({}, {})", id, str1);

  let capture = 0;
  let f = || { capture }; f();
  let info = json_kernel_info_for(&f);
  println!("({}, {})", info.id, info.info);

  let info = json_kernel_info_for(&struct_arg);
  println!("({}, {})", info.id, info.info);
}

pub fn second_function() {

}
pub fn generic_function<A>(arg1: A) -> usize {
  0
}
pub fn nested_call_function() {
  second_function();
}
pub fn gfn_arg<F>(f: F) -> KernelInfo<'static, &'static str>
  where F: Fn() + 'static,
{
  json_kernel_info_for(&f)
}
pub fn struct_arg(test: Test) -> u64 { test.value }
