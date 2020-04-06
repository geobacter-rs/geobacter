/// Data parallel primitive intrinsics

use super::*;

use rustc_middle::mir::*;
use rustc_span::{DUMMY_SP, Symbol, };


/// fn __geobacter_update_dpp<T, const DPP_CTRL: i32, const ROW_MASK: i32, const BANK_MASK: i32,
///                           const BOUND_CTRL: bool>(old: T, src: T) -> T;
pub struct UpdateDpp;
impl UpdateDpp {
  fn kernel_instance_i32() -> KernelInstance {
    amdgcn_intrinsics::amdgcn_update_dpp_i32
      .kernel_instance()
      .unwrap()
  }
  fn intrinsic<'tcx>(tcx: TyCtxt<'tcx>, t: ty::Ty<'tcx>,
                     instance: ty::Instance<'tcx>)
    -> Option<ty::Instance<'tcx>>
  {
    let intrinsic = if t == tcx.types.i32 || t == tcx.types.u32 {
      Self::kernel_instance_i32()
    } else {
      tcx.sess.span_err(tcx.def_span(instance.def_id()),
                        "expected a 32-bit integer type");
      return None;
    };
    let intrinsic = tcx.convert_kernel_instance(intrinsic)
      .expect("failed to convert kernel instance to rustc instance");
    Some(intrinsic)
  }
}
impl GeobacterCustomIntrinsicMirGen for UpdateDpp {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _dd: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>)
  {
    info!("mirgen intrinsic {}", self);

    let t = instance.substs
      .types()
      .next()
      .unwrap();

    let intrinsic = Self::intrinsic(tcx, t, instance);

    let param_env = ty::ParamEnv::reveal_all();
    let consts = instance.substs.consts()
      .map(|const_arg| {
        let c = const_arg.eval(tcx, param_env);
        mir::Constant {
          span: DUMMY_SP,
          user_ty: None,
          literal: c,
        }
      })
      .map(Box::new)
      .map(Operand::Constant);

    let args = mir.args_iter()
      .map(Place::from)
      .map(Operand::Move)
      .chain(consts)
      .collect::<Vec<_>>();

    if args.len() != 6 {
      // param types are checked by rustc. we just need to check that the consts
      // are present.
      tcx.sess.span_err(tcx.def_span(instance.def_id()),
                        "expected 5 constant parameters");
      return;
    }

    common::call_device_func_args(tcx, mir, move || {
      // panic if not running on an AMDGPU
      match &tcx.sess.target.target.arch[..] {
        "amdgpu" => { },
        _ => { return None; },
      };

      Some((intrinsic?, args))
    });
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    1
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    let n = 0;
    let p = Symbol::intern("T");
    let p = tcx.mk_ty_param(n, p);
    tcx.intern_type_list(&[p, p, ])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    let n = 0;
    let p = Symbol::intern("T");
    tcx.mk_ty_param(n, p)
  }
}

impl fmt::Display for UpdateDpp {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_amdgpu_update_dpp")
  }
}

/// Use of constant generics in ^ crashes the current version of the compiler.
pub struct UpdateDppWorkaround;
impl GeobacterCustomIntrinsicMirGen for UpdateDppWorkaround {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _dd: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>)
  {
    info!("mirgen intrinsic {}", self);

    let t = instance.substs
      .types()
      .next()
      .unwrap();

    let intrinsic = UpdateDpp::intrinsic(tcx, t, instance);

    let args = mir.args_iter()
      .map(Place::from)
      .map(Operand::Move)
      .collect::<Vec<_>>();

    common::call_device_func_args(tcx, mir, move || {
      // panic if not running on an AMDGPU
      match &tcx.sess.target.target.arch[..] {
        "amdgpu" => { },
        _ => { return None; },
      };

      Some((intrinsic?, args))
    });
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    1
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    let n = 0;
    let p = Symbol::intern("T");
    let p = tcx.mk_ty_param(n, p);
    tcx.intern_type_list(&[p, p, tcx.types.i32, tcx.types.i32,
      tcx.types.i32, tcx.types.bool])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    let n = 0;
    let p = Symbol::intern("T");
    tcx.mk_ty_param(n, p)
  }
}

impl fmt::Display for UpdateDppWorkaround {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_amdgpu_update_dpp2")
  }
}
