
use std::fmt;

use rustc_middle::mir::{Rvalue, Statement, StatementKind, };
use rustc_middle::mir::interpret::{ConstValue, Scalar, };
use rustc_middle::mir::{self, };
use rustc_middle::ty::{self, TyCtxt, };
use rustc_middle::ty::{Const, };
use rustc_span::{DUMMY_SP, };
use rustc_target::abi::Size;

use crate::grustc_help::*;

use crate::gvk_core::*;
use crate::common::{DriverData, GeobacterCustomIntrinsicMirGen, };

pub mod shader;
pub mod vk;

pub struct ExeModel(pub Option<ExecutionModel>);
impl GeobacterCustomIntrinsicMirGen for ExeModel {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _kid_did: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>)
  {
    info!("mirgen intrinsic {}", self);

    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let mk_u32 = |v: u32| {
      let v = Scalar::from_uint(v, Size::from_bytes(4));
      let v = ConstValue::Scalar(v);
      tcx.mk_const_op(source_info.clone(), Const {
        ty: tcx.types.u32,
        val: ty::ConstKind::Value(v),
      })
    };

    let ret = mir::Place::return_place();
    let rvalue = match self.0 {
      None => 0,
      Some(v) => (v as u32) + 1,
    };
    let rvalue = Rvalue::Use(mk_u32(rvalue));

    let stmt_kind = StatementKind::Assign(Box::new((ret, rvalue)));
    let stmt = Statement {
      source_info: source_info.clone(),
      kind: stmt_kind,
    };

    let bb = mir::BasicBlockData {
      statements: vec![stmt],
      terminator: Some(mir::Terminator {
        source_info,
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };
    mir.basic_blocks_mut().push(bb);
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
    tcx.types.u32
  }
}

impl fmt::Display for ExeModel {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_exe_model")
  }
}
