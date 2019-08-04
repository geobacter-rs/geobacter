
use rustc::hir::def_id::{DefId};
use rustc::ty::{TyCtxt, };

use crate::codegen::worker::{DriverData, };
use crate::codegen::PlatformCodegen;

pub enum PassType<P>
  where P: PlatformCodegen,
{
  Replacer(for<'tcx> fn(TyCtxt<'tcx>,
                        &DriverData<'tcx, P>,
                        DefId) -> Option<DefId>),
}

pub trait Pass<P>: Send + Sync
  where P: PlatformCodegen,
{
  fn pass_type(&self) -> PassType<P>;
}
