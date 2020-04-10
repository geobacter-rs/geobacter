//! TODO move this all the way into the Rust toolchain.

#![feature(rustc_private)]
#![feature(unboxed_closures)]
#![feature(core_intrinsics)]
#![feature(std_internals)]
#![feature(arbitrary_self_types)]
#![feature(raw)]
#![feature(geobacter, geobacter_intrinsics)]

#![recursion_limit="256"]

extern crate rustc_ast;
extern crate rustc_metadata;
extern crate rustc_data_structures;
extern crate rustc_codegen_ssa;
extern crate rustc_driver;
extern crate rustc_feature;
extern crate rustc_geobacter;
extern crate rustc_hir;
extern crate rustc_incremental;
extern crate rustc_index;
extern crate rustc_interface;
extern crate rustc_lint;
#[macro_use] extern crate rustc_middle;
extern crate rustc_mir;
extern crate rustc_mir_build;
extern crate rustc_passes;
extern crate rustc_privacy;
extern crate rustc_resolve;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_symbol_mangling;
extern crate rustc_target;
extern crate rustc_ty;
extern crate rustc_typeck;
extern crate rustc_trait_selection;
extern crate rustc_traits;
extern crate serde;
extern crate erased_serde;
extern crate indexed_vec as indexvec;
extern crate flate2;
extern crate goblin;
#[macro_use]
extern crate log;
extern crate seahash;
extern crate owning_ref;
extern crate any_key;

use std::any::Any;
use std::error::Error;
use std::fmt::Debug;
use std::geobacter::platform::Platform;
use std::hash::{Hash, Hasher, };
use std::sync::{Arc, };

use crate::any_key::AnyHash;

use crate::serde::{Serialize, };

use rustc_session::config::host_triple;
use crate::rustc_target::spec::{Target, TargetTriple, abi::Abi, };

use crate::context::{Context, PlatformModuleData, };
use crate::codegen::{PlatformCodegen, CodegenDriver};
use crate::codegen::products::PCodegenResults;

pub mod context;
pub mod codegen;
mod metadata;
mod platform;
mod serde_utils;
mod utils;

indexvec::newtype_index!(AcceleratorId);

/// A common interface to a specific compute device. Note this doesn't
/// include functions for managing memory or kernel instances. Sadly,
/// such features are left to implementation defined interfaces for
/// now.
pub trait Accelerator: Debug + Any + Send + Sync + 'static {
  fn id(&self) -> AcceleratorId;

  /// Get the value `current_platform()` would return if executed
  /// on this accelerator. Since some platforms include device specific
  /// info, this property requires an active device instance.
  fn platform(&self) -> Platform;

  fn accel_target_desc(&self) -> &Arc<AcceleratorTargetDesc>;
  /// Set the reference to the target to the provided object.
  /// This will be given target descs which are identical to
  /// the desc returned by `Self::accel_target_desc`.
  /// Used by the context only during initialization.
  fn set_accel_target_desc(&mut self, desc: Arc<AcceleratorTargetDesc>);

  /// Create the codegen worker, using the platform specific
  /// helper type. This will only be called once per unique
  /// `AcceleratorTargetDesc`.
  /// Used by the context only during initialization.
  fn create_target_codegen(self: &mut Arc<Self>, ctxt: &Context)
    -> Result<Arc<dyn Any + Send + Sync + 'static>, Box<dyn Error + Send + Sync + 'static>>
    where Self: Sized;
  /// Used by the context only during initialization.
  fn set_target_codegen(self: &mut Arc<Self>,
                        codegen_comms: Arc<dyn Any + Send + Sync + 'static>)
    where Self: Sized;

  /// Special downcast helper as trait objects can't be "reunsized" into
  /// another trait object, even when this trait requires
  /// `Self: Any + 'static`.
  fn downcast_ref(this: &dyn Accelerator) -> Option<&Self>
    where Self: Sized,
  {
    use std::any::TypeId;
    use std::mem::transmute;
    use std::raw::TraitObject;

    let this_tyid = Any::type_id(this);
    let self_tyid = TypeId::of::<Self>();
    if this_tyid != self_tyid {
      return None;
    }

    // We have to do this manually.
    let this: TraitObject = unsafe { transmute(this) };
    let this = this.data as *mut Self;
    Some(unsafe { &*this })
  }

  /// Special downcast helper as trait objects can't be "reunsized" into
  /// another trait object, even when this trait requires
  /// `Self: Any + 'static`.
  fn downcast_arc(this: &Arc<dyn Accelerator>) -> Option<Arc<Self>>
    where Self: Sized,
  {
    use std::any::TypeId;
    use std::mem::transmute;
    use std::raw::TraitObject;

    let this_tyid = Any::type_id(&**this);
    let self_tyid = TypeId::of::<Self>();
    if this_tyid != self_tyid {
      return None;
    }

    // We have to do this manually.
    let this = this.clone();
    let this = Arc::into_raw(this);
    let this: TraitObject = unsafe { transmute(this) };
    let this = this.data as *mut Self;
    Some(unsafe { Arc::from_raw(this) })
  }
}

