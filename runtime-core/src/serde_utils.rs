
use std::collections::{BTreeMap, BTreeSet, };
use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize, };
use serde::de::{Visitor, MapAccess, SeqAccess, };

use rustc_target::spec;

mod link_args {
  use serde::*;
  use std::collections::BTreeMap;

  use rustc_target::spec;

  use super::*;

  pub type InK = spec::LinkerFlavor;
  pub type InV = Vec<String>;
  pub type OutK = LinkerFlavor;
  pub type OutV = InV;
  pub type Output = BTreeMap<InK, InV>;

  pub fn deserialize<'de, D>(deserializer: D)
    -> Result<Output, D::Error>
    where D: Deserializer<'de>,
  {
    btree_map::deserialize::<D, InK, InV, OutK, OutV>(deserializer)
  }
  pub fn serialize<S>(this: &Output, serializer: S)
    -> Result<S::Ok, S::Error>
    where S: Serializer,
  {
    btree_map::serialize::<S, InK, InV, OutK, OutV>(this, serializer)
  }
}
mod crt_objects {
  use serde::*;
  use std::collections::BTreeMap;

  use rustc_target::spec;

  use super::*;

  pub type InK = spec::LinkOutputKind;
  pub type InV = Vec<String>;
  pub type OutK = LinkOutputKind;
  pub type OutV = InV;
  pub type Output = BTreeMap<InK, InV>;

  pub fn deserialize<'de, D>(deserializer: D)
                             -> Result<Output, D::Error>
    where D: Deserializer<'de>,
  {
    btree_map::deserialize::<D, InK, InV, OutK, OutV>(deserializer)
  }
  pub fn serialize<S>(this: &Output, serializer: S)
                      -> Result<S::Ok, S::Error>
    where S: Serializer,
  {
    btree_map::serialize::<S, InK, InV, OutK, OutV>(this, serializer)
  }
}
mod crt_objects_fallback {
  use serde::*;

  use rustc_target::spec;

  use super::*;

  pub type In = spec::crt_objects::CrtObjectsFallback;
  pub type Out = CrtObjectsFallback;
  pub type Output = Option<In>;

  pub fn deserialize<'de, D>(deserializer: D) -> Result<Output, D::Error>
    where D: Deserializer<'de>,
  {
    option::deserialize::<D, In, Out>(deserializer)
  }
  pub fn serialize<S>(this: &Output, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer,
  {
    option::serialize::<S, In, Out>(this, serializer)
  }
}
mod code_model {
  use serde::*;

  use rustc_target::spec;

  use super::*;

  pub type In = spec::CodeModel;
  pub type Out = CodeModel;
  pub type Output = Option<In>;

  pub fn deserialize<'de, D>(deserializer: D) -> Result<Output, D::Error>
    where D: Deserializer<'de>,
  {
    option::deserialize::<D, In, Out>(deserializer)
  }
  pub fn serialize<S>(this: &Output, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer,
  {
    option::serialize::<S, In, Out>(this, serializer)
  }
}
/// should be `abis`. oops.
mod apis {
  use serde::*;

  use rustc_target::spec;

  use super::*;

  pub type In = spec::abi::Abi;
  pub type Out = Abi;
  pub type Output = Vec<spec::abi::Abi>;

  pub fn deserialize<'de, D>(deserializer: D) -> Result<Output, D::Error>
    where D: Deserializer<'de>,
  {
    vec::deserialize::<D, In, Out>(deserializer)
  }
  pub fn serialize<S>(this: &Output, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer,
  {
    vec::serialize::<S, In, Out>(this, serializer)
  }
}
pub mod abi {
  use serde::*;

  use rustc_target::spec;

  use super::*;

  pub type Output = spec::abi::Abi;

  #[allow(dead_code)] // for completeness.
  pub fn deserialize<'de, D>(deserializer: D) -> Result<Output, D::Error>
    where D: Deserializer<'de>,
  {
    Abi::deserialize::<D>(deserializer)
      .map(Into::into)
  }
  pub fn serialize<S>(this: &Output, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer,
  {
    Abi::serialize::<S>(&Abi::from(*this), serializer)
  }
}



#[derive(Serialize, Deserialize)]
#[serde(remote = "::rustc_target::spec::Target")]
pub struct Target {
  /// Target triple to pass to LLVM.
  pub llvm_target: String,
  /// String to use as the `target_endian` `cfg` variable.
  pub target_endian: String,
  /// String to use as the `target_pointer_width` `cfg` variable.
  pub target_pointer_width: String,
  /// Width of c_int type
  pub target_c_int_width: String,
  /// OS name to use for conditional compilation.
  pub target_os: String,
  /// Environment name to use for conditional compilation.
  pub target_env: String,
  /// Vendor name to use for conditional compilation.
  pub target_vendor: String,
  /// Architecture to use for ABI considerations. Valid options: "x86",
  /// "x86_64", "arm", "aarch64", "mips", "powerpc", and "powerpc64".
  pub arch: String,
  /// [Data layout](http://llvm.org/docs/LangRef.html#data-layout) to pass to LLVM.
  pub data_layout: String,
  /// Linker flavor
  #[serde(with = "self::linker_flavor")]
  pub linker_flavor: ::rustc_target::spec::LinkerFlavor,
  /// Optional settings with defaults.
  #[serde(with = "TargetOptions")]
  pub options: ::rustc_target::spec::TargetOptions,
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "::rustc_target::spec::TargetOptions")]
pub struct TargetOptions {
  /// Whether the target is built-in or loaded from a custom target specification.
  pub is_builtin: bool,

