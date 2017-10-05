#![feature(plugin)]

#![plugin(hsa_rustc_plugin)]

extern crate hsa_core;
extern crate compiletime;

pub fn main() {
  let str1 = compiletime::json_kernel_info_for(&second_function);
  println!("{}", str1);
  let str2 = gfn_arg(second_function);
  assert_eq!(str1, str2);

  let str1 = compiletime::json_kernel_info_for(&generic_function::<u64>);
  println!("str2 {}", str1);

  let str1 = compiletime::json_kernel_info_for(&nested_call_function);
  println!("str2 {}", str1);
}

pub fn second_function() {

}
pub fn generic_function<A>(arg1: A) -> usize {
  0
}
pub fn nested_call_function() {
  second_function();
}
pub fn gfn_arg<F>(f: F) -> &'static str
  where F: Fn(),
{
  compiletime::json_kernel_info_for(&f)
}
