//! The "full" Geobacter compile time driver.
//!
//! A lot of code here is pretty much just copied from the Rust compiler source.
//! We have to modify the driver process in a way which isn't exposed at all in
//! the provided interface, but otherwise want the exact same behaviour as
//! upstream.

#![feature(rustc_private)]
#![feature(generators, generator_trait)]
#![feature(never_type)]
#![feature(specialization)]

#![recursion_limit="256"]

extern crate rustc;
extern crate rustc_codegen_ssa;
extern crate rustc_codegen_utils;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_errors as errors;
extern crate rustc_feature;
extern crate rustc_hir;
extern crate rustc_incremental;
extern crate rustc_index;
extern crate rustc_interface;
extern crate rustc_lint;
extern crate rustc_metadata;
extern crate rustc_mir;
extern crate rustc_passes;
extern crate rustc_privacy;
extern crate rustc_resolve;
extern crate rustc_save_analysis;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_target;
extern crate rustc_traits;
extern crate rustc_typeck;
extern crate serialize as rustc_serialize;
extern crate syntax;
extern crate tempfile;
#[macro_use]
extern crate log;
#[cfg(unix)]
extern crate libc;
extern crate lazy_static;

extern crate geobacter_shared_defs as shared_defs;
extern crate geobacter_rustc_help as rustc_help;

use std::fmt;
use std::mem::{transmute, };
use std::time::Instant;

use self::rustc::mir::{Constant, Operand, Rvalue, Statement,
                       StatementKind, Local, };
use self::rustc::mir::interpret::{ConstValue, Scalar, Allocation,
                                  PointerArithmetic, Pointer, };
use self::rustc::mir::{self, CustomIntrinsicMirGen, };
use rustc::ty::{self, TyCtxt, layout::Align, Const, };
use self::rustc_data_structures::fx::{FxHashMap, };
use self::rustc_data_structures::sync::{Lrc, };
use rustc_data_structures::profiling::print_time_passes_entry;
use rustc_driver::{Callbacks, init_rustc_env_logger, install_ice_hook,
                   catch_fatal_errors, run_compiler, EXIT_SUCCESS,
                   EXIT_FAILURE, };
use rustc_hir::def_id::{DefId, };
use rustc_interface::interface::Config;
use crate::rustc_index::vec::*;
use crate::rustc_serialize::Encodable;
use rustc_session::{Session, early_error, };
use rustc_session::config::{ErrorOutputType, };
use self::syntax::ast;
use rustc_span::{DUMMY_SP, };
use rustc_span::symbol::{Symbol, };

use crate::rustc_help::codec::GeobacterEncoder;
use rustc_help::driver_data::DriverData;
use rustc_help::intrinsics::CurrentPlatform;

use crate::rustc_help::*;

pub use crate::rustc_driver::pretty;
pub use crate::rustc_driver::plugin as rustc_plugin;

pub fn main<F>(f: F)
  where F: FnOnce(&mut Generators),
{
  let start = Instant::now();
  init_rustc_env_logger();
  let mut callbacks = GeobacterDriverCallbacks::default();
  install_ice_hook();

  let result = catch_fatal_errors(|| {
    let mut args: Vec<_> = ::std::env::args_os()
      .enumerate()
      .filter_map(|(idx, arg)| {
        match (idx, arg.to_str()) {
          (1, Some("rustc")) => None,
          _ => Some((idx, arg)),
        }
      })
      .map(|(i, arg)| arg.into_string().unwrap_or_else(|arg| {
        early_error(ErrorOutputType::default(),
                    &format!("Argument {} is not valid Unicode: {:?}", i, arg))
      }))
      .collect();

    // force MIR to be encoded:
    args.push("-Z".into());
    args.push("always-encode-mir".into());
    args.push("-Z".into());
    args.push("always-emit-metadata".into());
    args.push("-Z".into());
    args.push("mir-opt-level=0".into());

    let mut generators = Generators::default();

    let i = Lrc::new(CurrentPlatform::host_platform());
    let k = format!("{}", i);
    assert!(generators.intrinsics.insert(k, i as Lrc<_>).is_none());
    let i = Lrc::new(WorkItemKill);
    let k = format!("{}", i);
    assert!(generators.intrinsics.insert(k, i as Lrc<_>).is_none());

    f(&mut generators);
    {
      unsafe {
        GENERATORS = Some(transmute(&generators));
      }

      let result = run_compiler(&args, &mut callbacks,
                                None, None);

      unsafe {
        GENERATORS.take();
      }

      result
    }
  }).and_then(|result| result);

  let exit_code = match result {
    Ok(_) => EXIT_SUCCESS,
    Err(_) => EXIT_FAILURE,
  };

  // The extra `\t` is necessary to align this label with the others.
  print_time_passes_entry(callbacks.time_passes,
                          "\ttotal", start.elapsed());
  ::std::process::exit(exit_code);
}

