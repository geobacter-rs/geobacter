//! This crate is used in all three drivers: the bootstrap driver,
//! the host driver, and the runtime driver. This provides a driver
//! agnostic interface for implementing custom Rust intrinsics and
//! translating our `KernelId`s into Rust's `DefId`s.

#![feature(rustc_private)]
#![feature(core_intrinsics, std_internals)]
#![feature(box_patterns)]
#![feature(link_llvm_intrinsics)]
#![feature(intrinsics)]

#[macro_use]
extern crate rustc;
extern crate rustc_codegen_utils;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_index;
extern crate rustc_metadata;
extern crate rustc_mir;
extern crate rustc_target;
extern crate serialize;
extern crate syntax;
extern crate syntax_pos;

#[macro_use]
extern crate log;
extern crate num_traits;
extern crate seahash;

extern crate geobacter_core;
extern crate geobacter_rustc_help as rustc_help;

pub mod attrs;
pub mod collector;
pub mod hash;
pub mod platform;
pub mod stubbing;

// Note: don't try to depend on `geobacter_std`.
use std::fmt;
use std::marker::PhantomData;

use geobacter_core::kernel::{KernelInstance};

use self::rustc::hir::def_id::{DefId, };
use self::rustc::middle::lang_items::{self, LangItem, };
use self::rustc::mir::{self, CustomIntrinsicMirGen, Operand, Place,
                       Constant, };
use self::rustc::ty::{self, TyCtxt, Instance, };
use self::rustc_index::vec::Idx;
use self::rustc_data_structures::sync::{Lrc, };
use self::syntax_pos::{Span, DUMMY_SP, };
use syntax::symbol::Symbol;

pub use rustc_help::*;

/// TODO move the real runtime Driver data into this crate.
pub trait DriverData { }
pub trait GetDriverData {
  fn with_self<'tcx, F, R>(tcx: TyCtxt<'tcx>, f: F) -> R
    where F: FnOnce(&dyn DriverData) -> R;
}

pub trait GeobacterCustomIntrinsicMirGen: Send + Sync + 'static {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   kid_did: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::Body<'tcx>);

  fn generic_parameter_count<'tcx>(&self, tcx: TyCtxt<'tcx>) -> usize;
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>>;
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx>;
}

/// CurrentPlatform doesn't need anything special, but is used from the runtimes.
impl GeobacterCustomIntrinsicMirGen for CurrentPlatform {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   _kid_did: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::Body<'tcx>)
  {
    CustomIntrinsicMirGen::mirgen_simple_intrinsic(self, tcx, instance, mir)
  }

