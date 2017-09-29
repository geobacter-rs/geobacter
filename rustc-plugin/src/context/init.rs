
use std::cell::Cell;
use std::rc::Rc;

use rustc::ty::{TyCtxt};
use rustc::mir::{Mir};
use rustc::mir::transform::{MirPass, MirSource};

use super::GlobalCtx;

const HSA_CORE_CRATE_NAME: &'static str = "hsa_core";
const HSA_COMPILE_TIME_CRATE_NAME: &'static str = "compiletime";

#[derive(Debug, Clone)]
pub struct ContextLoader(GlobalCtx, Cell<bool>);

impl ContextLoader {
  pub fn new(ctxt: &GlobalCtx) -> ContextLoader {
    ContextLoader(ctxt.clone(),
                  Cell::new(false))
  }

  fn on_pass(&self) -> bool {
    let is_first = !self.1.get();
    self.1.set(true);
    is_first
  }
}


impl MirPass for ContextLoader {
  fn run_pass<'a, 'tcx>(&self,
                        tcx: TyCtxt<'a, 'tcx, 'tcx>,
                        src: MirSource,
                        mir: &mut Mir<'tcx>) {
    if !self.on_pass() { return; }

    let cstore = tcx.sess.cstore.clone();
    let all_crates = cstore.crates();
    for krate in all_crates.iter() {
      let original_crate_name = cstore
        .original_crate_name(*krate);

      if original_crate_name.as_str() == HSA_CORE_CRATE_NAME {
        self.0.with_mut(|ctxt| {
          ctxt.core_crate_num = Some(krate.clone());
        });
      } else if original_crate_name.as_str() == HSA_COMPILE_TIME_CRATE_NAME {
        self.0.with_mut(|ctxt| {
          ctxt.compiletime_crate_num = Some(krate.clone());
        });
      }
    }
  }
}
