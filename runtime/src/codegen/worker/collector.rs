
//! Figure out what needs codegening. Some of this is redundant, particularly
//! the evaluation of statics, in fact we should copy *exactly* the output of
//! the host evaluation.
//!
//! This contains code from the relevant parts of `rustc`.
//!
//! We proceed by gathering all possible mono items, then go back over the
//! items, possibly applying transforms specific to device capabilities.

use rustc::hir::def_id::{CrateNum, DefId, };
use rustc::middle::lang_items::{ExchangeMallocFnLangItem, StartFnLangItem};
use rustc::mir::{self, Location, Promoted, };
use rustc::mir::interpret::{AllocId, ConstValue, GlobalId, AllocType, Scalar,};
use rustc::mir::mono::{CodegenUnit, MonoItem, };
use rustc::ty::query::Providers;
use rustc::ty::{self, TyCtxt, Instance, ParamEnv, subst::Substs, };
use rustc::util::nodemap::{DefIdSet, FxHashSet, FxHashMap, DefIdMap, };
use rustc_mir::monomorphize;

use std::sync::{Arc, };

use super::tls;
use super::{TranslatorCtx, };

pub fn provide(providers: &mut Providers) {
  providers.collect_and_partition_mono_items =
    collect_and_partition_mono_items;
}

