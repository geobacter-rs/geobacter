#![feature(rustc_private, platform_intrinsics)]
#![feature(core_intrinsics, std_internals)]

extern crate rustc_ast;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_index;
extern crate rustc_metadata;
#[macro_use]
extern crate rustc_middle;
extern crate rustc_mir;
extern crate rustc_span;
extern crate rustc_target;

#[macro_use]
extern crate log;
extern crate num_traits;
extern crate vulkano as vko;

extern crate geobacter_core as gcore;
extern crate geobacter_vk_core as gvk_core;
extern crate geobacter_rustc_help as grustc_help;
extern crate geobacter_intrinsics_common as common;

// Note: don't try to depend on `geobacter_std`.

use std::str::{FromStr, };

pub use self::intrinsics::*;

pub mod intrinsics;
pub mod attrs;

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum GeobacterLangItemTypes {
  Uniform,
  UniformArray,
  Buffer,
  BufferArray,
}
impl FromStr for GeobacterLangItemTypes {
  type Err = &'static str;
  fn from_str(v: &str) -> Result<Self, &'static str> {
    match v {
      "Uniform" => Ok(GeobacterLangItemTypes::Uniform),
      "UniformArray" => Ok(GeobacterLangItemTypes::UniformArray),
      "Buffer" => Ok(GeobacterLangItemTypes::Buffer),
      "BufferArray" => Ok(GeobacterLangItemTypes::BufferArray),
      _ => Err("unknown Geobacter lang item type"),
    }
  }
}
