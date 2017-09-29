
use std::cell::RefCell;
use std::rc::Rc;

use rustc::hir::def_id::{DefId, CrateNum, };

pub mod init;

#[derive(Debug, Default)]
pub struct GlobalCtx_ {
  core_crate_num: Option<CrateNum>,
  compiletime_crate_num: Option<CrateNum>,

  kernel_info_for_def_id: Option<DefId>,
  //def_id_to_kernel_info: HashMap<DefId, KernelInfo>,
}

#[derive(Clone, Debug)]
pub struct GlobalCtx(Rc<RefCell<GlobalCtx_>>);

impl GlobalCtx {
  pub fn new() -> GlobalCtx {
    GlobalCtx(Default::default())
  }

  pub fn compiletime_crate_num(&self) -> CrateNum {
    self.0.borrow().compiletime_crate_num
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
