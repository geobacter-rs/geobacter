#![feature(plugin)]

#![plugin(hsa_rustc_plugin)]

extern crate hsa_core;
extern crate compiletime;

pub fn main() {
  let str = unsafe {
    compiletime::json_kernel_info_for(&second_function)
  };
  println!("{}", str);
  let str = gfn_arg(second_function);
  println!("{}", str);
}

pub fn second_function() {

}
pub fn gfn_arg<F>(f: F) -> &'static str
  where F: Fn(),
{
  unsafe {
    compiletime::json_kernel_info_for(&f)
  }
}