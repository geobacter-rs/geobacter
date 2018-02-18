#![feature(plugin)]

#![plugin(hsa_rustc_plugin)]

extern crate ndarray as nd;
extern crate hsa_core;

use hsa_core::kernel_info::json_kernel_info_for;

fn vector_fill<T: Copy>(mut into: nd::ArrayViewMut1<T>,
                        value: T) {
  into.fill(value);
}

pub fn main() {
  let kinfo = json_kernel_info_for(&vector_fill::<f64>);
  println!("{}", kinfo.info);
}