  /// Linker to invoke
  pub linker: Option<String>,

  /// LLD flavor
  #[serde(with = "LldFlavor")]
  pub lld_flavor: ::rustc_target::spec::LldFlavor,

  /// Linker arguments that are passed *before* any user-defined libraries.
  #[serde(with = "self::link_args")]
  pub pre_link_args: ::rustc_target::spec::LinkArgs,

  /// Objects to link before all others, always found within the
  /// sysroot folder.
  #[serde(with = "self::crt_objects")]
  pub pre_link_objects: ::rustc_target::spec::crt_objects::CrtObjects,
  #[serde(with = "self::crt_objects")]
  pub post_link_objects: ::rustc_target::spec::crt_objects::CrtObjects,
  /// Same as `(pre|post)_link_objects`, but when we fail to pull the objects with help of the
  /// target's native gcc and fall back to the "self-contained" mode and pull them manually.
  /// See `crt_objects.rs` for some more detailed documentation.
  #[serde(with = "self::crt_objects")]
  pub pre_link_objects_fallback: ::rustc_target::spec::crt_objects::CrtObjects,
  #[serde(with = "self::crt_objects")]
  pub post_link_objects_fallback: ::rustc_target::spec::crt_objects::CrtObjects,
  /// Which logic to use to determine whether to fall back to the "self-contained" mode or not.
  #[serde(with = "self::crt_objects_fallback")]
  pub crt_objects_fallback: Option<::rustc_target::spec::crt_objects::CrtObjectsFallback>,

  /// Linker arguments that are unconditionally passed after any
  /// user-defined but before post_link_objects.  Standard platform
  /// libraries that should be always be linked to, usually go here.
  #[serde(with = "self::link_args")]
  pub late_link_args: ::rustc_target::spec::LinkArgs,
  /// Linker arguments used in addition to `late_link_args` if at least one
  /// Rust dependency is dynamically linked.
  #[serde(with = "self::link_args")]
  pub late_link_args_dynamic: ::rustc_target::spec::LinkArgs,
  /// Linker arguments used in addition to `late_link_args` if aall Rust
  /// dependencies are statically linked.
  #[serde(with = "self::link_args")]
  pub late_link_args_static: ::rustc_target::spec::LinkArgs,
  /// Objects to link after all others, always found within the
  /// sysroot folder.
  /// Linker arguments that are unconditionally passed *after* any
  /// user-defined libraries.
  #[serde(with = "self::link_args")]
  pub post_link_args: ::rustc_target::spec::LinkArgs,

  /// Optional link script applied to `dylib` and `executable` crate types.
  /// This is a string containing the script, not a path. Can only be applied
  /// to linkers where `linker_is_gnu` is true.
  pub link_script: Option<String>,

  /// Environment variables to be set before invoking the linker.
  pub link_env: Vec<(String, String)>,
  /// Environment variables to be removed for the linker invocation.
  pub link_env_remove: Vec<String>,

  /// Extra arguments to pass to the external assembler (when used)
  pub asm_args: Vec<String>,