fn collect_and_partition_mono_items<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                              cnum: CrateNum)
  -> (Arc<DefIdSet>, Arc<Vec<Arc<CodegenUnit<'tcx>>>>)
{
  tls::with(|ctx| {
    collect_and_partition_mono_items_(ctx, cnum)
  })
}
fn collect_and_partition_mono_items_<'a, 'b, 'tcx>(tcx: TranslatorCtx<'a, 'b, 'a, 'tcx>,
                                                   cnum: CrateNum)
  -> (Arc<DefIdSet>, Arc<Vec<Arc<CodegenUnit<'tcx>>>>)
  where 'tcx: 'b,
{
  assert_eq!(cnum, LOCAL_CRATE);

  // normally, we start by collecting crate roots (non-generic items
  // to seed monomorphization).
  // We, however, already know the root: it's the root function passed to
  // the kernel_info intrinsic. Plus, we *only* want to codegen what is
  // actually used in the function.

  let root = Instance::mono(tcx.tcx, tcx.root);
  let mono_root = create_fn_mono_item(root);

  let mut visited = FxHashSet();

  {
    collect_items_rec(tcx, root, &mut visited);
  }

  let mono_items: DefIdSet = visited.into_inner()
    .iter()
    .filter_map(|mono_item| {
      match *mono_item {
        MonoItem::Fn(ref instance) => Some(instance.def_id()),
        MonoItem::Static(def_id) => Some(def_id),
        _ => None,
      }
    })
    .collect();
}

fn collect_items_rec<'a, 'b, 'tcx>(tcx: TranslatorCtx<'a, 'b, 'a, 'tcx>,
                                   start: MonoItem<'tcx>,
                                   visited: &mut FxHashSet<MonoItem<'tcx>>)
{
  if !visited.insert(start.clone()) {
    return;
  }
  debug!("BEGIN collect_items_rec({})", starting_point.to_string(tcx));

  let mut neighbors = Vec::new();

  match start {
    MonoItem::Static(def_id) => {
      let instance = Instance::mono(tcx.tcx, def_id);
      let ty = instance.ty(tcx);
      visit_drop_use(tcx, ty, true, &mut neighbors);

      let cid = GlobalId {
        instance,
        promoted: None,
      };
      let param_env = ParamEnv::reveal_all();
      if let Ok(val) = tcx.const_eval(param_env.and(cid)) {
        collect_const(tcx, val, instance.substs,
                      &mut neighbors);
      }
    },
    MonoItem::Fn(instance) => {
      // don't check recursion limits or type len limits: if they were
      // an issue, `rustc` targeting the host would have erred.
      collect_neighbours(tcx, instance, &mut neighbors);
    },
    MonoItem::GlobalAsm(..) => {

    },
  }
}

fn create_fn_mono_item<'a, 'tcx>(instance: Instance<'tcx>) -> MonoItem<'tcx> {
  debug!("create_fn_mono_item(instance={})", instance);
  MonoItem::Fn(instance)
}

fn visit_drop_use<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                            ty: Ty<'tcx>,
                            is_direct_call: bool,
                            output: &mut Vec<MonoItem<'tcx>>)
{
  let instance = monomorphize::resolve_drop_in_place(tcx, ty);
  visit_instance_use(tcx, instance, is_direct_call, output);
}
fn visit_fn_use<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                          ty: Ty<'tcx>,
                          is_direct_call: bool,
                          output: &mut Vec<MonoItem<'tcx>>)
{
  if let ty::TyFnDef(def_id, substs) = ty.sty {
    let instance = Instance::resolve(tcx, ParamEnv::reveal_all(),
                                     def_id, substs)
      .unwrap();
    visit_instance_use(tcx, instance, is_direct_call, output);
  } else {

  }
}

fn visit_instance_use<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                instance: ty::Instance<'tcx>,
                                is_direct_call: bool,
                                output: &mut Vec<MonoItem<'tcx>>)
{
  debug!("visit_item_use({:?}, is_direct_call={:?})", instance, is_direct_call);

  assert!(is_direct_call, "TODO: generate wrappers for sending calls to the queue");

  match instance.def {
    ty::InstanceDef::Intrinsic(def_id) => {
      if !is_direct_call {
        bug!("intrinsic {:?} being reified", def_id);
      }
    }
    ty::InstanceDef::Virtual(..) |
    ty::InstanceDef::DropGlue(_, None) => {
      // don't need to emit shim if we are calling directly.
      if !is_direct_call {
        output.push(create_fn_mono_item(instance));
      }
    }
    ty::InstanceDef::DropGlue(_, Some(_)) => {
      output.push(create_fn_mono_item(instance));
    }
    ty::InstanceDef::ClosureOnceShim { .. } |
    ty::InstanceDef::Item(..) |
    ty::InstanceDef::FnPtrShim(..) |
    ty::InstanceDef::CloneShim(..) => {
      output.push(create_fn_mono_item(instance));
    }
  }
}

fn collect_const<'a, 'b, 'tcx>(tcx: TranslatorCtx<'a, 'b, 'a, 'tcx>,
                               constant: &Const<'tcx>,
                               param_substs: &'tcx Substs<'tcx>,
                               output: &mut Vec<MonoItem<'tcx>>)
  where 'tcx: 'b,
{
  debug!("visiting const {:?}", *constant);

  let val = match constant.val {
    ConstValue::Unevaluated(def_id, substs) => {
      let param_env = ParamEnv::reveal_all();
      let substs = tcx.subst_and_normalize_erasing_regions(param_substs,
                                                           param_env,
                                                           &substs);
      let instance = Instance::resolve(tcx.tcx, param_env,
                                       def_id, substs)
        .unwrap();

      let cid = GlobalId {
        instance,
        promoted: None,
      };
      match tcx.const_eval(param_env.and(cid)) {
        Ok(val) => val.val,
        Err(err) => {
          let span = tcx.def_span(def_id);
          err.report_as_error(
            tcx.at(span),
            "constant evaluation error",
          );
          return;
        }
      }
    },
    _ => constant.val,
  };
  match val {
    ConstValue::Unevaluated(..) => bug!("const eval yielded unevaluated const"),
    ConstValue::ScalarPair(Scalar::Ptr(a), Scalar::Ptr(b)) => {
      collect_miri(tcx, a.alloc_id, output);
      collect_miri(tcx, b.alloc_id, output);
    }
    ConstValue::ScalarPair(_, Scalar::Ptr(ptr)) |
    ConstValue::ScalarPair(Scalar::Ptr(ptr), _) |
    ConstValue::Scalar(Scalar::Ptr(ptr)) =>
      collect_miri(tcx, ptr.alloc_id, output),
    ConstValue::ByRef(alloc, _offset) => {
      for &id in alloc.relocations.values() {
        collect_miri(tcx, id, output);
      }
    }
    _ => {},
  }
}
/// Scan the miri alloc in order to find function calls, closures, and drop-glue
fn collect_miri<'a, 'b, 'tcx>(tcx: TranslatorCtx<'a, 'b, 'a, 'tcx>,
                              alloc_id: AllocId,
                              output: &mut Vec<MonoItem<'tcx>>)
  where 'tcx: 'b,
{
  let alloc_type = tcx.alloc_map.lock().get(alloc_id);
  match alloc_type {
    Some(AllocType::Static(did)) => {
      let instance = Instance::mono(tcx, did);
      if should_monomorphize_locally(tcx, &instance) {
        trace!("collecting static {:?}", did);
        output.push(MonoItem::Static(did));
      }
    }
    Some(AllocType::Memory(alloc)) => {
      trace!("collecting {:?} with {:#?}", alloc_id, alloc);
      for &inner in alloc.relocations.values() {
        collect_miri(tcx, inner, output);
      }
    },
    Some(AllocType::Function(fn_instance)) => {
      if should_monomorphize_locally(tcx, &fn_instance) {
        trace!("collecting {:?} with {:#?}", alloc_id, fn_instance);
        output.push(create_fn_mono_item(fn_instance));
      }
    }
    None => bug!("alloc id without corresponding allocation: {}", alloc_id),
  }
}

/// Scan the MIR in order to find function calls, closures, and drop-glue
fn collect_neighbours<'a, 'b, 'tcx>(tcx: TranslatorCtx<'a, 'b, 'a, 'tcx>,
                                    instance: Instance<'tcx>,
                                    output: &mut Vec<MonoItem<'tcx>>)
  where 'tcx: 'b,
{
  let mir = tcx.instance_mir(instance.def);

  {
    let mut collector = MirNeighborCollector {
      tcx,
      mir: &mir,
      output,
      substs: instance.substs,
    };
    collector.visit_mir(&mir);
  }

  let param_env = ParamEnv::reveal_all();
  for i in 0..mir.promoted.len() {
    use rustc_data_structures::indexed_vec::Idx;
    let i = Promoted::new(i);
    let cid = GlobalId {
      instance,
      promoted: Some(i),
    };
    match tcx.const_eval(param_env.and(cid)) {
      Ok(val) => collect_const(tcx, val, instance.substs, output),
      Err(_) => {
        // would have erred on the host, ignore.
      },
    }
  }
}


struct MirNeighborCollector<'a, 'b, 'tcx>
  where 'tcx: 'b,
{
  tcx: TranslatorCtx<'a, 'b, 'a, 'tcx>,
  mir: &'b mir::Mir<'tcx>,
  output: &'b mut Vec<MonoItem<'tcx>>,
  substs: &'tcx Substs<'tcx>,
}

impl<'a, 'b, 'tcx> mir::visit::Visitor<'tcx> for MirNeighborCollector<'a, 'b, 'tcx>
  where 'tcx: 'b,
{

  fn visit_rvalue(&mut self, rvalue: &mir::Rvalue<'tcx>, location: Location) {
    debug!("visiting rvalue {:?}", *rvalue);

    match *rvalue {
      // When doing an cast from a regular pointer to a fat pointer, we
      // have to instantiate all methods of the trait being cast to, so we
      // can build the appropriate vtable.
      mir::Rvalue::Cast(mir::CastKind::Unsize, ref operand, target_ty) => {
        let target_ty = self.tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &target_ty,
        );
        let source_ty = operand.ty(self.mir, self.tcx);
        let source_ty = self.tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &source_ty,
        );
        let (source_ty, target_ty) = find_vtable_types_for_unsizing(self.tcx,
                                                                    source_ty,
                                                                    target_ty);
        // This could also be a different Unsize instruction, like
        // from a fixed sized array to a slice. But we are only
        // interested in things that produce a vtable.
        if target_ty.is_trait() && !source_ty.is_trait() {
          create_mono_items_for_vtable_methods(self.tcx,
                                               target_ty,
                                               source_ty,
                                               self.output);
        }
      }
      mir::Rvalue::Cast(mir::CastKind::ReifyFnPointer, ref operand, _) => {
        let fn_ty = operand.ty(self.mir, self.tcx);
        let fn_ty = self.tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &fn_ty,
        );
        visit_fn_use(self.tcx, fn_ty, false, &mut self.output);
      }
      mir::Rvalue::Cast(mir::CastKind::ClosureFnPointer, ref operand, _) => {
        let source_ty = operand.ty(self.mir, self.tcx);
        let source_ty = self.tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &source_ty,
        );
        match source_ty.sty {
          ty::TyClosure(def_id, substs) => {
            let instance = monomorphize::resolve_closure(self.tcx,
                                                         def_id,
                                                         substs,
                                                         ty::ClosureKind::FnOnce);
            self.output.push(create_fn_mono_item(instance));
          }
          _ => bug!(),
        }
      }
      mir::Rvalue::NullaryOp(mir::NullOp::Box, _) => {
        let tcx = self.tcx.tcx;
        let exchange_malloc_fn_def_id = tcx
          .lang_items()
          .require(ExchangeMallocFnLangItem)
          .unwrap_or_else(|e| tcx.sess.fatal(&e));
        let instance = Instance::mono(tcx, exchange_malloc_fn_def_id);
        self.output.push(create_fn_mono_item(instance));
      }
      _ => { /* not interesting */ }
    }

    self.super_rvalue(rvalue, location);
  }

  fn visit_const(&mut self, constant: &&'tcx ty::Const<'tcx>,
                 location: Location) {
    debug!("visiting const {:?} @ {:?}", *constant, location);

    collect_const(self.tcx, constant, self.substs, self.output);

    self.super_const(constant);
  }

  fn visit_terminator_kind(&mut self,
                           block: mir::BasicBlock,
                           kind: &mir::TerminatorKind<'tcx>,
                           location: Location) {
    debug!("visiting terminator {:?} @ {:?}", kind, location);

    let tcx = self.tcx.tcx;
    match *kind {
      mir::TerminatorKind::Call { ref func, .. } => {
        let callee_ty = func.ty(self.mir, tcx);
        let callee_ty = tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &callee_ty,
        );
        visit_fn_use(self.tcx, callee_ty, true, &mut self.output);
      }
      mir::TerminatorKind::Drop { ref location, .. } |
      mir::TerminatorKind::DropAndReplace { ref location, .. } => {
        let ty = location.ty(self.mir, self.tcx)
          .to_ty(self.tcx);
        let ty = tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &ty,
        );
        visit_drop_use(self.tcx, ty, true, self.output);
      }
      mir::TerminatorKind::Goto { .. } |
      mir::TerminatorKind::SwitchInt { .. } |
      mir::TerminatorKind::Resume |
      mir::TerminatorKind::Abort |
      mir::TerminatorKind::Return |
      mir::TerminatorKind::Unreachable |
      mir::TerminatorKind::Assert { .. } => {}
      mir::TerminatorKind::GeneratorDrop |
      mir::TerminatorKind::Yield { .. } |
      mir::TerminatorKind::FalseEdges { .. } |
      mir::TerminatorKind::FalseUnwind { .. } => bug!(),
    }

    self.super_terminator_kind(block, kind, location);
  }

  fn visit_static(&mut self,
                  static_: &mir::Static<'tcx>,
                  context: mir::visit::PlaceContext<'tcx>,
                  location: Location) {
    debug!("visiting static {:?} @ {:?}", static_.def_id, location);

    let tcx = self.tcx;
    let instance = Instance::mono(tcx, static_.def_id);
    self.output.push(MonoItem::Static(static_.def_id));

    self.super_static(static_, context, location);
  }
}