
use std::cell::Cell;

use rustc::ty::{TyCtxt};
use rustc::mir::{Mir};
use rustc::mir::transform::{MirPass, MirSource};

use rustc_driver::driver::compute_crate_disambiguator;

use super::GlobalCtx;

const HSA_CORE_CRATE_NAME: &'static str = "hsa_core";
const HSA_COMPILE_TIME_CRATE_NAME: &'static str = "compiletime";
const STD_CRATE_NAME: &'static str = "std";

#[derive(Debug, Clone)]
pub struct ContextLoader {
  ctx: GlobalCtx,
  run_once: Cell<bool>,
}

impl ContextLoader {
  pub fn new(ctxt: &GlobalCtx) -> ContextLoader {
    ContextLoader {
      ctx: ctxt.clone(),
      run_once: Cell::new(false),
    }
  }

  fn on_pass(&self) -> bool {
    let is_first = !self.run_once.get();
    self.run_once.set(true);
    is_first
  }
}


impl MirPass for ContextLoader {
  fn run_pass<'a, 'tcx>(&self,
                        tcx: TyCtxt<'a, 'tcx, 'tcx>,
                        _src: MirSource,
                        _mir: &mut Mir<'tcx>) {
    use rustc::middle::lang_items::LangItem::PanicFmtLangItem;

    if !self.on_pass() { return; }

    self.ctx.with_mut(|ctxt| {
      ctxt.local_crate_disambiguator =
        compute_crate_disambiguator(tcx.sess);
      ctxt.panic_fmt_did =
        Some(tcx.require_lang_item(PanicFmtLangItem));
    });

    let all_crates = tcx.crates();
    for krate in all_crates.iter() {
      let original_crate_name = tcx
        .original_crate_name(*krate);

      if original_crate_name.as_str() == HSA_CORE_CRATE_NAME {
        self.ctx.with_mut(|ctxt| {
          ctxt.core_crate_num = Some(krate.clone());
        });
      } else if original_crate_name.as_str() == HSA_COMPILE_TIME_CRATE_NAME {
        self.ctx.with_mut(|ctxt| {
          ctxt.compiletime_crate_num = Some(krate.clone());
        });
      }
    }
  }
}