#[derive(Default)]
pub struct GeobacterDriverCallbacks {
  time_passes: bool,
}
impl Callbacks for GeobacterDriverCallbacks {
  fn config(&mut self, config: &mut Config) {
    // If a --prints=... option has been given, we don't print the "total"
    // time because it will mess up the --prints output. See #64339.
    self.time_passes = config.opts.prints.is_empty()
      && (config.opts.debugging_opts.time_passes || config.opts.debugging_opts.time);

    config.override_queries = Some(override_queries);
  }
}

pub struct Generators {
  kernel_instance: Lrc<dyn CustomIntrinsicMirGen>,
  kernel_context_data_id: Lrc<dyn CustomIntrinsicMirGen>,
  specialization_param: Lrc<dyn CustomIntrinsicMirGen>,
  call_by_type: Lrc<dyn CustomIntrinsicMirGen>,
  /// no `InternedString` here: the required thread local vars won't
  /// be initialized
  pub intrinsics: FxHashMap<String, Lrc<dyn CustomIntrinsicMirGen>>,
}
impl Generators { }
impl Default for Generators {
  fn default() -> Self {
    use rustc_help::intrinsics::*;

    let spec_param: GeobacterMirGen<SpecializationParam, Generators> =
      Default::default();
    let call_by_type: GeobacterMirGen<CallByType, Generators> =
      Default::default();

    Generators {
      kernel_instance: Lrc::new(KernelInstance) as Lrc<_>,
      kernel_context_data_id: Lrc::new(KernelContextDataId) as Lrc<_>,
      specialization_param: Lrc::new(spec_param),
      call_by_type: Lrc::new(call_by_type),
      intrinsics: Default::default(),
    }
  }
}
impl rustc_help::driver_data::DriverData for Generators { }
impl rustc_help::driver_data::GetDriverData for Generators {
  fn with_self<'tcx, F, R>(_tcx: TyCtxt<'tcx>, f: F) -> R
    where F: FnOnce(&dyn DriverData) -> R,
  {
    f(generators())
  }
}
static mut GENERATORS: Option<&'static Generators> = None;
pub fn generators() -> &'static Generators {
  unsafe {
    GENERATORS.expect("Geobacter intrinsic generators isn't in scope!")
  }
}