  /// Default CPU to pass to LLVM. Corresponds to `llc -mcpu=$cpu`. Defaults
  /// to "generic".
  pub cpu: String,
  /// Default target features to pass to LLVM. These features will *always* be
  /// passed, and cannot be disabled even via `-C`. Corresponds to `llc
  /// -mattr=$features`.
  pub features: String,
  /// Whether dynamic linking is available on this target. Defaults to false.
  pub dynamic_linking: bool,
  /// If dynamic linking is available, whether only cdylibs are supported.
  pub only_cdylib: bool,
  /// Whether executables are available on this target. iOS, for example, only allows static
  /// libraries. Defaults to false.
  pub executables: bool,
  /// Relocation model to use in object file. Corresponds to `llc
  /// -relocation-model=$relocation_model`. Defaults to "pic".
  #[serde(with = "RelocModel")]
  pub relocation_model: ::rustc_target::spec::RelocModel,
  /// Code model to use. Corresponds to `llc -code-model=$code_model`.
  #[serde(with = "self::code_model")]
  pub code_model: Option<::rustc_target::spec::CodeModel>,
  /// TLS model to use. Options are "global-dynamic" (default), "local-dynamic", "initial-exec"
  /// and "local-exec". This is similar to the -ftls-model option in GCC/Clang.
  #[serde(with = "TlsModel")]
  pub tls_model: ::rustc_target::spec::TlsModel,
  /// Do not emit code that uses the "red zone", if the ABI has one. Defaults to false.
  pub disable_redzone: bool,
  /// Eliminate frame pointers from stack frames if possible. Defaults to true.
  pub eliminate_frame_pointer: bool,
  /// Emit each function in its own section. Defaults to true.
  pub function_sections: bool,
  /// String to prepend to the name of every dynamic library. Defaults to "lib".
  pub dll_prefix: String,
  /// String to append to the name of every dynamic library. Defaults to ".so".
  pub dll_suffix: String,
  /// String to append to the name of every executable.
  pub exe_suffix: String,
  /// String to prepend to the name of every static library. Defaults to "lib".
  pub staticlib_prefix: String,
  /// String to append to the name of every static library. Defaults to ".a".
  pub staticlib_suffix: String,
  /// OS family to use for conditional compilation. Valid options: "unix", "windows".
  pub target_family: Option<String>,
  /// Whether the target toolchain's ABI supports returning small structs as an integer.
  pub abi_return_struct_as_int: bool,
  /// Whether the target toolchain is like macOS's. Only useful for compiling against iOS/macOS,
  /// in particular running dsymutil and some other stuff like `-dead_strip`. Defaults to false.
  pub is_like_osx: bool,
  /// Whether the target toolchain is like Solaris's.
  /// Only useful for compiling against Illumos/Solaris,
  /// as they have a different set of linker flags. Defaults to false.
  pub is_like_solaris: bool,
  /// Whether the target toolchain is like Windows'. Only useful for compiling against Windows,
  /// only really used for figuring out how to find libraries, since Windows uses its own
  /// library naming convention. Defaults to false.
  pub is_like_windows: bool,
  pub is_like_msvc: bool,
  /// Whether the target toolchain is like Android's. Only useful for compiling against Android.
  /// Defaults to false.
  pub is_like_android: bool,
  /// Whether the target toolchain is like Emscripten's. Only useful for compiling with
  /// Emscripten toolchain.
  /// Defaults to false.
  pub is_like_emscripten: bool,
  /// Whether the target toolchain is like Fuchsia's.
  pub is_like_fuchsia: bool,
  /// Whether the linker support GNU-like arguments such as -O. Defaults to false.
  pub linker_is_gnu: bool,
  /// The MinGW toolchain has a known issue that prevents it from correctly
  /// handling COFF object files with more than 2<sup>15</sup> sections. Since each weak
  /// symbol needs its own COMDAT section, weak linkage implies a large
  /// number sections that easily exceeds the given limit for larger
  /// codebases. Consequently we want a way to disallow weak linkage on some
  /// platforms.
  pub allows_weak_linkage: bool,
  /// Whether the linker support rpaths or not. Defaults to false.
  pub has_rpath: bool,
  /// Whether to disable linking to the default libraries, typically corresponds
  /// to `-nodefaultlibs`. Defaults to true.
  pub no_default_libraries: bool,
  /// Dynamically linked executables can be compiled as position independent
  /// if the default relocation model of position independent code is not
  /// changed. This is a requirement to take advantage of ASLR, as otherwise
  /// the functions in the executable are not randomized and can be used
  /// during an exploit of a vulnerability in any code.
  pub position_independent_executables: bool,
  /// Executables that are both statically linked and position-independent are supported.
  pub static_position_independent_executables: bool,
  /// Determines if the target always requires using the PLT for indirect
  /// library calls or not. This controls the default value of the `-Z plt` flag.
  pub needs_plt: bool,
  /// Either partial, full, or off. Full RELRO makes the dynamic linker
  /// resolve all symbols at startup and marks the GOT read-only before
  /// starting the program, preventing overwriting the GOT.
  #[serde(with = "RelroLevel")]
  pub relro_level: ::rustc_target::spec::RelroLevel,
  /// Format that archives should be emitted in. This affects whether we use
  /// LLVM to assemble an archive or fall back to the system linker, and
  /// currently only "gnu" is used to fall into LLVM. Unknown strings cause
  /// the system linker to be used.
  pub archive_format: String,
  /// Is asm!() allowed? Defaults to true.
  pub allow_asm: bool,
  /// Whether the runtime startup code requires the `main` function be passed
  /// `argc` and `argv` values.
  pub main_needs_argc_argv: bool,

  /// Flag indicating whether ELF TLS (e.g. #[thread_local]) is available for
  /// this target.
  pub has_elf_tls: bool,
  // This is mainly for easy compatibility with emscripten.
  // If we give emcc .o files that are actually .bc files it
  // will 'just work'.
  pub obj_is_bitcode: bool,
  /// Whether the target requires that emitted object code includes bitcode.
  pub forces_embed_bitcode: bool,
  /// Content of the LLVM cmdline section associated with embedded bitcode.
  pub bitcode_llvm_cmdline: String,

  /// Don't use this field; instead use the `.min_atomic_width()` method.
  pub min_atomic_width: Option<u64>,

  /// Don't use this field; instead use the `.max_atomic_width()` method.
  pub max_atomic_width: Option<u64>,

  /// Whether the target supports atomic CAS operations natively
  pub atomic_cas: bool,

  /// Panic strategy: "unwind" or "abort"
  #[serde(with = "PanicStrategy")]
  pub panic_strategy: ::rustc_target::spec::PanicStrategy,

  /// A blacklist of ABIs unsupported by the current target. Note that generic
  /// ABIs are considered to be supported on all platforms and cannot be blacklisted.
  #[serde(with = "self::apis")]
  pub unsupported_abis: Vec<::rustc_target::spec::abi::Abi>,

  /// Whether or not linking dylibs to a static CRT is allowed.
  pub crt_static_allows_dylibs: bool,
  /// Whether or not the CRT is statically linked by default.
  pub crt_static_default: bool,
  /// Whether or not crt-static is respected by the compiler (or is a no-op).
  pub crt_static_respected: bool,

