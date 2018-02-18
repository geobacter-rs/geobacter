
use rustc::ty::{Ty, };
use rustc::ty::layout::{LayoutTyper, Layout, TyLayout};

pub fn type_is_fat_ptr<'tcx, T>(ccx: T, ty: Ty<'tcx>) -> bool
  where T: LayoutTyper<'tcx, TyLayout = TyLayout<'tcx>>,
{
  if let Layout::FatPointer { .. } = *ccx.layout_of(ty) {
    true
  } else {
    false
  }
}