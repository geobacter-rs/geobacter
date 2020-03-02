
use std::fmt;
use std::marker::PhantomData;

use super::*;

/// Kill (ie `abort()`) the current workitem/thread only.
pub struct WorkItemKill<T>(PhantomData<T>)
  where T: PlatformImplDetail;
impl<T> WorkItemKill<T>
  where T: PlatformImplDetail,
{
  fn kernel_instance(&self) -> KernelInstance {
    T::kernel_instance()
  }
}
impl<T> Default for WorkItemKill<T>
  where T: PlatformImplDetail,
{
  fn default() -> Self {
    WorkItemKill(PhantomData)
  }
}
impl<T> GeobacterCustomIntrinsicMirGen for WorkItemKill<T>
  where T: PlatformImplDetail,
{
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _dd: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>)
  {
    trace!("mirgen intrinsic {}", self);

    call_device_func(tcx, mir, move || {
      let id = self.kernel_instance();
      let instance = tcx.convert_kernel_instance(id)
        .expect("failed to convert kernel id to def id");
      Some(instance)
    });
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    0
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    tcx.intern_type_list(&[])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    tcx.types.never
  }
}

impl<T> fmt::Display for WorkItemKill<T>
  where T: PlatformImplDetail,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_kill")
  }
}
