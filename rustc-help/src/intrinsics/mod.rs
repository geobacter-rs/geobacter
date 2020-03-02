
use std::marker::PhantomData;

use rustc_data_structures::sync::Lrc;

use shared_defs::kernel::KernelInstance;

use driver_data::{DriverData, GetDriverData, };
use super::*;

pub mod call_by_type;
pub mod current_platform;
pub mod specialization_param;
pub mod work_item_kill;

pub use call_by_type::*;
pub use current_platform::*;
pub use specialization_param::*;
pub use work_item_kill::*;

pub trait PlatformImplDetail: Send + Sync + 'static {
  fn kernel_instance() -> KernelInstance;
}

pub trait GeobacterCustomIntrinsicMirGen: Send + Sync + 'static {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   kid_did: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>);

  fn generic_parameter_count<'tcx>(&self, tcx: TyCtxt<'tcx>) -> usize;
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>>;
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx>;
}

pub struct GeobacterMirGen<T, U>(T, PhantomData<U>)
  where T: GeobacterCustomIntrinsicMirGen + Send + Sync + 'static,
        U: GetDriverData + Send + Sync + 'static;

impl<T, U> GeobacterMirGen<T, U>
  where T: GeobacterCustomIntrinsicMirGen + fmt::Display + Send + Sync + 'static,
        U: GetDriverData + Send + Sync + 'static,
{
  pub fn new(intrinsic: T, _: &U) -> (String, Lrc<dyn CustomIntrinsicMirGen>) {
    let name = format!("{}", intrinsic);
    let mirgen: Self = GeobacterMirGen(intrinsic, PhantomData);
    let mirgen = Lrc::new(mirgen) as Lrc<_>;
    (name, mirgen)
  }
}
impl<T, U> GeobacterMirGen<T, U>
  where T: GeobacterCustomIntrinsicMirGen + Send + Sync + 'static,
        U: GetDriverData + Send + Sync + 'static,
{
  pub fn wrap(intrinsic: T, _: &U) -> Lrc<dyn CustomIntrinsicMirGen> {
    let mirgen: Self = GeobacterMirGen(intrinsic, PhantomData);
    let mirgen = Lrc::new(mirgen) as Lrc<_>;
    mirgen
  }
}
impl<T, U> Default for GeobacterMirGen<T, U>
  where T: GeobacterCustomIntrinsicMirGen + Default + Send + Sync + 'static,
        U: GetDriverData + Send + Sync + 'static,
{
  fn default() -> Self {
    GeobacterMirGen(T::default(), PhantomData)
  }
}

impl<T, U> CustomIntrinsicMirGen for GeobacterMirGen<T, U>
  where T: GeobacterCustomIntrinsicMirGen + Send + Sync + 'static,
        U: GetDriverData + Send + Sync,
{
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>)
  {
    U::with_self(tcx, |s| {
      self.0.mirgen_simple_intrinsic(s, tcx,
                                     instance, mir)
    })
  }

  fn generic_parameter_count(&self, tcx: TyCtxt) -> usize {
    self.0.generic_parameter_count(tcx)
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>> {
    self.0.inputs(tcx)
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    self.0.output(tcx)
  }
}
