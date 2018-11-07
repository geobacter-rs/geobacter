#![feature(custom_attribute)]
#![feature(intrinsics)]
#![feature(unboxed_closures)]
#![feature(repr_simd)]
#![feature(associated_type_defaults)]
#![feature(platform_intrinsics)]

#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate num_traits;

#[macro_use] extern crate hsa_core_gen;

mod intrinsics;
pub mod marker;
pub mod traits;
pub mod unit;
pub mod kernel;
