//! The "full" Legionella compile time driver.
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

#[macro_use]
extern crate rustc;
extern crate rustc_ast_borrowck;
extern crate rustc_codegen_ssa;
extern crate rustc_codegen_utils;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_errors as errors;
extern crate rustc_incremental;
extern crate rustc_interface;
extern crate rustc_lint;
extern crate rustc_metadata;
extern crate rustc_mir;
extern crate rustc_passes;
extern crate rustc_plugin;
extern crate rustc_privacy;
extern crate rustc_resolve;
extern crate rustc_save_analysis;
extern crate rustc_target;
extern crate rustc_traits;
extern crate rustc_typeck;
extern crate serialize as rustc_serialize;
extern crate syntax;
extern crate syntax_ext;
extern crate syntax_pos;
extern crate tempfile;
#[macro_use]
extern crate log;
#[cfg(unix)]
extern crate libc;

extern crate legionella_shared_defs as shared_defs;

use std::cell::Cell;
use std::fmt;
use std::mem::{transmute, size_of, };

use self::rustc::hir::def_id::{DefId, };
use self::rustc::middle::lang_items::{self, LangItem, };
use self::rustc::mir::{Constant, Operand, Rvalue, Statement,
                       StatementKind, Local, };
use self::rustc::mir::interpret::{ConstValue, Scalar, Allocation,
                                  PointerArithmetic, Pointer, };
use self::rustc::mir::{self, CustomIntrinsicMirGen, AggregateKind,
                       LocalDecl, Place, };
use self::rustc::session::{Session, early_error, };
use self::rustc::session::config::{ErrorOutputType, };
use self::rustc::ty::{self, TyCtxt, layout::Align, layout::Size, Instance, };
use self::rustc::ty::{Const, };
use self::rustc_data_structures::fx::{FxHashMap, };
use self::rustc_data_structures::sync::{Lrc, };
use self::rustc_data_structures::indexed_vec::*;
use crate::rustc_serialize::Encodable;
use self::syntax::ast;
use crate::syntax::feature_gate::{AttributeType, };
use self::syntax_pos::{Span, DUMMY_SP, };
use self::syntax_pos::symbol::{Symbol, };

use crate::codec::LegionellaEncoder;
use crate::driver::{report_ices_to_stderr_if_any,
                    EXIT_SUCCESS, EXIT_FAILURE,
                    DefaultCallbacks, };

use crate::help::*;

use self::shared_defs::platform::*;

pub use crate::rustc_driver::pretty;

pub mod codec;
pub mod driver;
pub mod help;
pub mod interface;
pub mod passes;
pub mod proc_macro_decls;
pub mod profile;
pub mod queries;
pub mod util;

pub fn main<F>(f: F)
  where F: FnOnce(&mut Generators),
{
  driver::init_rustc_env_logger();

  let result = report_ices_to_stderr_if_any(move || {
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
    assert!(generators.intrinsics.insert(k, i).is_none());
    let i = Lrc::new(WorkItemKill);
    let k = format!("{}", i);
    assert!(generators.intrinsics.insert(k, i).is_none());

    f(&mut generators);
    {
      unsafe {
        GENERATORS = Some(transmute(&generators));
      }

      let result = driver::run_compiler(&args, &mut DefaultCallbacks,
                                        None, None);

      unsafe {
        GENERATORS.take();
      }

      result
    }
  });

  ::std::process::exit(match result {
    Ok(Ok(_)) => EXIT_SUCCESS,
    Err(_) | Ok(Err(_)) => EXIT_FAILURE,
  });
}

pub struct Generators {
  /// technically unsafe, but *should* only be set from one
  /// thread in practice.
  cstore: Cell<Option<&'static rustc_metadata::cstore::CStore>>,

