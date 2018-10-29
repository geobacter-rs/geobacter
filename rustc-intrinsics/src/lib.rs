#![feature(rustc_private)]

extern crate rustc;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_metadata;
extern crate rustc_mir;
extern crate rustc_codegen_utils;
extern crate rustc_data_structures;
extern crate syntax;
extern crate syntax_pos;
#[macro_use]
extern crate log;
extern crate env_logger;

use std::cell::Cell;
use std::path::PathBuf;

use self::rustc_driver::{driver, Compilation, CompilerCalls, RustcDefaultCalls, };
use self::rustc::hir::def_id::{DefId, };
use self::rustc::mir::{Constant, Operand, Rvalue, Statement,
                 StatementKind, AggregateKind, Local, };
use self::rustc::mir::interpret::{ConstValue, Scalar, Allocation,
                                  PointerArithmetic};
use self::rustc::mir::{self, CustomIntrinsicMirGen, RETURN_PLACE, };
use self::rustc::session::{config, Session, };
use self::rustc::session::config::{ErrorOutputType, Input, };
use self::rustc::ty::{self, TyCtxt, Instance, layout::Size, layout::Align, };
use self::rustc::ty::query::Providers;
use self::rustc::ty::{Const, Closure, ParamEnv, Ref, FnDef, };
use self::rustc_codegen_utils::codegen_backend::CodegenBackend;
use self::rustc_data_structures::fx::{FxHashMap, };
use self::rustc_data_structures::sync::{Lrc, };
use self::rustc_data_structures::indexed_vec::*;
use self::syntax::ast;
use self::syntax_pos::DUMMY_SP;
use self::syntax_pos::symbol::{Symbol, };

use self::rustc_driver::getopts;

pub fn main<F>(f: F)
  where F: FnOnce(&mut Generators),
{
  use std::mem::transmute;

  rustc_driver::env_logger::init();
  env_logger::init();

  let exit;
  {
    let mut args: Vec<_> = ::std::env::args()
      .enumerate()
      .filter_map(|(idx, arg)| {
        match (idx, arg.as_str()) {
          (1, "rustc") => None,
          _ => Some(arg),
        }
      }).collect();

    // force MIR to be encoded:
    args.push("-Z".into());
    args.push("always-encode-mir".into());
    args.push("-Z".into());
    args.push("always-emit-metadata".into());

    let mut generators = Generators::default();
    f(&mut generators);
    {
      unsafe {
        GENERATORS = Some(transmute(&generators));
      }

      let driver = HsaRustcDriver::new();
      let driver = Box::new(driver);
      exit = rustc_driver::run(move || {
        rustc_driver::run_compiler(&args, driver, None, None)
      });

      unsafe {
        GENERATORS.take();
      }
    }
  }

  ::std::process::exit(exit as _);
}

pub struct Generators {
  /// technically unsafe, but *should* only be set from one
  /// thread in practice.
  cstore: Cell<Option<&'static rustc_metadata::cstore::CStore>>,

  kernel_id_for: Lrc<dyn CustomIntrinsicMirGen>,
  kernel_context_data_id: Lrc<dyn CustomIntrinsicMirGen>,
  kernel_upvars_for: Lrc<dyn CustomIntrinsicMirGen>,
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
      kernel_id_for: Lrc::new(KernelIdFor) as Lrc<_>,
      kernel_context_data_id: Lrc::new(KernelContextDataId) as Lrc<_>,
      kernel_upvars_for: Lrc::new(KernelUpvars) as Lrc<_>,
      intrinsics: Default::default(),
    }
  }
}
static mut GENERATORS: Option<&'static Generators> = None;
pub fn generators() -> &'static Generators {
  unsafe {
    GENERATORS.unwrap()
  }
}

pub struct HsaRustcDriver {
  default: Box<RustcDefaultCalls>,
}
impl HsaRustcDriver {
  pub fn new() -> Self {
    HsaRustcDriver {
      default: Box::new(RustcDefaultCalls),
    }
  }
}