pub trait Device: Accelerator + Sized {
  type Error: From<codegen::error::Error<Self::Error>> + Send;
  type Codegen: PlatformCodegen<Device = Self>;
  type TargetDesc: PlatformTargetDesc;
  type ModuleData: PlatformModuleData;

  fn codegen(&self) -> &Arc<CodegenDriver<Self::Codegen>>;

  /// Load the result of `post_codegen` into whatever platform/API
  /// specific structure is required.
  /// `self` is a reference to the Arc containing this accel,
  /// use if you need to store a reference to this accel in
  /// the platform specific object.
  /// Note: the returned `PlatformModuleData` will be stored in a
  /// per-function *global*; `self` references should probably be weak.
  fn load_kernel(self: &Arc<Self>, results: &PCodegenResults<Self::Codegen>)
    -> Result<Arc<Self::ModuleData>, Self::Error>;
}

/// A hashable structure describing what is best supported by a device.
#[derive(Debug, Serialize)]
pub struct AcceleratorTargetDesc {
  pub allow_indirect_function_calls: bool,
  #[serde(with = "serde_utils::abi")]
  pub kernel_abi: Abi,

  /// This is here to make sure two identical devices which are used from
  /// two different hosts aren't mistakenly shared.
  #[serde(with = "serde_utils::Target")]
  pub host_target: Target,

  #[serde(flatten, with = "serde_utils::Target")]
  pub target: Target,
  pub platform: Box<dyn PlatformTargetDesc>,
}
impl AcceleratorTargetDesc {
  pub fn new<T>(platform: T) -> Self
    where T: PlatformTargetDesc,
  {
    // always base the info on the host.
    let target = Self::host_target();

    AcceleratorTargetDesc {
      allow_indirect_function_calls: true,
      kernel_abi: Abi::C,
      host_target: target.clone(),
      target,
      platform: Box::new(platform),
    }
  }

  pub fn host_target() -> Target {
    let host = host_triple();
    let triple = TargetTriple::from_triple(host);
    let target = Target::search(&triple)
      .expect("no host target?");
    target
  }

  pub fn allow_indirect_function_calls(&self) -> bool {
    self.allow_indirect_function_calls
  }

  pub fn rustc_target_options(&self, target: &mut Target) {
    *target = self.target.clone();
  }

  pub fn is_host(&self) -> bool {
    // XXX this comparison is rather expensive
    self.target == self.host_target
  }
  pub fn is_amdgpu(&self) -> bool {
    self.target.arch == "amdgpu"
  }
  pub fn is_spirv(&self) -> bool {
    self.target.llvm_target == "spir64-unknown-unknown"
  }
  pub fn is_cuda(&self) -> bool { false }
}
impl Eq for AcceleratorTargetDesc { }
impl PartialEq for AcceleratorTargetDesc {
  fn eq(&self, rhs: &Self) -> bool {
    self.allow_indirect_function_calls == rhs.allow_indirect_function_calls &&
      self.kernel_abi == rhs.kernel_abi &&
      self.host_target == rhs.host_target &&
      self.target == rhs.target &&
      AnyHash::eq(self.platform.as_any_hash(),
                  rhs.platform.as_any_hash())
  }
}
impl Hash for AcceleratorTargetDesc {
  fn hash<H>(&self, hasher: &mut H)
    where H: Hasher,
  {
    ::std::hash::Hash::hash(&self.allow_indirect_function_calls,
                            hasher);
    ::std::hash::Hash::hash(&self.kernel_abi, hasher);

    macro_rules! impl_for {
      ({
        $(pub $field_name:ident: $field_ty:ty,)*
      }, $field:expr) => (
        $(::std::hash::Hash::hash(&$field.$field_name, hasher);)*
      );
    }

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
    }, self.host_target);

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
      pub is_like_fuchsia: bool,
      pub linker_is_gnu: bool,
      pub allows_weak_linkage: bool,
      pub has_rpath: bool,
      pub no_default_libraries: bool,
      pub position_independent_executables: bool,
      pub relro_level: RelroLevel,
      pub archive_format: String,
      pub allow_asm: bool,
      pub has_elf_tls: bool,
      pub obj_is_bitcode: bool,
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
      pub codegen_backend: String,
      pub default_hidden_visibility: bool,
      pub emit_debug_gdb_scripts: bool,
      pub requires_uwtable: bool,
      pub override_export_symbols: Option<Vec<String>>,
      pub addr_spaces: AddrSpaces,
    }, self.target.options);
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
      pub is_like_fuchsia: bool,
      pub linker_is_gnu: bool,
      pub allows_weak_linkage: bool,
      pub has_rpath: bool,
      pub no_default_libraries: bool,
      pub position_independent_executables: bool,
      pub relro_level: RelroLevel,
      pub archive_format: String,
      pub allow_asm: bool,
      pub has_elf_tls: bool,
      pub obj_is_bitcode: bool,
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
      pub codegen_backend: String,
      pub default_hidden_visibility: bool,
      pub emit_debug_gdb_scripts: bool,
      pub requires_uwtable: bool,
      pub override_export_symbols: Option<Vec<String>>,
      pub addr_spaces: AddrSpaces,
    }, self.host_target.options);

    let platform = self.platform.as_any_hash();
    platform.hash(hasher);
  }
}

