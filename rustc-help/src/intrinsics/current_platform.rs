use std::fmt;
use std::mem::{size_of, transmute, };

use crate::GeobacterTyCtxtHelp;
use crate::driver_data::DriverData;
use crate::intrinsics::GeobacterCustomIntrinsicMirGen;

use shared_defs::platform::*;

use rustc_middle::mir::{self, Rvalue, Statement, StatementKind, };
use rustc_middle::mir::interpret::{ConstValue, Pointer, Allocation, };
use rustc_middle::ty::{self, TyCtxt, };
use rustc_middle::ty::{Const, ConstKind, };
use rustc_span::DUMMY_SP;
use rustc_target::abi::Align;

/// This intrinsic has to be manually inserted by the drivers
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct CurrentPlatform(pub Platform);
impl CurrentPlatform {
  pub const fn host_platform() -> Self { CurrentPlatform(host_platform()) }

  fn data(self) -> [u8; size_of::<Platform>()] {
    unsafe {
      transmute(self.0)
    }
  }
}
impl fmt::Display for CurrentPlatform {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_current_platform")
  }
}
impl mir::CustomIntrinsicMirGen for CurrentPlatform {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>) {
    let align = Align::from_bits(64).unwrap(); // XXX arch dependent.
    let data = &self.data()[..];
    let alloc = Allocation::from_bytes(data, align);
    let alloc = tcx.intern_const_alloc(alloc);
    let alloc_id = tcx.alloc_map.lock()
      .create_memory_alloc(alloc);

    let ret = mir::Place::return_place();

    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let mut bb = mir::BasicBlockData {
      statements: Vec::new(),
      terminator: Some(mir::Terminator {
        source_info: source_info.clone(),
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };

    let ptr = Pointer::from(alloc_id);
    let const_val = ConstValue::Scalar(ptr.into());
    let constant = tcx.mk_const_op(source_info.clone(), Const {
      ty: GeobacterCustomIntrinsicMirGen::output(self, tcx),
      val: ConstKind::Value(const_val),
    });
    let rvalue = Rvalue::Use(constant);

    let stmt_kind = StatementKind::Assign(Box::new((ret, rvalue)));
    let stmt = Statement {
      source_info: source_info.clone(),
      kind: stmt_kind,
    };
    bb.statements.push(stmt);
    mir.basic_blocks_mut().push(bb);
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    0
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>> {
    tcx.intern_type_list(&[])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    let arr = tcx.mk_array(tcx.types.u8, size_of::<Platform>() as _);
    tcx.mk_imm_ref(tcx.lifetimes.re_static, arr)
  }
}
/// CurrentPlatform doesn't need anything special, but is used from the runtimes.
impl GeobacterCustomIntrinsicMirGen for CurrentPlatform {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _kid_did: &dyn DriverData,
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