impl<'a> CompilerCalls<'a> for HsaRustcDriver {
  fn early_callback(
    &mut self,
    matches: &getopts::Matches,
    sopts: &config::Options,
    cfg: &ast::CrateConfig,
    descriptions: &rustc_errors::registry::Registry,
    output: ErrorOutputType,
  ) -> Compilation {
    self.default
      .early_callback(matches, sopts, cfg, descriptions, output)
  }
  fn no_input(
    &mut self,
    matches: &getopts::Matches,
    sopts: &config::Options,
    cfg: &ast::CrateConfig,
    odir: &Option<PathBuf>,
    ofile: &Option<PathBuf>,
    descriptions: &rustc_errors::registry::Registry,
  ) -> Option<(Input, Option<PathBuf>)> {
    self.default
      .no_input(matches, sopts, cfg, odir, ofile, descriptions)
  }
  fn late_callback(
    &mut self,
    trans_crate: &dyn CodegenBackend,
    matches: &getopts::Matches,
    sess: &Session,
    crate_stores: &rustc_metadata::cstore::CStore,
    input: &Input,
    odir: &Option<PathBuf>,
    ofile: &Option<PathBuf>,
  ) -> Compilation {
    let gen = generators();
    assert!(gen.cstore.get().is_none());
    gen.cstore.set(Some(unsafe {
      ::std::mem::transmute(crate_stores)
    }));

    self.default
      .late_callback(trans_crate, matches, sess, crate_stores, input, odir, ofile)
  }
  fn build_controller(self: Box<Self>, sess: &Session,
                      matches: &getopts::Matches)
    -> driver::CompileController<'a>
  {
    let mut controller = self.default.build_controller(sess, matches);

    let old_provide = std::mem::replace(&mut controller.provide,
                                        Box::new(|_| {}));
    controller.provide = Box::new(move |providers| {
      old_provide(providers);

      *providers = Providers {
        custom_intrinsic_mirgen,
        ..*providers
      };
    });

    controller
  }
}

fn custom_intrinsic_mirgen<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                     def_id: DefId)
  -> Option<Lrc<dyn CustomIntrinsicMirGen>>
{
  let name = tcx.item_name(def_id);
  let name_str = name.as_str();
  info!("custom_intrinsic_mirgen: {}", name);

  let gen = generators();

  match &name_str[..] {
    "kernel_id_for" => {
      Some(gen.kernel_id_for.clone())
    },
    "kernel_context_data_id" => {
      Some(gen.kernel_context_data_id.clone())
    },
    "kernel_env_for" => {
      Some(gen.kernel_upvars_for.clone())
    },
    _ => {
      gen.intrinsics
        .get(&name_str[..])
        .cloned()
    },
  }
}

struct KernelIdFor;
struct KernelUpvars;

