
use rustc::hir::def_id::{DefId};
use rustc::ty::{TyCtxt, };
use codegen::worker::{DriverData, };

pub mod lang_item;
pub mod alloc;
pub mod panic;
pub mod compiler_builtins;

pub enum PassType {
  Replacer(for<'a, 'tcx, 'dd> fn(TyCtxt<'a, 'tcx, 'tcx>,
                                 &'dd DriverData<'tcx>,
                                 DefId) -> Option<DefId>),
}

pub trait Pass: Send + Sync {
  fn pass_type(&self) -> PassType;
}
