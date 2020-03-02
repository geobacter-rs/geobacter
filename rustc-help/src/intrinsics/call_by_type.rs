
use std::fmt;

use rustc::mir::*;
use rustc::ty::{self, TyCtxt, TyKind, layout::VariantIdx, };
use rustc_span::Symbol;

use super::*;

#[derive(Clone, Copy, Debug, Default, Hash)]
pub struct CallByType;

impl CallByType { }

impl mir::CustomIntrinsicMirGen for CallByType {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>) {
    let source_info = SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    assert_eq!(mir.basic_blocks().len(), 0);

    let ret_bb = BasicBlock::new(1);

    let local_ty = instance.substs.type_at(0);

    let args = Local::new(1);
    let args_ty = mir.local_decls[args].ty;
    let args_ty = match args_ty.kind {
      TyKind::Tuple(i) => i.types(),
      _ => bug!("unexpected type: {:?}", args_ty),
    };

    let args = args_ty.enumerate()
      .map(|(i, _)| {
        let vi = VariantIdx::from_usize(i);
        let proj = tcx.mk_place_downcast_unnamed(args.into(), vi);
        Operand::Move(proj)
      })
      .collect();

    let instance = extract_fn_instance(tcx, instance, local_ty);

    let func = Operand::function_handle(tcx, instance.def_id(),
                                        instance.substs,
                                        DUMMY_SP);

    let ret = Place::return_place();

    let bb = BasicBlockData {
      statements: Vec::new(),
      terminator: Some(mir::Terminator {
        source_info: source_info.clone(),
        kind: TerminatorKind::Call {
          func,
          args,
          destination: Some((ret, ret_bb)),
          cleanup: None,
          from_hir_call: false,
        },
      }),

      is_cleanup: false,
    };
    mir.basic_blocks_mut().push(bb);

    let ret_bb = BasicBlockData {
      statements: Vec::new(),
      terminator: Some(mir::Terminator {
        source_info: source_info.clone(),
        kind: TerminatorKind::Return,
      }),

      is_cleanup: false,
    };
    mir.basic_blocks_mut().push(ret_bb);
  }

  fn generic_parameter_count<'tcx>(&self, _tcx: TyCtxt<'tcx>) -> usize {
    3
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>> {
    let n = 1;
    let p = Symbol::intern("A");
    let a = tcx.mk_ty_param(n, p);
    tcx.intern_type_list(&[a])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    let n = 2;
    let p = Symbol::intern("R");
    tcx.mk_ty_param(n, p)
  }
}
impl GeobacterCustomIntrinsicMirGen for CallByType {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _dd: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>)
  {
    mir::CustomIntrinsicMirGen::mirgen_simple_intrinsic(self, tcx, instance, mir)
  }

  fn generic_parameter_count(&self, tcx: TyCtxt) -> usize {
    mir::CustomIntrinsicMirGen::generic_parameter_count(self,  tcx)
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>> {
    mir::CustomIntrinsicMirGen::inputs(self,  tcx)
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    mir::CustomIntrinsicMirGen::output(self,  tcx)
  }
}

impl fmt::Display for CallByType {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_call_by_type")
  }
}