  /// Whether or not stack probes (__rust_probestack) are enabled
  pub stack_probes: bool,

  /// The minimum alignment for global symbols.
  pub min_global_align: Option<u64>,

  /// Default number of codegen units to use in debug mode
  pub default_codegen_units: Option<u64>,

  /// Whether to generate trap instructions in places where optimization would
  /// otherwise produce control flow that falls through into unrelated memory.
  pub trap_unreachable: bool,

  /// This target requires everything to be compiled with LTO to emit a final
  /// executable, aka there is no native linker for this target.
  pub requires_lto: bool,

  /// This target has no support for threads.
  pub singlethread: bool,

  /// Whether library functions call lowering/optimization is disabled in LLVM
  /// for this target unconditionally.
  pub no_builtins: bool,

  /// The default visibility for symbols in this target should be "hidden"
  /// rather than "default"
  pub default_hidden_visibility: bool,

  /// Whether a .debug_gdb_scripts section will be added to the output object file
  pub emit_debug_gdb_scripts: bool,

  /// Whether or not to unconditionally `uwtable` attributes on functions,
  /// typically because the platform needs to unwind for things like stack
  /// unwinders.
  pub requires_uwtable: bool,

  /// Whether or not SIMD types are passed by reference in the Rust ABI,
  /// typically required if a target can be compiled with a mixed set of
  /// target features. This is `true` by default, and `false` for targets like
  /// wasm32 where the whole program either has simd or not.
  pub simd_types_indirect: bool,

  /// Pass a list of symbol which should be exported in the dylib to the linker.
  pub limit_rdylib_exports: bool,

  /// If set, have the linker export exactly these symbols, instead of using
  /// the usual logic to figure this out from the crate itself.
  pub override_export_symbols: Option<Vec<String>>,

  /// Description of all address spaces and how they are shared with one another.
  /// Defaults to a single, flat, address space. Note it is generally assumed that
  /// the address space `0` is your flat address space.
  #[serde(with = "AddrSpaces")]
  pub addr_spaces: ::rustc_target::spec::AddrSpaces,

  /// Determines how or whether the MergeFunctions LLVM pass should run for
  /// this target. Either "disabled", "trampolines", or "aliases".
  /// The MergeFunctions pass is generally useful, but some targets may need
  /// to opt out. The default is "aliases".
  ///
  /// Workaround for: https://github.com/rust-lang/rust/issues/57356
  #[serde(with = "MergeFunctions")]
  pub merge_functions: ::rustc_target::spec::MergeFunctions,

  /// Use platform dependent mcount function
  pub target_mcount: String,

  /// LLVM ABI name, corresponds to the '-mabi' parameter available in multilib C compilers
  pub llvm_abiname: String,

  /// Whether or not RelaxElfRelocation flag will be passed to the linker
  pub relax_elf_relocations: bool,

  /// Additional arguments to pass to LLVM, similar to the `-C llvm-args` codegen option.
  pub llvm_args: Vec<String>,

  /// Whether to use legacy .ctors initialization hooks rather than .init_array. Defaults
  /// to false (uses .init_array).
  pub use_ctors_section: bool,

