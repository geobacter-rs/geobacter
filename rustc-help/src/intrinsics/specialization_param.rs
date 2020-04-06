
use std::fmt;

use rustc_middle::mir::*;
use rustc_middle::ty::{self, TyCtxt, };
use rustc_span::Symbol;

use super::*;

#[derive(Clone, Copy, Debug, Default, Hash)]
pub struct SpecializationParam;

impl SpecializationParam { }

impl GeobacterCustomIntrinsicMirGen for SpecializationParam {
  fn mirgen_simple_intrinsic<'tcx>(&self, dd: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut BodyAndCache<'tcx>)
  {
    let source_info = SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let mut bb = BasicBlockData {
      statements: Vec::new(),
      terminator: Some(mir::Terminator {
        source_info: source_info.clone(),
        kind: TerminatorKind::Return,
      }),

      is_cleanup: false,
    };

    let ret = Place::return_place();
    let local = Local::new(1);
    let local_ty = mir.local_decls[local].ty;

    let instance = extract_fn_instance(tcx, instance, local_ty);
    let param_data = dd.spec_param_data(tcx, instance);

    let slice = match param_data {
      Some(param_data) => {
        let alloc = Allocation::from_byte_aligned_bytes(&*param_data);
        let alloc = tcx.intern_const_alloc(alloc);
        tcx.alloc_map.lock().create_memory_alloc(alloc);
        ConstValue::Slice {
          data: alloc,
          start: 0,
          end: 1,
        }
      },
      None => {
        let alloc = Allocation::from_byte_aligned_bytes(&([0u8; 0])[..]);
        let alloc = tcx.intern_const_alloc(alloc);
        tcx.alloc_map.lock().create_memory_alloc(alloc);
        ConstValue::Slice {
          data: alloc,
          start: 0,
          end: 0,
        }
      },
    };

    let rvalue = const_value_rvalue(tcx, slice,
                                    self.output(tcx));

    let stmt_kind = StatementKind::Assign(Box::new((ret, rvalue)));
    let stmt = Statement {
      source_info: source_info.clone(),
      kind: stmt_kind,
    };
    bb.statements.push(stmt);
    mir.basic_blocks_mut().push(bb);
  }

  fn generic_parameter_count<'tcx>(&self, _tcx: TyCtxt<'tcx>) -> usize {
    2
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>> {
    let n = 0;
    let p = Symbol::intern("F");
    let f = tcx.mk_ty_param(n, p);
    let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                               ty::BrAnon(0)));
    let t = tcx.mk_imm_ref(region, f);
    tcx.intern_type_list(&[t])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    let n = 1;
    let p = Symbol::intern("R");
    let f = tcx.mk_ty_param(n, p);
    return mk_static_slice(tcx, f);
  }
}
impl fmt::Display for SpecializationParam {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_specialization_param")
  }
}
