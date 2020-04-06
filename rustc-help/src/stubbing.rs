
use rustc_middle::ty::{Instance, InstanceDef, TyCtxt, };
use rustc_hir::def_id::DefId;

use crate::driver_data::DriverData;

pub trait Stubber {
  fn map_instance<'tcx>(&self,
                        tcx: TyCtxt<'tcx>,
                        dd: &dyn DriverData,
                        inst: Instance<'tcx>)
    -> Instance<'tcx>
  {
    let did = match inst.def {
      InstanceDef::Item(did) => did,
      _ => { return inst; },
    };

    let did = self.stub_def_id(tcx, dd, did);

    // we should be able to just replace `stub_instance.substs`
    // with `inst.substs`, assuming our stub signatures match the
    // real functions (which they should).

    return Instance {
      def: InstanceDef::Item(did),
      substs: inst.substs,
    };
  }
  fn stub_def_id<'tcx>(&self,
                       tcx: TyCtxt<'tcx>,
                       dd: &dyn DriverData,
                       did: DefId)
    -> DefId;
}
impl Stubber for () {
  #[inline(always)]
  fn stub_def_id<'tcx>(&self,
                       _tcx: TyCtxt<'tcx>,
                       _dd: &dyn DriverData,
                       did: DefId)
    -> DefId
  {
    did
  }
}