  /// Whether the linker is instructed to add a `GNU_EH_FRAME` ELF header
  /// used to locate unwinding information is passed
  /// (only has effect if the linker is `ld`-like).
  pub eh_frame_header: bool,
}

#[derive(Serialize, Deserialize)]
pub enum CrtObjectsFallback {
  Musl,
  Mingw,
  Wasm,
}
impl Into<spec::crt_objects::CrtObjectsFallback> for CrtObjectsFallback {
  fn into(self) -> spec::crt_objects::CrtObjectsFallback {
    use self::CrtObjectsFallback::*;
    use rustc_target::spec::crt_objects::CrtObjectsFallback;

    match self {
      Musl => CrtObjectsFallback::Musl,
      Mingw => CrtObjectsFallback::Mingw,
      Wasm => CrtObjectsFallback::Wasm,
    }
  }
}
impl From<spec::crt_objects::CrtObjectsFallback> for CrtObjectsFallback {
  fn from(v: spec::crt_objects::CrtObjectsFallback) -> CrtObjectsFallback {
    use rustc_target::spec::crt_objects::CrtObjectsFallback::*;

    match v {
      Musl => CrtObjectsFallback::Musl,
      Mingw => CrtObjectsFallback::Mingw,
      Wasm => CrtObjectsFallback::Wasm,
    }
  }
}
#[derive(Serialize, Deserialize)]
pub enum LinkOutputKind {
  /// Dynamically linked non position-independent executable.
  DynamicNoPicExe,
  /// Dynamically linked position-independent executable.
  DynamicPicExe,
  /// Statically linked non position-independent executable.
  StaticNoPicExe,
  /// Statically linked position-independent executable.
  StaticPicExe,
  /// Regular dynamic library ("dynamically linked").
  DynamicDylib,
  /// Dynamic library with bundled libc ("statically linked").
  StaticDylib,
}
impl Into<spec::LinkOutputKind> for LinkOutputKind {
  fn into(self) -> spec::LinkOutputKind {
    use self::LinkOutputKind::*;
    use rustc_target::spec::LinkOutputKind;

    match self {
      DynamicNoPicExe => LinkOutputKind::DynamicNoPicExe,
      DynamicPicExe => LinkOutputKind::DynamicPicExe,
      StaticNoPicExe => LinkOutputKind::StaticNoPicExe,
      StaticPicExe => LinkOutputKind::StaticPicExe,
      DynamicDylib => LinkOutputKind::DynamicDylib,
      StaticDylib => LinkOutputKind::StaticDylib,
    }
  }
}
impl From<spec::LinkOutputKind> for LinkOutputKind {
  fn from(v: spec::LinkOutputKind) -> LinkOutputKind {
    use rustc_target::spec::LinkOutputKind::*;

    match v {
      DynamicNoPicExe => LinkOutputKind::DynamicNoPicExe,
      DynamicPicExe => LinkOutputKind::DynamicPicExe,
      StaticNoPicExe => LinkOutputKind::StaticNoPicExe,
      StaticPicExe => LinkOutputKind::StaticPicExe,
      DynamicDylib => LinkOutputKind::DynamicDylib,
      StaticDylib => LinkOutputKind::StaticDylib,
    }
  }
}
#[derive(Serialize, Deserialize)]
pub enum CodeModel {
  Tiny,
  Small,
  Kernel,
  Medium,
  Large,
}
impl Into<spec::CodeModel> for CodeModel {
  fn into(self) -> spec::CodeModel {
    use self::CodeModel::*;
    use rustc_target::spec::CodeModel;

    match self {
      Tiny => CodeModel::Tiny,
      Small => CodeModel::Small,
      Kernel => CodeModel::Kernel,
      Medium => CodeModel::Medium,
      Large => CodeModel::Large,
    }
  }
}
impl From<spec::CodeModel> for CodeModel {
  fn from(v: spec::CodeModel) -> CodeModel {
    use rustc_target::spec::CodeModel::*;

    match v {
      Tiny => CodeModel::Tiny,
      Small => CodeModel::Small,
      Kernel => CodeModel::Kernel,
      Medium => CodeModel::Medium,
      Large => CodeModel::Large,
    }
  }
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "::rustc_target::spec::RelocModel")]
pub enum RelocModel {
  Static,
  Pic,
  DynamicNoPic,
  Ropi,
  Rwpi,
  RopiRwpi,
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "::rustc_target::spec::TlsModel")]
pub enum TlsModel {
  GeneralDynamic,
  LocalDynamic,
  InitialExec,
  LocalExec,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "::rustc_target::spec::AddrSpaceIdx")]
pub struct AddrSpaceIdx(pub u32);

mod addr_spaces {
  use serde::*;
  use std::collections::BTreeMap;

  use rustc_target::spec;

  use super::*;

  pub type InK = spec::AddrSpaceKind;
  pub type InV = spec::AddrSpaceProps;
  pub type OutK = AddrSpaceKind;
  pub type OutV = AddrSpaceProps;
  pub type Output = BTreeMap<InK, InV>;

  pub fn deserialize<'de, D>(deserializer: D) -> Result<Output, D::Error>
    where D: Deserializer<'de>,
  {
    btree_map::deserialize::<D, InK, InV, OutK, OutV>(deserializer)
  }
  pub fn serialize<S>(this: &Output, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer,
  {
    btree_map::serialize::<S, InK, InV, OutK, OutV>(this, serializer)
  }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "::rustc_target::spec::AddrSpaces")]
pub struct AddrSpaces(#[serde(with = "self::addr_spaces")] pub BTreeMap<
  ::rustc_target::spec::AddrSpaceKind,
  ::rustc_target::spec::AddrSpaceProps
>);

#[derive(Serialize, Deserialize, Ord, PartialOrd, PartialEq, Eq)]
pub enum AddrSpaceKind {
  Flat,
  Alloca,
  /// aka constant
  ReadOnly,
  /// aka global
  ReadWrite,
  Instruction,
  Named(String),
}
impl Into<spec::AddrSpaceKind> for AddrSpaceKind {
  fn into(self) -> spec::AddrSpaceKind {
    match self {
      AddrSpaceKind::Flat => spec::AddrSpaceKind::Flat,
      AddrSpaceKind::Alloca => spec::AddrSpaceKind::Alloca,
      AddrSpaceKind::ReadOnly => spec::AddrSpaceKind::ReadOnly,
      AddrSpaceKind::ReadWrite => spec::AddrSpaceKind::ReadWrite,
      AddrSpaceKind::Instruction => spec::AddrSpaceKind::Instruction,
      AddrSpaceKind::Named(name) => spec::AddrSpaceKind::Named(name),
    }
  }
}
impl From<spec::AddrSpaceKind> for AddrSpaceKind {
  fn from(v: spec::AddrSpaceKind) -> Self {
    match v {
      spec::AddrSpaceKind::Flat => AddrSpaceKind::Flat,
      spec::AddrSpaceKind::Alloca => AddrSpaceKind::Alloca,
      spec::AddrSpaceKind::ReadOnly => AddrSpaceKind::ReadOnly,
      spec::AddrSpaceKind::ReadWrite => AddrSpaceKind::ReadWrite,
      spec::AddrSpaceKind::Instruction => AddrSpaceKind::Instruction,
      spec::AddrSpaceKind::Named(name) => AddrSpaceKind::Named(name),
    }
  }
}

