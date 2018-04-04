
use std::cell::RefCell;
use std::rc::Rc;

use rustc::hir::def_id::{DefId, CrateNum, };
use rustc::ty::{TyCtxt};
use rustc_data_structures::stable_hasher::{self, HashStable};

pub mod init;

#[derive(Debug)]
pub struct GlobalCtx_ {
  id_gen: stable_hasher::StableHasher<u64>,
  core_crate_num: Option<CrateNum>,
  compiletime_crate_num: Option<CrateNum>,

  panic_fmt_did: Option<DefId>,

  kernel_info_for_def_id: Option<DefId>,
  //def_id_to_kernel_info: HashMap<DefId, KernelInfo>,
}

#[derive(Clone, Debug)]
pub struct GlobalCtx(Rc<RefCell<GlobalCtx_>>);

impl GlobalCtx {
  pub fn new() -> GlobalCtx {
    GlobalCtx(Default::default())
  }

  pub fn function_def_hash(&self, tcx: TyCtxt, def_id: DefId) -> u64 {
    let mut hcx = tcx.create_stable_hashing_context();
    let b = self.0.borrow();
    let mut hash = b.id_gen.clone();
    def_id.hash_stable(&mut hcx, &mut hash);
    hash.finish()
  }

  pub fn compiletime_crate_num(&self) -> CrateNum {
    self.0.borrow().compiletime_crate_num
      .clone()
      .expect("internal initialization error")
  }

  pub fn panic_fmt_lang_item(&self) -> DefId {
    self.0.borrow()
      .panic_fmt_did
      .clone()
      .expect("internal initialization error")
  }

  pub fn with_mut<F, U>(&self, f: F) -> U
    where F: FnOnce(&mut GlobalCtx_) -> U,
  {
    let mut b = self.0.borrow_mut();
    f(&mut *b)
  }
}

impl Default for GlobalCtx_ {
  fn default() -> Self {
    GlobalCtx_ {
      id_gen: stable_hasher::StableHasher::new(),
      core_crate_num: Default::default(),
      compiletime_crate_num: Default::default(),
      panic_fmt_did: None,
      kernel_info_for_def_id: Default::default(),
    }
  }
}