//! This crate is used in the non-bootstrap drivers: the host driver, and the
//! runtime driver. This provides a driver agnostic interface for implementing
//! custom Rust intrinsics and translating our `KernelId`s into Rust's `DefId`s.

#![feature(rustc_private)]
#![feature(core_intrinsics, std_internals)]
#![feature(box_patterns)]
#![feature(link_llvm_intrinsics)]
#![feature(intrinsics)]

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
extern crate rustc_target;
extern crate rustc_span;
extern crate serialize;

#[macro_use]
extern crate log;
extern crate num_traits;
extern crate seahash;

extern crate geobacter_core;
extern crate geobacter_rustc_help as rustc_help;

pub mod attrs;
pub mod collector;
pub mod hash;
pub mod intrinsics;
pub mod platform;
pub mod stubbing;

pub use rustc_help::*;
pub use driver_data::*;
pub use intrinsics::*;