mod addr_space_props_shared_with {
  use serde::*;
  use std::collections::BTreeSet;

  use rustc_target::spec;

  use super::*;

  pub type InK = spec::AddrSpaceKind;
  pub type OutK = AddrSpaceKind;
  pub type Output = BTreeSet<InK>;

  pub fn deserialize<'de, D>(deserializer: D) -> Result<Output, D::Error>
    where D: Deserializer<'de>,
  {
    btree_set::deserialize::<D, InK, OutK>(deserializer)
  }
  pub fn serialize<S>(this: &Output, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer,
  {
    btree_set::serialize::<S, InK, OutK>(this, serializer)
  }
}
/// TODO: make it so we don't have to clone this to serialize it.
#[derive(Serialize, Deserialize)]
pub struct AddrSpaceProps {
  #[serde(with = "AddrSpaceIdx")]
  pub index: spec::AddrSpaceIdx,
  /// Indicates which addr spaces this addr space can be addrspacecast-ed to.
  #[serde(with = "self::addr_space_props_shared_with")]
  pub shared_with: BTreeSet<spec::AddrSpaceKind>,
}
impl Into<spec::AddrSpaceProps> for AddrSpaceProps {
  fn into(self) -> spec::AddrSpaceProps {
    let AddrSpaceProps {
      index, shared_with,
    } = self;
    spec::AddrSpaceProps {
      index,
      shared_with,
    }
  }
}
impl From<spec::AddrSpaceProps> for AddrSpaceProps {
  fn from(v: spec::AddrSpaceProps) -> Self {
    let spec::AddrSpaceProps {
      index, shared_with,
    } = v;
    AddrSpaceProps {
      index,
      shared_with,
    }
  }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "spec::RelroLevel")]
pub enum RelroLevel {
    Full,
    Partial,
    Off,
    None,
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "spec::PanicStrategy")]
pub enum PanicStrategy {
    Unwind,
    Abort,
}

#[derive(Serialize, Deserialize)]
pub enum LinkerFlavor {
  Em,
  Gcc,
  Ld,
  Msvc,
  Lld(#[serde(with = "LldFlavor")] ::rustc_target::spec::LldFlavor),
  PtxLinker,
}
mod linker_flavor {
  use serde::*;

  use rustc_target::spec;

  use super::*;

  pub type Output = spec::LinkerFlavor;

  pub fn deserialize<'de, D>(deserializer: D) -> Result<Output, D::Error>
    where D: Deserializer<'de>,
  {
    Ok(LinkerFlavor::deserialize(deserializer)?.into())
  }
  pub fn serialize<S>(this: &Output, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer,
  {
    let this = this.clone().into();
    LinkerFlavor::serialize(&this, serializer)
  }
}

impl Into<spec::LinkerFlavor> for LinkerFlavor {
  fn into(self) -> spec::LinkerFlavor {
    use self::LinkerFlavor::*;
    use rustc_target::spec::LinkerFlavor;

    match self {
      Em => LinkerFlavor::Em,
      Gcc => LinkerFlavor::Gcc,
      Ld => LinkerFlavor::Ld,
      Msvc => LinkerFlavor::Msvc,
      Lld(v) => LinkerFlavor::Lld(v.into()),
      PtxLinker => LinkerFlavor::PtxLinker,
    }
  }
}
impl From<spec::LinkerFlavor> for LinkerFlavor {
  fn from(v: spec::LinkerFlavor) -> LinkerFlavor {
    use rustc_target::spec::LinkerFlavor::*;

    match v {
      Em => LinkerFlavor::Em,
      Gcc => LinkerFlavor::Gcc,
      Ld => LinkerFlavor::Ld,
      Msvc => LinkerFlavor::Msvc,
      Lld(v) => LinkerFlavor::Lld(v.into()),
      PtxLinker => LinkerFlavor::PtxLinker,
    }
  }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "spec::LldFlavor")]
pub enum LldFlavor {
    Wasm,
    Ld64,
    Ld,
    Link,
}
impl Into<spec::LldFlavor> for LldFlavor {
  fn into(self) -> spec::LldFlavor {
    use self::LldFlavor::*;
    use rustc_target::spec::LldFlavor;

    match self {
      Wasm => LldFlavor::Wasm,
      Ld64 => LldFlavor::Ld64,
      Ld => LldFlavor::Ld,
      Link => LldFlavor::Link,
    }
  }
}
impl From<spec::LldFlavor> for LldFlavor {
  fn from(v: spec::LldFlavor) -> LldFlavor {
    use rustc_target::spec::LldFlavor::*;

    match v {
      Wasm => LldFlavor::Wasm,
      Ld64 => LldFlavor::Ld64,
      Ld => LldFlavor::Ld,
      Link => LldFlavor::Link,
    }
  }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "::rustc_target::spec::MergeFunctions")]
pub enum MergeFunctions {
  Disabled,
  Trampolines,
  Aliases
}

#[derive(Serialize, Deserialize)]
pub enum Abi {
  // NB: This ordering MUST match the AbiDatas array below.
  // (This is ensured by the test indices_are_correct().)