  fn generic_parameter_count(&self, tcx: TyCtxt) -> usize {
    CustomIntrinsicMirGen::generic_parameter_count(self,  tcx)
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>> {
    CustomIntrinsicMirGen::inputs(self,  tcx)
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    CustomIntrinsicMirGen::output(self,  tcx)
  }
}

pub struct GeobacterMirGen<T, U>(T, PhantomData<U>)
  where T: GeobacterCustomIntrinsicMirGen + Send + Sync + 'static,
        U: GetDriverData + Send + Sync + 'static;

impl<T, U> GeobacterMirGen<T, U>
  where T: GeobacterCustomIntrinsicMirGen + fmt::Display + Send + Sync + 'static,
        U: GetDriverData + Send + Sync + 'static,
{
  pub fn new(intrinsic: T, _: &U) -> (String, Lrc<dyn CustomIntrinsicMirGen>) {
    let name = format!("{}", intrinsic);
    let mirgen: Self = GeobacterMirGen(intrinsic, PhantomData);
    let mirgen = Lrc::new(mirgen) as Lrc<_>;
    (name, mirgen)
  }
}
impl<T, U> GeobacterMirGen<T, U>
  where T: GeobacterCustomIntrinsicMirGen + Send + Sync + 'static,
        U: GetDriverData + Send + Sync + 'static,
{
  pub fn wrap(intrinsic: T, _: &U) -> Lrc<dyn CustomIntrinsicMirGen> {
    let mirgen: Self = GeobacterMirGen(intrinsic, PhantomData);
    let mirgen = Lrc::new(mirgen) as Lrc<_>;
    mirgen
  }
}

impl<T, U> CustomIntrinsicMirGen for GeobacterMirGen<T, U>
  where T: GeobacterCustomIntrinsicMirGen + Send + Sync + 'static,
        U: GetDriverData + Send + Sync,
{
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::Body<'tcx>)
  {
    U::with_self(tcx, |s| {
      let stubs = stubbing::Stubber::default(); // TODO move into the drivers
      self.0.mirgen_simple_intrinsic(&stubs, s, tcx,
                                     instance, mir)
    })
  }

  fn generic_parameter_count(&self, tcx: TyCtxt) -> usize {
    self.0.generic_parameter_count(tcx)
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>> {
    self.0.inputs(tcx)
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    self.0.output(tcx)
  }
}

/// Either call the instance returned from `f` or insert code to panic.
/// TODO this should probably be turned into an attribute so it's more systematic.
pub fn redirect_or_panic<'tcx, F>(tcx: TyCtxt<'tcx>,
                                  mir: &mut mir::Body<'tcx>,
                                  f: F)
  where F: FnOnce() -> Option<Instance<'tcx>>,
{
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

  let (real_instance, args, term_kind) = match f() {
    Some(instance) => {
      (instance, vec![], mir::TerminatorKind::Return)
    },
    None => {
      // call `panic` from `libcore`
      let lang_item = lang_items::PanicFnLangItem;

      let def_id = langcall(tcx, None, "", lang_item);
      let instance = Instance::mono(tcx, def_id);

      let loc = tcx.const_caller_location((
        Symbol::intern("TODO panic file location"),
        0,
        1,
      ));
      let loc = Constant {
        span: source_info.span,
        literal: loc,
        user_ty: None,
      };
      let loc = Operand::Constant(Box::new(loc));
      let desc = tcx.mk_static_str_operand(source_info,
                                           "Device function called on the host");

      (instance,
       vec![desc, loc],
       mir::TerminatorKind::Unreachable)
    },
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
    destination: Some((Place::return_place(), success)),
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

pub trait PlatformImplDetail: Send + Sync + 'static {
  fn kernel_instance() -> KernelInstance;
}

/// Kill (ie `abort()`) the current workitem/thread only.
pub struct WorkItemKill<T>(PhantomData<T>)
  where T: PlatformImplDetail;
impl<T> WorkItemKill<T>
  where T: PlatformImplDetail,
{
  fn kernel_instance(&self) -> KernelInstance {
    T::kernel_instance()
  }
}
impl<T> Default for WorkItemKill<T>
  where T: PlatformImplDetail,
{
  fn default() -> Self {
    WorkItemKill(PhantomData)
  }
}
impl<T> GeobacterCustomIntrinsicMirGen for WorkItemKill<T>
  where T: PlatformImplDetail,
{
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   _dd: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   mir: &mut mir::Body<'tcx>)
  {
    trace!("mirgen intrinsic {}", self);

    redirect_or_panic(tcx, mir, move || {
      let id = self.kernel_instance();
      let instance = tcx.convert_kernel_instance(id)
        .expect("failed to convert kernel id to def id");
      Some(instance)
    });
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

impl<T> fmt::Display for WorkItemKill<T>
  where T: PlatformImplDetail,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_kill")
  }
}
pub struct HostKillDetail;
impl PlatformImplDetail for HostKillDetail {
  fn kernel_instance() -> KernelInstance {
    fn host_kill() -> ! {
      panic!("__geobacter_kill");
    }

    KernelInstance::get(&host_kill)
  }
}
pub type WorkItemHostKill = WorkItemKill<HostKillDetail>;