/// Info specific to a OS/API combo. This must not contain device-unique
/// data. An example of data to not include would be a PCIe address of a
/// device.
pub trait PlatformTargetDesc
  where Self: Debug + erased_serde::Serialize,
        Self: AnyHash + Sync + Send + 'static,
{
  fn as_any_hash(&self) -> &dyn AnyHash;

  fn downcast_ref(this: &dyn PlatformTargetDesc) -> Option<&Self>
    where Self: Sized,
  {
    use std::any::TypeId;
    use std::mem::transmute;
    use std::raw::TraitObject;

    let this_tyid = Any::type_id(this);
    let self_tyid = TypeId::of::<Self>();
    if this_tyid != self_tyid {
      return None;
    }

    // We have to do this manually.
    let this: TraitObject = unsafe { transmute(this) };
    let this = this.data as *mut Self;
    Some(unsafe { &*this })
  }
  fn downcast_box(this: Box<dyn PlatformTargetDesc>)
    -> Result<Box<Self>, Box<dyn PlatformTargetDesc>>
    where Self: Sized,
  {
    use std::any::TypeId;
    use std::mem::transmute;
    use std::raw::TraitObject;

    let this_tyid = Any::type_id(&*this);
    let self_tyid = TypeId::of::<Self>();
    if this_tyid != self_tyid {
      return Err(this);
    }

    // We have to do this manually.
    let this: TraitObject = unsafe { transmute(Box::into_raw(this)) };
    let this = this.data as *mut Self;
    Ok(unsafe { Box::from_raw(this) })
  }
  fn downcast_arc(this: &Arc<dyn PlatformTargetDesc>) -> Option<Arc<Self>>
    where Self: Sized,
  {
    use std::any::TypeId;
    use std::mem::transmute;
    use std::raw::TraitObject;

    let this_tyid = Any::type_id(&**this);
    let self_tyid = TypeId::of::<Self>();
    if this_tyid != self_tyid {
      return None;
    }

    // We have to do this manually.
    let this = this.clone();
    let this = Arc::into_raw(this);
    let this: TraitObject = unsafe { transmute(this) };
    let this = this.data as *mut Self;
    Some(unsafe { Arc::from_raw(this) })
  }
}
erased_serde::serialize_trait_object!(PlatformTargetDesc);

#[cfg(test)]
mod test {
  use super::*;

  use serde::*;

  #[derive(Clone, Debug, Serialize, Deserialize, Hash, Eq, PartialEq)]
  pub struct MyTargetDesc;
  impl PlatformTargetDesc for MyTargetDesc {
    fn as_any_hash(&self) -> &dyn AnyHash { self }
  }

  #[test]
  fn target_desc_downcast() {
    let arc = Arc::new(MyTargetDesc) as Arc<dyn PlatformTargetDesc>;
    assert!(MyTargetDesc::downcast_arc(&arc).is_some());
    assert!(MyTargetDesc::downcast_ref(&*arc).is_some());
    let b = Box::new(MyTargetDesc) as Box<dyn PlatformTargetDesc>;
    assert!(MyTargetDesc::downcast_box(b).is_ok());
  }

  #[derive(Debug)]
  struct MyAccelerator;

  impl Accelerator for MyAccelerator {
    fn id(&self) -> AcceleratorId { unimplemented!() }
    fn platform(&self) -> Platform { unimplemented!() }
    fn accel_target_desc(&self) -> &Arc<AcceleratorTargetDesc> { unimplemented!() }
    fn set_accel_target_desc(&mut self, _desc: Arc<AcceleratorTargetDesc>) { unimplemented!() }
    fn create_target_codegen(self: &mut Arc<Self>, _ctxt: &Context)
      -> Result<Arc<dyn Any + Send + Sync + 'static>, Box<dyn Error + Send + Sync + 'static>>
      where Self: Sized,
    { unimplemented!() }
    fn set_target_codegen(self: &mut Arc<Self>,
                          _codegen_comms: Arc<dyn Any + Send + Sync + 'static>)
      where Self: Sized,
    { unimplemented!() }
  }
  #[test]
  fn accelerator_downcast() {
    let arc = Arc::new(MyAccelerator) as Arc<dyn Accelerator>;
    assert!(MyAccelerator::downcast_arc(&arc).is_some());
    assert!(MyAccelerator::downcast_ref(&*arc).is_some());
  }
}
