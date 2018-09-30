#![feature(custom_attribute)]
#![feature(use_extern_macros)]
#![feature(intrinsics)]
#![feature(unboxed_closures)]
#![feature(extern_prelude)]

#[macro_use]
extern crate serde_derive;
extern crate serde;

#[macro_use] extern crate hsa_core_gen;

mod intrinsics;
pub mod marker;
pub mod traits;
pub mod unit;
pub mod kernel;
