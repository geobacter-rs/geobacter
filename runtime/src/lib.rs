#![feature(rustc_private)]
#![feature(unboxed_closures)]

extern crate ir;
extern crate hsa_core;
extern crate serde_json;
extern crate hsa_rt;
extern crate rustc_llvm as llvm;
extern crate indexvec;
#[macro_use]
extern crate serde_derive;

use std::fmt::Debug;

pub mod context;
pub mod module;
pub mod trans;
pub mod hsa;

pub type AcceleratorId = u64;

pub trait Accelerator: Debug {
  fn set_id(&self, id: AcceleratorId);
}
