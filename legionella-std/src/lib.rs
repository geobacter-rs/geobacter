#![feature(core_intrinsics, intrinsics)]
#![feature(allocator_api)]
#![feature(slice_index_methods)]
#![feature(crate_visibility_modifier)]
#![feature(linkage)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(const_slice_len)]
#![feature(unsize)]

extern crate hsa_core;
extern crate core;
extern crate vulkano as vk;
extern crate legionella_core as lcore;
extern crate vulkano;

pub mod shader;
pub mod kernel;

pub mod vk_help;

mod spirv_help;

pub mod mem;

pub use vk_help::*;
pub use lcore::ss::ExeModel;

/// Returns the current execution model. This changes depending how the
/// function is compiled.
pub fn exe_model() -> ExeModel {
  extern "rust-intrinsic" {
    fn __legionella_exe_model() -> u32;
  }

  unsafe {
    ::std::mem::transmute(__legionella_exe_model())
  }
}

/// Returns a constant which will be false when a function is recompiled for
/// any accelerator processor. LLVM *should* (if not it's a bug) see the constant
/// and remove any branches etc.
#[inline(always)]
pub fn is_host() -> bool {
  exe_model() == ExeModel::Host
}
/// Are we compiling for SPIRV? Returns a constant which should be eliminated by
/// LLVM optimizations.
#[inline(always)]
pub fn is_spirv() -> bool {
  !is_host()
}
/// This currently always returns false.
#[inline(always)]
pub fn is_nvptx() -> bool {
  false
}
/// This currently always returns false.
#[inline(always)]
pub fn is_amdgcn() -> bool { false }
