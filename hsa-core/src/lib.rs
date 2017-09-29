#![feature(custom_attribute)]
#![feature(associated_consts)]
#![feature(plugin)]
#![feature(use_extern_macros)]

//#![plugin(hsa_rustc_plugin)]

#[macro_use]
extern crate serde_derive;
extern crate serde;

#[macro_use] extern crate hsa_core_gen;

mod intrinsics;
pub mod marker;

pub mod unit;
