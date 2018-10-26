
use super::{WorkerTranslatorData, TranslatorCtx};

use rustc::hir::def_id::{DefId, };
use rustc::mir::{CustomIntrinsicMirGen, };
use rustc::ty::TyCtxt;
use rustc_data_structures::fx::{FxHashMap, };
use rustc_data_structures::sync::Lrc;
use syntax_pos::symbol::{InternedString, };

use std::cell::Cell;

enum ThreadLocalWorkerState {}

thread_local! {
  static CTXT: Cell<Option<*const ThreadLocalWorkerState>> = Cell::new(None);
}

pub fn enter<'a, 'b, 'c, 'tcx, F, R>(state: &'c WorkerTranslatorData<'a>,
                                     tcx: TyCtxt<'_, 'tcx, 'tcx>,
                                     root: DefId,
                                     f: F) -> R
  where F: for<'a2, 'b2> FnOnce(TranslatorCtx<'a2, 'b2, 'a2, 'tcx>) -> R,
        'a: 'c,
{
  let ctx = TranslatorCtx {
    tcx: tcx.global_tcx(),
    worker: state,
    root,
  };
  let ctx_ptr = &ctx as *const _ as *const ThreadLocalWorkerState;
  CTXT.with(|tls| {
    let prev = tls.get();
    tls.set(Some(ctx_ptr));
    let ret = f(ctx);
    tls.set(prev);
    ret
  })
}

pub fn with<F, R>(f: F) -> R
  where F: for<'a, 'b, 'tcx> FnOnce(TranslatorCtx<'a, 'b, 'a, 'tcx>) -> R,
{
  CTXT.with(|ctx| {
    let ctx_ptr = ctx.get().unwrap();
    let ctx_ref = unsafe {
      &*(ctx_ptr as *const TranslatorCtx)
    };
    f(*ctx_ref)
  })
}
pub fn with_tcx<'a, 'tcx, F, R>(_tcx: TyCtxt<'a, 'tcx, 'tcx>, f: F) -> R
  where F: for<'b> FnOnce(TranslatorCtx<'a, 'b, 'a, 'tcx>) -> R,
{
  CTXT.with(|ctx| {
    let ctx_ptr = ctx.get().unwrap();
    let ctx_ref = unsafe {
      &*(ctx_ptr as *const TranslatorCtx)
    };
    f(unsafe { ::std::mem::transmute(*ctx_ref) })
  })
}
