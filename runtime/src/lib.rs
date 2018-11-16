#![feature(rustc_private)]
#![feature(unboxed_closures)]
#![feature(core_intrinsics)]
#![feature(custom_attribute)]
#![feature(std_internals)]
#![feature(compiler_builtins_lib)]

#![crate_type = "dylib"]

#![recursion_limit="256"]

extern crate hsa_core;
extern crate hsa_rt;
#[macro_use] extern crate rustc;
extern crate rustc_metadata;
extern crate rustc_data_structures;
extern crate rustc_codegen_utils;
extern crate rustc_driver;
extern crate rustc_mir;
extern crate rustc_incremental;
extern crate rustc_target;
extern crate rustc_typeck;
extern crate syntax;
extern crate syntax_pos;
extern crate compiler_builtins;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate indexed_vec as indexvec;
extern crate tempdir;
extern crate flate2;
extern crate goblin;
#[macro_use]
extern crate log;
extern crate serde_json;
extern crate git2;
extern crate num_cpus;
extern crate seahash;
extern crate core;
extern crate dirs;
extern crate fs2;
extern crate legionella_intrinsics;
extern crate ndarray as nd;

use std::error::Error;
use std::fmt::Debug;
use std::hash::{Hash, Hasher, };
use std::ops::{Range, };
use std::sync::{Arc, };

use indexvec::Idx;

use hsa_rt::agent::{Agent, Isa, };
use hsa_rt::mem::region::Region;

use rustc::session::config::host_triple;
use rustc_target::spec::{Target, TargetTriple, };

use accelerators::DeviceLibsBuild;
use codegen::worker::CodegenComms;

pub mod context;
pub mod module;
pub mod codegen;
pub mod accelerators;
pub mod passes;
pub mod error;
mod metadata;
mod platform;
mod serde_utils;
mod utils;

newtype_index!(AcceleratorId);

pub trait Accelerator: Debug + Send + Sync {
  // A type with all arch specific data present, for use as a key
  // in the object cache.
  fn id(&self) -> AcceleratorId;

  /// Returns `None` if this accel is a host.
  fn host_accel(&self) -> Option<Arc<Accelerator>> { None }

  fn agent(&self) -> &Agent;
  fn isa(&self) -> Option<&Isa> { None }

  fn kernargs_region(&self) -> &Region;

  fn accel_target_desc(&self) -> Result<Arc<AcceleratorTargetDesc>, Box<Error>>;
  fn device_libs_builder(&self) -> Option<Box<DeviceLibsBuilder>> { None }

  fn set_codegen(&self, comms: CodegenComms) -> Option<CodegenComms>;
  fn get_codegen(&self) -> Option<CodegenComms>;
}

pub trait DeviceLibsBuilder: Debug + Send + Sync {
  fn run_build(&self, device_libs: &mut DeviceLibsBuild) -> Result<(), Box<Error>>;
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AcceleratorTargetDesc {
  pub allow_indirect_function_calls: bool,
  #[serde(flatten, with = "serde_utils::Target")]
  pub target: Target,
}
impl AcceleratorTargetDesc {
  pub fn allow_indirect_function_calls(&self) -> bool {
    self.allow_indirect_function_calls
  }

  pub fn rustc_target_options(&self, target: &mut Target) {
    *target = self.target.clone();
  }

  pub fn get_stable_hash(&self) -> u64 {
    let mut hasher = seahash::SeaHasher::new();
    self.hash(&mut hasher);
    hasher.finish()
  }
}
impl Eq for AcceleratorTargetDesc { }
impl Default for AcceleratorTargetDesc {
  fn default() -> Self {
    // always base the info on the host.
    let host = host_triple();
    let triple = TargetTriple::from_triple(host);
    let target = Target::search(&triple)
      .expect("no host target?");
    AcceleratorTargetDesc {
      allow_indirect_function_calls: true,
      target,
    }
  }
}
impl Hash for AcceleratorTargetDesc {
  fn hash<H>(&self, hasher: &mut H)
    where H: Hasher,
  {
    self.allow_indirect_function_calls.hash(hasher);

    macro_rules! impl_for {
      ({
        $(pub $field_name:ident: $field_ty:ty,)*
      }, $field:expr) => (
        $($field.$field_name.hash(hasher);)*
      );
    }

    // skipped:
    impl_for!({
      pub llvm_target: String,
      pub target_endian: String,
      pub target_pointer_width: String,
      pub target_c_int_width: String,
      pub target_os: String,
      pub target_env: String,
      pub target_vendor: String,
      pub arch: String,
      pub data_layout: String,
      pub linker_flavor: LinkerFlavor,
    }, self.target);

    // skipped:
    // pub is_builtin: bool,
    impl_for!({
      pub linker: Option<String>,
      pub lld_flavor: LldFlavor,
      pub pre_link_args: LinkArgs,
      pub pre_link_args_crt: LinkArgs,
      pub pre_link_objects_exe: Vec<String>,
      pub pre_link_objects_exe_crt: Vec<String>,
      pub pre_link_objects_dll: Vec<String>,
      pub late_link_args: LinkArgs,
      pub post_link_objects: Vec<String>,
      pub post_link_objects_crt: Vec<String>,
      pub post_link_args: LinkArgs,
      pub link_env: Vec<(String, String)>,
      pub asm_args: Vec<String>,
      pub cpu: String,
      pub features: String,
      pub dynamic_linking: bool,
      pub only_cdylib: bool,
      pub executables: bool,
      pub relocation_model: String,
      pub code_model: Option<String>,
      pub tls_model: String,
      pub disable_redzone: bool,
      pub eliminate_frame_pointer: bool,
      pub function_sections: bool,
      pub dll_prefix: String,
      pub dll_suffix: String,
      pub exe_suffix: String,
      pub staticlib_prefix: String,
      pub staticlib_suffix: String,
      pub target_family: Option<String>,
      pub abi_return_struct_as_int: bool,
      pub is_like_osx: bool,
      pub is_like_solaris: bool,
      pub is_like_windows: bool,
      pub is_like_msvc: bool,
      pub is_like_android: bool,
      pub is_like_emscripten: bool,
      pub linker_is_gnu: bool,
      pub allows_weak_linkage: bool,
      pub has_rpath: bool,
      pub no_default_libraries: bool,
      pub position_independent_executables: bool,
      pub relro_level: RelroLevel,
      pub archive_format: String,
      pub allow_asm: bool,
      pub custom_unwind_resume: bool,
      pub exe_allocation_crate: Option<String>,
      pub has_elf_tls: bool,
      pub obj_is_bitcode: bool,
      pub no_integrated_as: bool,
      pub min_atomic_width: Option<u64>,
      pub max_atomic_width: Option<u64>,
      pub atomic_cas: bool,
      pub panic_strategy: PanicStrategy,
      pub abi_blacklist: Vec<Abi>,
      pub crt_static_allows_dylibs: bool,
      pub crt_static_default: bool,
      pub crt_static_respected: bool,
      pub stack_probes: bool,
      pub min_global_align: Option<u64>,
      pub default_codegen_units: Option<u64>,
      pub trap_unreachable: bool,
      pub requires_lto: bool,
      pub singlethread: bool,
      pub no_builtins: bool,
      pub i128_lowering: bool,
      pub codegen_backend: String,
      pub default_hidden_visibility: bool,
      pub embed_bitcode: bool,
      pub emit_debug_gdb_scripts: bool,
      pub requires_uwtable: bool,
    }, self.target.options);
  }
}
