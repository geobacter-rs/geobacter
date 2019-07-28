#![feature(rustc_private, platform_intrinsics,
           rustc_diagnostic_macros)]
#![feature(core_intrinsics, std_internals)]

#[macro_use]
extern crate rustc;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_metadata;
extern crate rustc_mir;
extern crate rustc_codegen_utils;
extern crate rustc_data_structures;
extern crate rustc_target;
extern crate syntax;
extern crate syntax_pos;

extern crate core;

#[macro_use]
extern crate log;
extern crate num_traits;
extern crate vulkano as vko;

extern crate hsa_core;
extern crate legionella_vk_core as lcore;
extern crate rustc_intrinsics;
extern crate legionella_intrinsics_common as common;

// Note: don't try to depend on `legionella_std`.

use std::str::{FromStr, };

pub use self::intrinsics::*;

pub mod intrinsics;
pub mod attrs;

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum LegionellaLangItemTypes {
  Uniform,
  UniformArray,
  Buffer,
  BufferArray,
}
impl FromStr for LegionellaLangItemTypes {
  type Err = &'static str;
  fn from_str(v: &str) -> Result<Self, &'static str> {
    match v {
      "Uniform" => Ok(LegionellaLangItemTypes::Uniform),
      "UniformArray" => Ok(LegionellaLangItemTypes::UniformArray),
      "Buffer" => Ok(LegionellaLangItemTypes::Buffer),
      "BufferArray" => Ok(LegionellaLangItemTypes::BufferArray),
      _ => Err("unknown Legionella lang item type"),
    }
  }
}
