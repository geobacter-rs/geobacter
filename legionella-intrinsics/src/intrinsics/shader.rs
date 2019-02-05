
/// mirgen for the various shader specific intrinsics

use std::fmt;

use crate::rustc::mir::{self, mono::MonoItem, Local,
                        CustomIntrinsicMirGen, };
use crate::rustc::ty::{self, TyCtxt, Instance, };
use crate::rustc::ty::{ParamEnv, };
use crate::rustc_data_structures::indexed_vec::*;
use crate::rustc_data_structures::fx::{FxHashSet, };
use crate::rustc_data_structures::sync::{Lrc, };
use crate::syntax_pos::{DUMMY_SP, symbol::Symbol, };

use crate::{DefIdFromKernelId, LegionellaCustomIntrinsicMirGen,
            GetDefIdFromKernelId, LegionellaMirGen };

use crate::lcore::*;
use crate::collector::collect_items_rec;
use crate::attrs::{legionella_root_attrs, legionella_global_attrs, };
use crate::stubbing;

pub fn insert_all_intrinsics<F, U>(marker: &U, mut into: F)
  where F: FnMut(String, Lrc<dyn CustomIntrinsicMirGen>),
        U: GetDefIdFromKernelId + Send + Sync + 'static,
{
  for id in CheckFn::permutations().into_iter() {
    let (k, v) = LegionellaMirGen::new(id, marker);
    into(k, v);
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CheckFn(pub ExecutionModel);

impl CheckFn {
  fn permutations() -> Vec<Self> {
    vec![
      CheckFn(ExecutionModel::Vertex),
      CheckFn(ExecutionModel::Geometry),
      CheckFn(ExecutionModel::TessellationControl),
      CheckFn(ExecutionModel::TessellationEval),
      CheckFn(ExecutionModel::Fragment),
      CheckFn(ExecutionModel::Kernel),
    ]
  }
}

impl fmt::Display for CheckFn {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let kind = match self.0 {
      ExecutionModel::Vertex => "vertex",
      ExecutionModel::Geometry => "geometry",
      ExecutionModel::TessellationControl => "tessellation_control",
      ExecutionModel::TessellationEval => "tessellation_eval",
      ExecutionModel::Fragment => "fragment",
      ExecutionModel::GLCompute => "glcompute",
      ExecutionModel::Kernel => "kernel",
    };
    write!(f, "__legionella_check_{}_shader", kind)
  }
}
impl LegionellaCustomIntrinsicMirGen for CheckFn {
  fn mirgen_simple_intrinsic<'a, 'tcx>(&self,
                                       stubber: &stubbing::Stubber,
                                       kid_did: &dyn DefIdFromKernelId,
                                       tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       instance: ty::Instance<'tcx>,
                                       mir: &mut mir::Mir<'tcx>)
    where 'tcx: 'a,
  {
    // Create an empty function:
    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let bb = mir::BasicBlockData {
      statements: vec![],
      terminator: Some(mir::Terminator {
        source_info,
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };
    mir.basic_blocks_mut().push(bb);

    // do the checking:
    let reveal_all = ParamEnv::reveal_all();

    let local = Local::new(1);
    let local_ty = mir.local_decls[local].ty;
    let local_ty = tcx
      .subst_and_normalize_erasing_regions(instance.substs,
                                           reveal_all,
                                           &local_ty);

    let instance = match local_ty.sty {
      ty::Ref(_, &ty::TyS {
        sty: ty::FnDef(def_id, subs),
        ..
      }, ..) => {
        let subs = tcx
          .subst_and_normalize_erasing_regions(instance.substs,
                                               reveal_all,
                                               &subs);
        ty::Instance::resolve(tcx, reveal_all, def_id, subs)
          .expect("must be resolvable")
      },
      _ => {
        unreachable!("unexpected param type: {:?}", local_ty);
      },
    };

    let did = instance.def_id();
    let ty = instance.ty(tcx);
    let sig = ty.fn_sig(tcx);
    let sig = tcx.normalize_erasing_late_bound_regions(reveal_all, &sig);
    // check that the function satisfies Fn<(), Output = ()>:
    if sig.inputs().len() != 0 {
      let msg = "shader/kernel function must accept no parameters";
      tcx.sess.span_err(tcx.def_span(did), &msg);
    }
    if sig.output() != tcx.types.unit {
      let msg = format!("shader/kernel function must return `()`; found {:?}",
                        sig.output());
      tcx.sess.span_err(tcx.def_span(did), &msg);
    }

    // collect all referenced mono items upfront:
    let mono_root = MonoItem::Fn(instance);
    let mut visited: FxHashSet<_> = Default::default();
    collect_items_rec(tcx, stubber, kid_did,
                      mono_root, &mut visited,
                      &mut None);

    // TODO Question: should we report a stack trace of an errant item's
    // usage?

    // visit each mono item and check annotated requirements are
    // respected
    visited.remove(&mono_root);

    let root_attrs = legionella_root_attrs(tcx, did,
                                           self.0,
                                           true);

    for mono in visited.into_iter() {
      let instance = match mono {
        MonoItem::Fn(instance) => instance,
        MonoItem::Static(mono_did) => Instance::mono(tcx, mono_did),
        MonoItem::GlobalAsm(..) => {
          bug!("unexpected `{:?}`", mono);
        },
      };

      let mono_attrs = legionella_global_attrs(tcx,
                                               self.0,
                                               instance,
                                               true);

      if !mono_attrs.capabilities().eval(&|cap| root_attrs.capabilities.contains(cap) ) {
        let msg = "unsatisfied capability (TODO which one???)";
        tcx.sess.span_err(tcx.def_span(instance.def_id()), &msg);
      }

      if !mono_attrs.exe_model().eval(&|model| model == &root_attrs.execution_model ) {
        let msg = "unsatisfied execution model requirement";
        tcx.sess.span_err(tcx.def_span(instance.def_id()), &msg);
        let msg = format!("attrs: {:#?}", mono_attrs);
        tcx.sess.span_note_without_error(tcx.def_span(instance.def_id()),
                                         &msg);
      }

      // TODO check Vulkan requirements for builtins, inputs, outputs, etc
    }
  }

  fn generic_parameter_count<'a, 'tcx>(&self, _tcx: TyCtxt<'a, 'tcx, 'tcx>)
    -> usize
  {
    1
  }
  /// The types of the input args.
  fn inputs<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    let n = 0;
    let p = Symbol::intern(&format!("P{}", n)).as_interned_str();
    let f = tcx.mk_ty_param(n, p);
    let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                               ty::BrAnon(0)));
    let t = tcx.mk_imm_ref(region, f);
    tcx.intern_type_list(&[t])
  }
  /// The return type.
  fn output<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>) -> ty::Ty<'tcx> {
    tcx.types.unit
  }
}
