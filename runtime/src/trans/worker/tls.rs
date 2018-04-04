
use super::{WorkerTranslatorData, TranslatorCtxtRef};

use rustc::ty::tls;

use std::cell::Cell;
use std::mem::transmute;

enum ThreadLocalWorkerState {}

thread_local! {
  static CTXT: Cell<Option<*const ThreadLocalWorkerState>> = Cell::new(None);
}

pub fn enter<'a, 'b, 'c, F, R>(state: &'c WorkerTranslatorData<'a>, f: F) -> R
  where F: for<'a2, 'b2, 'tcx> FnOnce(TranslatorCtxtRef<'a2, 'b2, 'a2, 'tcx>) -> R,
        'a: 'c,
{
  let state_ptr = state as *const _ as *const ThreadLocalWorkerState;
  CTXT.with(|tls| {
    let prev = tls.get();
    tls.set(Some(state_ptr));
    let ret = tls::with(|tcx| {
      let r = TranslatorCtxtRef {
        tcx: tcx.global_tcx(),
        worker: state,
      };
      f(r)
    });
    tls.set(prev);
    ret
  })
}

pub fn with<F, R>(f: F) -> R
  where F: for<'a, 'b, 'tcx: 'b> FnOnce(TranslatorCtxtRef<'a, 'b, 'a, 'tcx>) -> R,
{
  tls::with(|tcx| {
    CTXT.with(|state| {
      let state_ptr = state.get().unwrap();
      let state_ref = unsafe {
        &*(state_ptr as *const WorkerTranslatorData)
      };
      let r = TranslatorCtxtRef {
        tcx: tcx.global_tcx(),
        worker: state_ref,
      };
      f(r)
    })
  })
}
