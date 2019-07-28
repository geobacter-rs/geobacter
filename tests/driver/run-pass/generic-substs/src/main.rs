
//! Check that a single generic function instantiated with
//! different types don't share the same kernel instance.

extern crate hsa_core;

use hsa_core::kernel::*;

fn test<T>(_arg: T) { }

pub fn main() {
  let lhs = KernelInstance::get(&test::<u64>);
  let rhs = KernelInstance::get(&test::<u32>);
  assert_ne!(lhs, rhs);
}