  // Single platform ABIs
  Cdecl,
  Stdcall,
  Fastcall,
  Vectorcall,
  Thiscall,
  Aapcs,
  Win64,
  SysV64,
  PtxKernel,
  Msp430Interrupt,
  X86Interrupt,
  AmdGpuKernel,
  SpirKernel,
  EfiApi,
  AvrInterrupt,
  AvrNonBlockingInterrupt,

  // Multiplatform / generic ABIs
  Rust,
  C,
  System,
  RustIntrinsic,
  RustCall,
  PlatformIntrinsic,
  Unadjusted
}
impl Into<spec::abi::Abi> for Abi {
  fn into(self) -> spec::abi::Abi {
    use self::Abi::*;
    use rustc_target::spec::abi::Abi;

    match self {
      // Single platform ABIs
      Cdecl => Abi::Cdecl,
      Stdcall => Abi::Stdcall,
      Fastcall => Abi::Fastcall,
      Vectorcall => Abi::Vectorcall,
      Thiscall => Abi::Thiscall,
      Aapcs => Abi::Aapcs,
      Win64 => Abi::Win64,
      SysV64 => Abi::SysV64,
      PtxKernel => Abi::PtxKernel,
      Msp430Interrupt => Abi::Msp430Interrupt,
      X86Interrupt => Abi::X86Interrupt,
      AmdGpuKernel => Abi::AmdGpuKernel,
      SpirKernel => Abi::SpirKernel,
      EfiApi => Abi::EfiApi,
      AvrInterrupt => Abi::AvrInterrupt,
      AvrNonBlockingInterrupt => Abi::AvrNonBlockingInterrupt,

      // Multiplatform / generic ABIs
      Rust => Abi::Rust,
      C => Abi::C,
      System => Abi::System,
      RustIntrinsic => Abi::RustIntrinsic,
      RustCall => Abi::RustCall,
      PlatformIntrinsic => Abi::PlatformIntrinsic,
      Unadjusted => Abi::Unadjusted,
    }
  }
}
impl From<spec::abi::Abi> for Abi {
  fn from(v: spec::abi::Abi) -> Abi {
    use rustc_target::spec::abi::Abi::*;

    match v {
      // Single platform ABIs
      Cdecl => Abi::Cdecl,
      Stdcall => Abi::Stdcall,
      Fastcall => Abi::Fastcall,
      Vectorcall => Abi::Vectorcall,
      Thiscall => Abi::Thiscall,
      Aapcs => Abi::Aapcs,
      Win64 => Abi::Win64,
      SysV64 => Abi::SysV64,
      PtxKernel => Abi::PtxKernel,
      Msp430Interrupt => Abi::Msp430Interrupt,
      X86Interrupt => Abi::X86Interrupt,
      AmdGpuKernel => Abi::AmdGpuKernel,
      SpirKernel => Abi::SpirKernel,
      EfiApi => Abi::EfiApi,
      AvrInterrupt => Abi::AvrInterrupt,
      AvrNonBlockingInterrupt => Abi::AvrNonBlockingInterrupt,

      // Multiplatform / generic ABIs
      Rust => Abi::Rust,
      C => Abi::C,
      System => Abi::System,
      RustIntrinsic => Abi::RustIntrinsic,
      RustCall => Abi::RustCall,
      PlatformIntrinsic => Abi::PlatformIntrinsic,
      Unadjusted => Abi::Unadjusted,
    }
  }
}

pub struct BTreeMapDeVisitor<InK, InV, OutK, OutV> {
  _m: PhantomData<(InK, InV, OutK, OutV)>,
}
impl<InK, InV, OutK, OutV> Default for BTreeMapDeVisitor<InK, InV, OutK, OutV> {
  fn default() -> Self {
    BTreeMapDeVisitor {
      _m: PhantomData,
    }
  }
}

impl<'de, InK, InV, OutK, OutV> Visitor<'de> for BTreeMapDeVisitor<InK, InV, OutK, OutV>
  where InK: Ord,
        OutK: Deserialize<'de> + Into<InK>,
        OutV: Deserialize<'de> + Into<InV>,
{
  // The type that our Visitor is going to produce.
  type Value = BTreeMap<InK, InV>;

  // Format a message stating what data this Visitor expects to receive.
  fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
    formatter
      .write_str("BTreeMap for type which don't impl Deserialize")
  }

  fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where M: MapAccess<'de>,
  {
    let mut map = BTreeMap::new();

    // While there are entries remaining in the input, add them
    // into our map.
    while let Some((key, value)) = access.next_entry()? {
      let key: OutK = key;
      let value: OutV = value;
      let key: InK = key.into();
      let value: InV = value.into();
      map.insert(key, value);
    }

    Ok(map)
  }
}

