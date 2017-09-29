
use rustc

pub fn generate_kernel_info<'a>(bcx: &Builder<'a, 'a>,
                                callee_ty: Ty<'a>,
                                fn_ty: &FnType,
                                llargs: &[ValueRef],
                                llresult: ValueRef,
                                span: Span) {
  let ccx = bcx.ccx;
  let tcx = ccx.tcx();

  let (def_id, substs) = match callee_ty.sty {
    ty::TyFnDef(def_id, substs) => (def_id, substs),
    _ => bug!("expected fn item type, found {}", callee_ty)
  };

  let sig = callee_ty.fn_sig(tcx);
  let sig = tcx.erase_late_bound_regions_and_normalize(&sig);
  let arg_tys = sig.inputs();
  let ret_ty = sig.output();
  let name = &*tcx.item_name(def_id).as_str();

  let llret_ty = type_of::type_of(ccx, ret_ty);

  unimplemented!();
}
