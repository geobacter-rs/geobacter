#![feature(rustc_private)]
#![feature(unboxed_closures)]
#![feature(conservative_impl_trait)]
#![feature(core_intrinsics)]
#![feature(plugin)]
#![feature(compiler_builtins_lib)]

#![plugin(hsa_rustc_plugin)]

extern crate hsa_core;
extern crate serde_json;
extern crate hsa_rt;
extern crate rustc;
extern crate rustc_llvm as llvm;
extern crate rustc_metadata;
extern crate rustc_data_structures;
extern crate rustc_back;
extern crate rustc_trans_utils;
extern crate rustc_driver;
extern crate rustc_mir;
extern crate rustc_incremental;
extern crate syntax;
extern crate syntax_pos;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate indexvec;
extern crate tempdir;
extern crate flate2;
extern crate compiler_builtins;

use std::fmt::Debug;

pub mod context;
pub mod module;
pub mod trans;
pub mod hsa;
pub mod accelerators;
pub mod passes;
mod metadata;
mod util;
mod platform;

newtype_idx!(#[derive(Deserialize, Serialize)] pub struct AcceleratorId => "accelerator");

pub const INVALID_ACCELERATOR_ID: AcceleratorId = AcceleratorId(usize::max_value());
impl Default for AcceleratorId {
  fn default() -> Self {
    INVALID_ACCELERATOR_ID
  }
}
impl AcceleratorId {
  pub fn check_invalid(self) -> Option<AcceleratorId> {
    if self == INVALID_ACCELERATOR_ID {
      None
    } else {
      Some(self)
    }
  }
}

pub trait Accelerator: Debug + Send + Sync {
  fn id_opt(&self) -> Option<AcceleratorId>;
  fn id(&self) -> AcceleratorId {
    self.id_opt()
      .expect("accelerator not added to context")
  }
  fn set_id(&self, id: AcceleratorId);

  fn llvm_target(&self) -> String;
  fn target_arch(&self) -> String;
  fn target_cpu(&self) -> String;
  // no ptr info: it's all 64 bits.
}