  kernel_instance: Lrc<dyn CustomIntrinsicMirGen>,
  kernel_context_data_id: Lrc<dyn CustomIntrinsicMirGen>,
  /// no `InternedString` here: the required thread local vars won't
  /// be initialized
  pub intrinsics: FxHashMap<String, Lrc<dyn CustomIntrinsicMirGen>>,
}
impl Generators {
  pub fn cstore(&self) -> &'static rustc_metadata::cstore::CStore {
    self.cstore
      .get()
      .expect("requesting the cstore, but CompilerCalls::late_callback has been called yet")
  }
}
impl Default for Generators {
  fn default() -> Self {
    Generators {
      cstore: Cell::new(None),
      kernel_instance: Lrc::new(KernelInstance) as Lrc<_>,
      kernel_context_data_id: Lrc::new(KernelContextDataId) as Lrc<_>,
      intrinsics: Default::default(),
    }
  }
}
static mut GENERATORS: Option<&'static Generators> = None;
pub fn generators() -> &'static Generators {
  unsafe {
    GENERATORS.expect("Legionella intrinsic generators isn't in scope!")
  }
}

fn whitelist_legionella_attr(sess: &Session) {
  let mut plugin_attributes = sess.plugin_attributes
    .borrow_mut();

  plugin_attributes
    .push((Symbol::intern("legionella"),
           AttributeType::Whitelisted));
  plugin_attributes
    .push((Symbol::intern("legionella_attr"),
           AttributeType::Whitelisted));
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
      tcx.types.u64,
      tcx.types.u64,
      tcx.types.u64,
      tcx.mk_imm_ref(tcx.lifetimes.re_static,
                     tcx.mk_slice(tcx.types.u8))
    ].into_iter())
  }
}

impl CustomIntrinsicMirGen for KernelInstance {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::Body<'tcx>)
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

    let ret = mir::Place::RETURN_PLACE.clone();
    let local = Local::new(1);
    let local_ty = mir.local_decls[local].ty;

    let instance = extract_opt_fn_instance(tcx, instance, local_ty);

    let slice = build_compiler_opt(tcx, instance, |tcx, instance| {
      let def_id = instance.def_id();
      let crate_name = tcx.crate_name(def_id.krate);
      let disambiguator = tcx.crate_disambiguator(def_id.krate);
      let (d_hi, d_lo) = disambiguator.to_fingerprint().as_value();

      let crate_name = static_str_const_value(tcx, &*crate_name.as_str());
      let d_hi = tcx.mk_u64_cv(d_hi);
      let d_lo = tcx.mk_u64_cv(d_lo);
      let id = tcx.mk_u64_cv(def_id.index.as_usize() as u64);

      let substs = LegionellaEncoder::with(tcx, |encoder| {
        instance.substs.encode(encoder)
          .expect("actual encode kernel substs");

        Ok(())
      })
        .expect("encode kernel substs");
      let substs_len = substs.len();
      let alloc = Allocation::from_byte_aligned_bytes(substs);
      let alloc = tcx.intern_const_alloc(alloc);
      tcx.alloc_map.lock().create_memory_alloc(alloc);
      let substs = ConstValue::Slice {
        data: alloc,
        start: 0,
        end: substs_len,
      };

      static_tuple_const_value(tcx, "kernel_id_for",
                               vec![crate_name, d_hi, d_lo, id,
                               substs].into_iter(),
                               self.inner_ret_ty(tcx))
    });
    let rvalue = const_value_rvalue(tcx, slice,
                                    self.output(tcx));

    let stmt_kind = StatementKind::Assign(ret, Box::new(rvalue));
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
    let p = Symbol::intern(&format!("P{}", n)).as_interned_str();
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
                                   mir: &mut mir::Body<'tcx>) {
    let ptr_size = tcx.pointer_size();
    let data = vec![0; ptr_size.bytes() as usize];
    let align = Align::from_bits(64).unwrap(); // XXX arch dependent.
    let mut alloc = Allocation::from_bytes(&data[..], align);
    alloc.mutability = ast::Mutability::Mutable;
    let alloc = tcx.intern_const_alloc(alloc);
    let alloc_id = tcx.alloc_map.lock().create_memory_alloc(alloc);

