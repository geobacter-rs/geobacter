
use std::cell::Cell;

use rustc::mir::{Mir};
use rustc::mir::transform::{MirPass, MirSource};
use rustc::ty::{TyCtxt};

#[derive(Debug, Default)]
pub struct Debug(Cell<bool>);

impl Debug {
  pub fn new() -> Debug {
    Debug(Cell::new(false))
  }
}

impl MirPass for Debug {
  fn run_pass<'a, 'tcx>(&self,
                        tcx: TyCtxt<'a, 'tcx, 'tcx>,
                        src: MirSource,
                        mir: &mut Mir<'tcx>) {
    if self.0.get() {
      return;
    }
    self.0.set(true);

    let cstore = tcx.sess.cstore.clone();
    let all_crates = cstore.crates();

    for krate in all_crates.iter() {
      let original_crate_name = cstore
        .original_crate_name(*krate);
      println!("crate #{} original name: {}", krate,
               original_crate_name);
    }
  }
}
