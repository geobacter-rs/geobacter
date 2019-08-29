
//! TODO specialization of libcore/libstd traits (like Deref on
//! `&'a NewType<T>`) is broken because they don't mark things
//! as `default`, as required for specialization.

#![feature(intrinsics)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(const_fn, const_transmute)]
#![feature(slice_from_raw_parts)]
#![feature(slice_index_methods)]
#![feature(coerce_unsized)]
#![feature(unsize)]
#![feature(specialization)]
#![feature(ptr_internals)]

extern crate geobacter_shared_defs as shared_defs;

#[macro_use]
pub mod macros;

pub mod intrinsics;
pub mod kernel;
pub mod platform;
pub mod ptr;
pub mod ref_;
pub mod slice;