impl CustomIntrinsicMirGen for KernelIdFor {
  fn mirgen_simple_intrinsic<'a, 'tcx>(&self,
                                       tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       instance: ty::Instance<'tcx>,
                                       mir: &mut mir::Mir<'tcx>)
    where 'tcx: 'a
  {
    let parent_param_substs = instance.substs;

    let reveal_all = ParamEnv::reveal_all();

    let ret = mir::Place::Local(RETURN_PLACE);
    let local = Local::new(1);
    let local_ty = mir.local_decls[local].ty;
    let local_ty = tcx
      .subst_and_normalize_erasing_regions(parent_param_substs,
                                           reveal_all,
                                           &local_ty);

    let instance = match local_ty.sty {
      Ref(_, &ty::TyS {
          sty: FnDef(def_id, subs),
          ..
        }, ..) |
      FnDef(def_id, subs) => {
        let subs = tcx
          .subst_and_normalize_erasing_regions(parent_param_substs,
                                               reveal_all,
                                               &subs);

        rustc::ty::Instance::resolve(tcx, reveal_all, def_id, subs)
          .expect("must be resolvable")
      },

      Ref(_, &ty::TyS {
          sty: Closure(def_id, subs),
          ..
        }, ..) |
      Closure(def_id, subs) => {
        let subs = tcx
          .subst_and_normalize_erasing_regions(parent_param_substs,
                                               reveal_all,
                                               &subs);

        let env = subs.closure_kind(def_id, tcx);
        Instance::resolve_closure(tcx, def_id, subs, env)
      },

      _ => {
        let msg = format!("can't expand the type of this item: {}",
                          tcx.item_path_str(instance.def_id()));
        tcx.sess.span_fatal(DUMMY_SP, &msg[..]);
      },
    };

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

    let def_id = instance.def_id();
    let ty = instance.ty(tcx);
    let sig = ty.fn_sig(tcx);
    let inputs = sig.inputs();
    for input in inputs.skip_binder().iter() {
      let msg = format!("input ty: {:?}", input);
      tcx.sess.note_without_error(&msg);
    }

    let crate_name = tcx.crate_name(def_id.krate);
    let crate_name = format!("{}", crate_name);
    let id = tcx.allocate_bytes(crate_name.as_bytes());
    let crate_name = ConstValue::new_slice(Scalar::Ptr(id.into()),
                                           crate_name.len() as u64,
                                           tcx);
    let crate_name = tcx.mk_const(Const {
      ty: tcx.mk_static_str(),
      val: crate_name,
    });
    let crate_name = Constant {
      span: source_info.span,
      ty: tcx.mk_static_str(),
      literal: crate_name,
      user_ty: None,
    };
    let crate_name = Box::new(crate_name);
    let crate_name = Operand::Constant(crate_name);

    let mk_u64 = |v: u64| {
      let v = Scalar::from_uint(v, Size::from_bytes(8));
      let v = ConstValue::Scalar(v);
      let v = tcx.mk_const(Const {
        ty: tcx.types.u64,
        val: v,
      });
      let v = Constant {
        span: source_info.span,
        ty: tcx.types.u64,
        literal: v,
        user_ty: None,
      };
      let v = Box::new(v);
      Operand::Constant(v)
    };

    let disambiguator = tcx.crate_disambiguator(def_id.krate);
    let (d_hi, d_lo) = disambiguator.to_fingerprint().as_value();
    let d_hi = mk_u64(d_hi);
    let d_lo = mk_u64(d_lo);

    let id = mk_u64(def_id.index.as_raw_u32() as u64);

    let rvalue = Rvalue::Aggregate(Box::new(AggregateKind::Tuple),
                                   vec![crate_name, d_hi, d_lo, id]);
    let stmt_kind = StatementKind::Assign(ret, Box::new(rvalue));
    let stmt = Statement {
      source_info: source_info.clone(),
      kind: stmt_kind,
    };
    bb.statements.push(stmt);
    mir.basic_blocks_mut().push(bb);
  }

  fn generic_parameter_count<'a, 'tcx>(&self, _tcx: TyCtxt<'a, 'tcx, 'tcx>)
    -> usize
  {
    3
  }
  /// The types of the input args.
  fn inputs<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    let n = 0;
    let p = Symbol::intern(&format!("P{}", n)).as_interned_str();
    let f = tcx.mk_ty_param(n, p);
    let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                               ty::BrAnon(0)));
    let t = tcx.mk_imm_ref(region, f);
    tcx.intern_type_list(&[t])
  }
  /// The return type.
  fn output<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>) -> ty::Ty<'tcx> {
    let inner = tcx.mk_tup([tcx.mk_static_str(),
                            tcx.types.u64,
                            tcx.types.u64,
                            tcx.types.u64,].into_iter());
    return inner;
  }
}
impl CustomIntrinsicMirGen for KernelUpvars {
  fn mirgen_simple_intrinsic<'a, 'tcx>(&self,
                                       _tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       _instance: ty::Instance<'tcx>,
                                       _mir: &mut mir::Mir<'tcx>)
    where 'tcx: 'a
  {
    unimplemented!();
  }

  fn generic_parameter_count<'a, 'tcx>(&self, _tcx: TyCtxt<'a, 'tcx, 'tcx>)
                                       -> usize
  {
    3
  }
  /// The types of the input args.
  fn inputs<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    let n = 0;
    let p = Symbol::intern(&format!("P{}", n)).as_interned_str();
    let f = tcx.mk_ty_param(n, p);
    let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                               ty::BrAnon(0)));
    let t = tcx.mk_imm_ref(region, f);
    tcx.intern_type_list(&[t])
  }
  /// The return type.
  fn output<'a, 'tcx>(&self, _tcx: TyCtxt<'a, 'tcx, 'tcx>)
    -> ty::Ty<'tcx>
  {
    unimplemented!();
  }
}
/// creates a static variable which can be used (atomically!) to store
/// an ID for a function.
struct KernelContextDataId;
impl CustomIntrinsicMirGen for KernelContextDataId {
  fn mirgen_simple_intrinsic<'a, 'tcx>(&self,
                                       tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       _instance: ty::Instance<'tcx>,
                                       mir: &mut mir::Mir<'tcx>)
    where 'tcx: 'a
  {
    let ptr_size = tcx.pointer_size();
    let data = vec![0; ptr_size.bytes() as usize];
    let align = Align::from_bits(64, 64).unwrap(); // XXX arch dependent.
    let mut alloc = Allocation::from_bytes(&data[..], align);
    alloc.mutability = ast::Mutability::Mutable;
    let alloc = tcx.intern_const_alloc(alloc);
    let alloc_id = tcx.alloc_map.lock().allocate(alloc);

    let ret = mir::Place::Local(RETURN_PLACE);

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

    let const_val = ConstValue::ByRef(alloc_id.into(), alloc, ptr_size);
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

  fn generic_parameter_count<'a, 'tcx>(&self, _tcx: TyCtxt<'a, 'tcx, 'tcx>)
                                       -> usize
  {
    3
  }
  /// The types of the input args.
  fn inputs<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    let n = 0;
    let p = Symbol::intern(&format!("P{}", n)).as_interned_str();
    let f = tcx.mk_ty_param(n, p);
    let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                               ty::BrAnon(0)));
    let t = tcx.mk_imm_ref(region, f);
    tcx.intern_type_list(&[t])
  }
  /// The return type.
  fn output<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>)
    -> ty::Ty<'tcx>
  {
    tcx.mk_imm_ref(tcx.types.re_static, tcx.types.usize)
  }
}
