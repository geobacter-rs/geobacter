
use rustc_middle::ty::{self, TyCtxt, };
use rustc_serialize::Encodable;
use rustc_data_structures::sync::*;

use shared_defs::kernel::KernelInstanceRef;

use crate::codec::GeobacterEncoder;
use crate::stubbing::Stubber;

pub trait DriverData {
  fn spec_param_data_raw(&self, _instance: KernelInstanceRef)
    -> Option<MappedReadGuard<[u8]>>
  {
    None
  }
  fn spec_param_data<'tcx>(&self, tcx: TyCtxt<'tcx>,
                           instance: ty::Instance<'tcx>)
    -> Option<MappedReadGuard<[u8]>>
  {
    let instance = GeobacterEncoder::with(tcx, |encoder| {
      instance.encode(encoder).expect("actual encode kernel instance");
      Ok(())
    }).expect("encode kernel instance");
    let instance = KernelInstanceRef {
      name: "",
      instance: &instance,
    };
    self.spec_param_data_raw(instance)
  }

  fn stubber(&self) -> Option<MappedReadGuard<dyn Stubber>> {
    None
  }
}
pub trait GetDriverData {
  fn with_self<'tcx, F, R>(tcx: TyCtxt<'tcx>, f: F) -> R
    where F: FnOnce(&dyn DriverData) -> R;
}