    let ret = mir::Place::RETURN_PLACE.clone();

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
      val: const_val,
    });
    let constant = Constant {
      span: source_info.span,
      ty: self.output(tcx),
      literal: constant,
      user_ty: None,
    };
    let constant = Box::new(constant);
    let constant = Operand::Constant(constant);

    let rvalue = Rvalue::Use(constant);

    let stmt_kind = StatementKind::Assign(ret, Box::new(rvalue));
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
    let p = Symbol::intern(&format!("P{}", n)).as_interned_str();
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

/// This intrinsic has to be manually inserted by the drivers
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct CurrentPlatform(pub Platform);
impl CurrentPlatform {
  pub const fn host_platform() -> Self { CurrentPlatform(host_platform()) }

  fn data(self) -> [u8; size_of::<Platform>()] {
    unsafe {
      transmute(self.0)
    }
  }
}
impl fmt::Display for CurrentPlatform {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__legionella_current_platform")
  }
}
impl CustomIntrinsicMirGen for CurrentPlatform {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   mir: &mut mir::Body<'tcx>) {
    let align = Align::from_bits(64).unwrap(); // XXX arch dependent.
    let data = &self.data()[..];
    let alloc = Allocation::from_bytes(data, align);
    let alloc = tcx.intern_const_alloc(alloc);
    let alloc_id = tcx.alloc_map.lock()
      .create_memory_alloc(alloc);

    let ret = mir::Place::RETURN_PLACE.clone();

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
    let const_val = ConstValue::Scalar(ptr.into());
    let constant = tcx.mk_const_op(source_info.clone(), Const {
      ty: self.output(tcx),
      val: const_val,
    });
    let rvalue = Rvalue::Use(constant);

    let stmt_kind = StatementKind::Assign(ret, Box::new(rvalue));
    let stmt = Statement {
      source_info: source_info.clone(),
      kind: stmt_kind,
    };
    bb.statements.push(stmt);
    mir.basic_blocks_mut().push(bb);
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    0
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>> {
    tcx.intern_type_list(&[])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    let arr = tcx.mk_array(tcx.types.u8, size_of::<Platform>() as _);
    tcx.mk_imm_ref(tcx.lifetimes.re_static, arr)
  }
}

/// Kill (ie `abort()`) the current workitem/thread only. This one is
/// for the bootstrap driver.
pub struct WorkItemKill;
impl CustomIntrinsicMirGen for WorkItemKill {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   mir: &mut mir::Body<'tcx>)
  {
    info!("mirgen intrinsic {}", self);
    // this always redirects to a panic here (this impl is only used on
    // the host).

    pub fn langcall(tcx: TyCtxt,
                    span: Option<Span>,
                    msg: &str,
                    li: LangItem)
      -> DefId
    {
      tcx.lang_items().require(li).unwrap_or_else(|s| {
        let msg = format!("{} {}", msg, s);
        match span {
          Some(span) => tcx.sess.span_fatal(span, &msg[..]),
          None => tcx.sess.fatal(&msg[..]),
        }
      })
    }

    fn static_str_operand<T>(tcx: TyCtxt,
                             source_info: mir::SourceInfo,
                             str: T) -> Operand
      where T: fmt::Display,
    {
      let str = format!("{}", str);
      let alloc = Allocation::from_byte_aligned_bytes(str.as_bytes());
      let v = ConstValue::Slice {
        data: tcx.intern_const_alloc(alloc),
        start: 0,
        end: str.len(),
      };
      let v = tcx.mk_const(Const {
        ty: tcx.mk_static_str(),
        val: v,
      });
      let v = Constant {
        span: source_info.span,
        ty: tcx.mk_static_str(),
        literal: v,
        user_ty: None,
      };
      let v = Box::new(v);
      Operand::Constant(v)
    }

    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let mk_u32 = |v: u32| {
      let v = Scalar::from_uint(v, Size::from_bytes(4));
      let v = ConstValue::Scalar(v);
      let v = tcx.mk_const(Const {
        ty: tcx.types.u32,
        val: v,
      });
      let v = Constant {
        span: source_info.span,
        ty: tcx.types.u32,
        literal: v,
        user_ty: None,
      };
      let v = Box::new(v);
      Operand::Constant(v)
    };