fn override_queries(_sess: &Session, local: &mut ty::query::Providers<'_>,
                    remote: &mut ty::query::Providers<'_>)
{
  local.custom_intrinsic_mirgen = custom_intrinsic_mirgen;
  remote.custom_intrinsic_mirgen = custom_intrinsic_mirgen;
}

fn custom_intrinsic_mirgen(tcx: TyCtxt<'_>, def_id: DefId)
  -> Option<Lrc<dyn CustomIntrinsicMirGen>>
{
  let name = tcx.item_name(def_id);
  let name_str = name.as_str();
  info!("custom_intrinsic_mirgen: {}", name);

  let gen = generators();

  match &name_str[..] {
    "kernel_instance" => {
      Some(gen.kernel_instance.clone())
    },
    "kernel_context_data_id" => {
      Some(gen.kernel_context_data_id.clone())
    },
    "__geobacter_specialization_param" => {
      Some(gen.specialization_param.clone())
    },
    "__geobacter_call_by_type" => {
      Some(gen.call_by_type.clone())
    },
    _ => {
      gen.intrinsics
        .get(&name_str[..])
        .cloned()
    },
  }
}

struct KernelInstance;

impl KernelInstance {
  fn inner_ret_ty<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    tcx.mk_tup([
      tcx.mk_static_str(),
      tcx.mk_imm_ref(tcx.lifetimes.re_static,
                     tcx.mk_slice(tcx.types.u8))
    ].iter())
  }
}

impl CustomIntrinsicMirGen for KernelInstance {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>)
  {
    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let mut bb = mir::BasicBlockData {
      statements: Vec::new(),
      terminator: Some(mir::Terminator {
        source_info: source_info.clone(),
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };

    let ret = mir::Place::return_place();
    let local = Local::new(1);
    let local_ty = mir.local_decls[local].ty;

    let instance = extract_opt_fn_instance(tcx, instance, local_ty);

    let slice = build_compiler_opt(tcx, instance, |tcx, instance| {
      let name = tcx.def_path_str(instance.def_id());
      let name = static_str_const_value(tcx, &*name.as_str());

      let instance = GeobacterEncoder::with(tcx, |encoder| {
        instance.encode(encoder).expect("actual encode kernel instance");
        Ok(())
      }).expect("encode kernel instance");

      let instance_len = instance.len();
      let alloc = Allocation::from_byte_aligned_bytes(instance);
      let alloc = tcx.intern_const_alloc(alloc);
      tcx.alloc_map.lock().create_memory_alloc(alloc);
      let instance = ConstValue::Slice {
        data: alloc,
        start: 0,
        end: instance_len,
      };

      static_tuple_const_value(tcx, "kernel_instance",
                               vec![name, instance].into_iter(),
                               self.inner_ret_ty(tcx))
    });
    let rvalue = const_value_rvalue(tcx, slice,
                                    self.output(tcx));

    let stmt_kind = StatementKind::Assign(Box::new((ret, rvalue)));
    let stmt = Statement {
      source_info: source_info.clone(),
      kind: stmt_kind,
    };
    bb.statements.push(stmt);
    mir.basic_blocks_mut().push(bb);
  }

  fn generic_parameter_count<'tcx>(&self, _tcx: TyCtxt<'tcx>) -> usize {
    3
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>> {
    let n = 0;
    let p = Symbol::intern(&format!("P{}", n));
    let f = tcx.mk_ty_param(n, p);
    let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                               ty::BrAnon(0)));
    let t = tcx.mk_imm_ref(region, f);
    tcx.intern_type_list(&[t])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    return mk_static_slice(tcx, self.inner_ret_ty(tcx));
  }
}
/// creates a static variable which can be used (atomically!) to store
/// an ID for a function.
struct KernelContextDataId;
impl CustomIntrinsicMirGen for KernelContextDataId {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>) {
    let ptr_size = tcx.pointer_size();
    let data = vec![0; ptr_size.bytes() as usize];
    let align = Align::from_bits(64).unwrap(); // XXX arch dependent.
    let mut alloc = Allocation::from_bytes(&data[..], align);
    alloc.mutability = ast::Mutability::Mut;
    let alloc = tcx.intern_const_alloc(alloc);
    let alloc_id = tcx.alloc_map.lock().create_memory_alloc(alloc);

    let ret = mir::Place::return_place();

    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let mut bb = mir::BasicBlockData {
      statements: Vec::new(),
      terminator: Some(mir::Terminator {
        source_info: source_info.clone(),
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };

    let ptr = Pointer::from(alloc_id);
    let scalar = Scalar::Ptr(ptr);
    let const_val = ConstValue::Scalar(scalar);
    let constant = tcx.mk_const(Const {
      ty: self.output(tcx),
      val: ty::ConstKind::Value(const_val),
    });
    let constant = Constant {
      span: source_info.span,
      literal: constant,
      user_ty: None,
    };
    let constant = Box::new(constant);
    let constant = Operand::Constant(constant);

    let rvalue = Rvalue::Use(constant);

    let stmt_kind = StatementKind::Assign(Box::new((ret, rvalue)));
    let stmt = Statement {
      source_info: source_info.clone(),
      kind: stmt_kind,
    };
    bb.statements.push(stmt);
    mir.basic_blocks_mut().push(bb);
  }

  fn generic_parameter_count<'tcx>(&self, _tcx: TyCtxt<'tcx>) -> usize {
    3
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>> {
    let n = 0;
    let p = Symbol::intern(&format!("P{}", n));
    let f = tcx.mk_ty_param(n, p);
    let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                               ty::BrAnon(0)));
    let t = tcx.mk_imm_ref(region, f);
    tcx.intern_type_list(&[t])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    tcx.mk_imm_ref(tcx.lifetimes.re_static, tcx.types.usize)
  }
}

/// Kill (ie `panic!()`) the current workitem/thread only. This one is
/// for the bootstrap driver.
pub struct WorkItemKill;
impl CustomIntrinsicMirGen for WorkItemKill {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>)
  {
    trace!("mirgen intrinsic {}", self);
    // this always redirects to a panic here (this impl is only used on
    // the host).
    redirect_or_panic(tcx, mir, "Host workitem kill called", || None );
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    0
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    tcx.intern_type_list(&[])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    tcx.types.never
  }
}

impl fmt::Display for WorkItemKill {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_kill")
  }
}
