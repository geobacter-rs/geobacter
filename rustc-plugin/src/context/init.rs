
use std::cell::Cell;
use std::rc::Rc;

use rustc::ty::{TyCtxt};
use rustc::mir::{Mir};
use rustc::mir::transform::{MirPass, MirSource};

use rustc_driver::driver::compute_crate_disambiguator;

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

    self.0.with_mut(|ctxt| {
      ctxt.local_crate_disambiguator =
        compute_crate_disambiguator(tcx.sess);
    });

    let all_crates = tcx.crates();
    for krate in all_crates.iter() {
      let original_crate_name = tcx
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