pub struct BTreeSetDeVisitor<InK, OutK> {
  _m: PhantomData<(InK, OutK)>,
}
impl<InK, OutK> Default for BTreeSetDeVisitor<InK, OutK> {
  fn default() -> Self {
    BTreeSetDeVisitor {
      _m: PhantomData,
    }
  }
}
impl<'de, InK, OutK> Visitor<'de> for BTreeSetDeVisitor<InK, OutK>
  where InK: Ord,
        OutK: Deserialize<'de> + Into<InK>,
{
  // The type that our Visitor is going to produce.
  type Value = BTreeSet<InK>;

  // Format a message stating what data this Visitor expects to receive.
  fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
    formatter
      .write_str("BTreeMap for type which don't impl Deserialize")
  }

  fn visit_seq<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where M: SeqAccess<'de>,
  {
    let mut map = BTreeSet::new();

    // While there are entries remaining in the input, add them
    // into our map.
    while let Some(key) = access.next_element()? {
      let key: OutK = key;
      let key: InK = key.into();
      map.insert(key);
    }

    Ok(map)
  }
}
pub struct VecDeVisitor<InK, OutK> {
  _m: PhantomData<(InK, OutK)>,
}
impl<InK, OutK> Default for VecDeVisitor<InK, OutK> {
  fn default() -> Self {
    VecDeVisitor {
      _m: PhantomData,
    }
  }
}
impl<'de, InK, OutK> Visitor<'de> for VecDeVisitor<InK, OutK>
  where OutK: Deserialize<'de> + Into<InK>,
{
  // The type that our Visitor is going to produce.
  type Value = Vec<InK>;

  // Format a message stating what data this Visitor expects to receive.
  fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
    formatter
      .write_str("BTreeMap for type which don't impl Deserialize")
  }

  fn visit_seq<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where M: SeqAccess<'de>,
  {
    let size = access.size_hint().unwrap_or_default();
    let mut map = Vec::with_capacity(size);

    // While there are entries remaining in the input, add them
    // into our map.
    while let Some(key) = access.next_element()? {
      let key: OutK = key;
      let key: InK = key.into();
      map.push(key);
    }

    Ok(map)
  }
}

mod btree_set {
  use serde::*;
  use serde::ser::*;
  use std::collections::BTreeSet;

  use super::*;

  pub fn deserialize<'de, D, InK, OutK>(deserializer: D) -> Result<BTreeSet<InK>, D::Error>
    where D: Deserializer<'de>,
          InK: Ord,
          OutK: Deserialize<'de> + Into<InK>,
  {
    let visitor: BTreeSetDeVisitor<InK, OutK> = Default::default();
    let r = deserializer.deserialize_seq(visitor)?;
    Ok(r)
  }
  pub fn serialize<S, InK, OutK>(this: &BTreeSet<InK>, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer,
          InK: Clone + Into<OutK> + Ord,
          OutK: Serialize,
  {
    let mut map = serializer.serialize_seq(Some(this.len()))?;
    for k in this.iter() {
      let k: OutK = k.clone().into();
      map.serialize_element(&k)?;
    }
    map.end()
  }
}
mod btree_map {
  use serde::*;
  use serde::ser::*;
  use std::collections::BTreeMap;

  use super::*;

  pub fn deserialize<'de, D, InK, InV, OutK, OutV>(deserializer: D)
    -> Result<BTreeMap<InK, InV>, D::Error>
    where D: Deserializer<'de>,
          InK: Ord,
          OutK: Deserialize<'de> + Into<InK>,
          OutV: Deserialize<'de> + Into<InV>,
  {
    let visitor: BTreeMapDeVisitor<InK, InV, OutK, OutV> = Default::default();
    let r = deserializer.deserialize_map(visitor)?;
    Ok(r)
  }
  pub fn serialize<S, InK, InV, OutK, OutV>(this: &BTreeMap<InK, InV>, serializer: S)
    -> Result<S::Ok, S::Error>
    where S: Serializer,
          InK: Clone + Into<OutK> + Ord,
          InV: Clone + Into<OutV>,
          OutK: Serialize,
          OutV: Serialize,
  {
    let mut map = serializer.serialize_map(Some(this.len()))?;
    for (k, v) in this.iter() {
      let k: OutK = k.clone().into();
      let v: OutV = v.clone().into();
      map.serialize_entry(&k, &v)?;
    }
    map.end()
  }
}
mod vec {
  use serde::*;
  use serde::ser::*;

  use super::*;

  pub fn deserialize<'de, D, InK, OutK>(deserializer: D) -> Result<Vec<InK>, D::Error>
    where D: Deserializer<'de>,
          OutK: Deserialize<'de> + Into<InK>,
  {
    let visitor: VecDeVisitor<InK, OutK> = Default::default();
    let r = deserializer.deserialize_seq(visitor)?;
    Ok(r)
  }
  pub fn serialize<S, InK, OutK>(this: &Vec<InK>, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer,
          InK: Clone + Into<OutK>,
          OutK: Serialize,
  {
    let mut map = serializer.serialize_seq(Some(this.len()))?;
    for k in this.iter() {
      let k: OutK = k.clone().into();
      map.serialize_element(&k)?;
    }
    map.end()
  }
}
mod option {
  use serde::*;

  pub fn deserialize<'de, D, InK, OutK>(deserializer: D) -> Result<Option<InK>, D::Error>
    where D: Deserializer<'de>,
          OutK: Deserialize<'de> + Into<InK>,
  {
    let i = <Option<OutK>>::deserialize(deserializer)?
      .map(Into::into);
    Ok(i)
  }
  pub fn serialize<S, InK, OutK>(this: &Option<InK>, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer,
          InK: Clone + Into<OutK>,
          OutK: Serialize,
  {
    this.clone().map(Into::into)
      .serialize(serializer)
  }
}
