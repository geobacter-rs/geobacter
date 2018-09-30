#![feature(rustc_private)]
#![feature(unboxed_closures)]
#![feature(conservative_impl_trait)]
#![feature(core_intrinsics)]
#![feature(plugin)]
#![feature(compiler_builtins_lib)]

extern crate hsa_core;
extern crate hsa_rt;
#[macro_use] extern crate rustc;
extern crate rustc_metadata;
extern crate rustc_data_structures;
extern crate rustc_codegen_utils;
extern crate rustc_driver;
extern crate rustc_mir;
extern crate rustc_incremental;
extern crate syntax;
extern crate syntax_pos;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate indexed_vec as indexvec;
extern crate tempdir;
extern crate flate2;
extern crate compiler_builtins;
extern crate goblin;
#[macro_use]
extern crate log;
extern crate serde_json;

use std::fmt::Debug;
use std::hash::Hash;

pub mod context;
pub mod module;
pub mod codegen;
pub mod hsa;
pub mod accelerators;
pub mod passes;
pub mod error;
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
  /// A type with all arch specific data present, for use as a key
  /// in the object cache.
  type CacheKey: Hash;
  fn id(&self) -> AcceleratorId;

  fn target_triple(&self) -> String;
  fn target_arch(&self) -> String;
  fn target_cpu(&self) -> String;
  // no ptr info: it's all 64 bits.
  fn target_datalayout(&self) -> String;

  fn allow_indirect_function_calls(&self) -> bool;


}