    let mut bb = mir::BasicBlockData {
      statements: Vec::new(),
      terminator: Some(mir::Terminator {
        source_info: source_info.clone(),
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };

    let (real_instance, args, term_kind) = {
      // call `panic` from `libcore`
      // `fn panic(expr_file_line_col: &(&'static str, &'static str, u32, u32)) -> !`
      let lang_item = lang_items::PanicFnLangItem;

      let expr = static_str_operand(tcx, source_info.clone(),
                                    "__legionella_kill called");
      let file = static_str_operand(tcx, source_info.clone(),
                                    "TODO panic file");
      let line = mk_u32(0); // TODO
      let col = mk_u32(0); // TODO
      let rvalue = Rvalue::Aggregate(Box::new(AggregateKind::Tuple),
                                     vec![expr, file, line, col]);
      let arg_ty = tcx.intern_tup(&[
        tcx.mk_static_str(),
        tcx.mk_static_str(),
        tcx.types.u32,
        tcx.types.u32,
      ]);
      let arg_local = LocalDecl::new_temp(arg_ty, DUMMY_SP);
      let arg_local_id = Place::from(mir.local_decls.next_index());
      mir.local_decls.push(arg_local);
      let stmt_kind = StatementKind::Assign(arg_local_id.clone(),
                                            Box::new(rvalue));
      let stmt = Statement {
        source_info: source_info.clone(),
        kind: stmt_kind,
      };
      bb.statements.push(stmt);

      let arg_ref_ty = tcx.mk_imm_ref(tcx.lifetimes.re_erased, arg_ty);
      let arg_ref_local = LocalDecl::new_temp(arg_ref_ty, DUMMY_SP);
      let arg_ref_local_id = Place::from(mir.local_decls.next_index());
      mir.local_decls.push(arg_ref_local);
      let rvalue = Rvalue::Ref(tcx.lifetimes.re_erased,
                               mir::BorrowKind::Shared,
                               arg_local_id);
      let stmt_kind = StatementKind::Assign(arg_ref_local_id.clone(),
                                            Box::new(rvalue));
      let stmt = Statement {
        source_info: source_info.clone(),
        kind: stmt_kind,
      };
      bb.statements.push(stmt);

      let def_id = langcall(tcx, None, "", lang_item);
      let instance = Instance::mono(tcx, def_id);

      (instance,
       vec![Operand::Copy(arg_ref_local_id), ],
       mir::TerminatorKind::Unreachable)
    };
    debug!("mirgen intrinsic into {}", real_instance);
    let success = mir::BasicBlock::new(mir.basic_blocks().next_index().index() + 1);
    let fn_ty = real_instance.ty(tcx);
    bb.terminator.as_mut()
      .unwrap()
      .kind = mir::TerminatorKind::Call {
      func: tcx.mk_const_op(source_info.clone(),
                            *ty::Const::zero_sized(tcx, fn_ty)),
      args,
      destination: Some((Place::RETURN_PLACE.clone(), success)),
      cleanup: None,
      from_hir_call: false,
    };
    mir.basic_blocks_mut().push(bb);
    let bb = mir::BasicBlockData {
      statements: Vec::new(),
      terminator: Some(mir::Terminator {
        source_info: source_info.clone(),
        kind: term_kind,
      }),

      is_cleanup: false,
    };
    mir.basic_blocks_mut().push(bb);
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
    write!(f, "__legionella_kill")
  }
}
