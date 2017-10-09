#![feature(plugin)]

#![plugin(hsa_rustc_plugin)]

extern crate hsa_core;
extern crate compiletime;

pub fn main() {
  let (id, str1) = compiletime::json_kernel_info_for(&second_function);
  println!("({}, {})", id, str1);
  let (id2, str2) = gfn_arg(second_function);
  assert_eq!(str1, str2);
  assert_eq!(id, id2);

  let (id, str1) = compiletime::json_kernel_info_for(&generic_function::<u64>);
  println!("({}, {})", id, str1);

  let (id, str1) = compiletime::json_kernel_info_for(&nested_call_function);
  println!("({}, {})", id, str1);
}

pub fn second_function() {

}
pub fn generic_function<A>(arg1: A) -> usize {
  0
}
pub fn nested_call_function() {
  second_function();
}
pub fn gfn_arg<F>(f: F) -> (u64, &'static str)
  where F: Fn(),
{
  compiletime::json_kernel_info_for(&f)
}
