
/// Collection code common to the compiler driver and the runtime
/// codegenner. This collects items in a slightly different way to the
/// unmodified driver, mostly involving pruning functions and statics
/// which aren't used uniformly, ie virtual dispatch. Additionally,
/// `panic!` will likely never be supported, so we stub the panic entry
/// function with one that aborts.
/// TODO need to check for global variables which are not wrapped in
/// lang item types. Const and immutable (and w/o interior mut) statics
/// can be allowed.

use rustc::middle::lang_items::{self, ExchangeMallocFnLangItem, };
use rustc::mir::{self, Location, Promoted, visit::Visitor, };
use rustc::mir::interpret::{AllocId, ConstValue, GlobalId, AllocKind,
                            Scalar, EvalErrorKind, ErrorHandled, };
use rustc::mir::mono::{MonoItem, };
use rustc::ty::adjustment::CustomCoerceUnsized;
use rustc::ty::{self, TyCtxt, Instance, ParamEnv, subst::Substs,
                TypeFoldable, };
use rustc::util::nodemap::{FxHashSet, };
use rustc_mir::monomorphize::{self, collector::InliningMap, };
use rustc_data_structures::indexed_vec::Idx;

use crate::stubbing::Stubber;
use crate::DefIdFromKernelId;

pub fn collect_items_rec<'tcx>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                               stubber: &Stubber,
                               kid_did: &dyn DefIdFromKernelId,
                               start: MonoItem<'tcx>,
                               visited: &mut FxHashSet<MonoItem<'tcx>>,
                               inlining_map: &mut Option<InliningMap<'tcx>>)
{
  if !visited.insert(start.clone()) {
    return;
  }
  //info!("BEGIN collect_items_rec({})", start.to_string(tcx));

  let mut indirect_neighbors = Vec::new();
  let mut neighbors = Vec::new();

  {
    let mut output_indirect = |item: MonoItem<'tcx>| {
      indirect_neighbors.push(item);
    };
    let mut output = |item: MonoItem<'tcx>| {

      let item = match item {
        MonoItem::Fn(inst) => {
          MonoItem::Fn(stubber.map_instance(tcx, kid_did, inst))
        },
        _ => item,
      };

      neighbors.push(item);
    };

    match start {
      MonoItem::Static(def_id) => {
        let instance = Instance::mono(tcx, def_id);
        let ty = instance.ty(tcx);
        visit_drop_use(tcx, ty, true, &mut output);

        let cid = GlobalId {
          instance,
          promoted: None,
        };
        let param_env = ParamEnv::reveal_all();
        if let Ok(val) = tcx.const_eval(param_env.and(cid)) {
          collect_const(tcx, val, &mut output);
        }
      },
      MonoItem::Fn(instance) => {
        // don't check recursion limits or type len limits: if they were
        // an issue, `rustc` targeting the host would have erred.
        collect_neighbours(tcx, instance,
                           &mut output_indirect,
                           &mut output);
      },
      MonoItem::GlobalAsm(..) => {
        unimplemented!();
      },
    }
  }

  if let Some(inlining_map) = inlining_map.as_mut() {
    record_accesses(tcx,
                    start,
                    &neighbors[..],
                    inlining_map);
  }

  for neighbour in neighbors {
    collect_items_rec(tcx, stubber, kid_did,
                      neighbour, visited,
                      inlining_map);
  }

  //info!("END collect_items_rec({})", start.to_string(tcx));
}

fn record_accesses<'a, 'tcx>(_tcx: TyCtxt<'a, 'tcx, 'tcx>,
                             caller: MonoItem<'tcx>,
                             callees: &[MonoItem<'tcx>],
                             inlining_map: &mut InliningMap<'tcx>) {
  let accesses = callees.into_iter()
    .map(|mono_item| {
      (*mono_item, true)
    });

  inlining_map.record_accesses(caller, accesses);
}

fn visit_drop_use<'a, 'tcx, F>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                               ty: ty::Ty<'tcx>,
                               is_direct_call: bool,
                               output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  let instance = monomorphize::resolve_drop_in_place(tcx, ty);
  visit_instance_use(tcx, instance, is_direct_call, output);
}
fn visit_fn_use<'a, 'tcx, F>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                             ty: ty::Ty<'tcx>,
                             is_direct_call: bool,
                             output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  if let ty::FnDef(def_id, substs) = ty.sty {
    let instance = Instance::resolve(tcx, ParamEnv::reveal_all(),
                                     def_id, substs)
      .unwrap();
    visit_instance_use(tcx, instance, is_direct_call, output);
  } else {
    warn!("visit_fn_use: fn type {:?} not implemented!", ty);
  }
}

fn visit_instance_use<'a, 'tcx, F>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   is_direct_call: bool,
                                   output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  debug!("visit_item_use({:?}, is_direct_call={:?})", instance, is_direct_call);

  //assert!(is_direct_call, "TODO: generate wrappers for sending calls to the queue");

  match instance.def {
    ty::InstanceDef::Intrinsic(def_id) => {
      if !is_direct_call {
        bug!("intrinsic {:?} being reified", def_id);
      }
      if let Some(_mir) = tcx.custom_intrinsic_mir(instance) {
        output(create_fn_mono_item(instance));
      }
    },
    ty::InstanceDef::VtableShim(..) |
    ty::InstanceDef::Virtual(..) |
    ty::InstanceDef::DropGlue(_, None) => {
      // don't need to emit shim if we are calling directly.
      if !is_direct_call {
        output(create_fn_mono_item(instance));
      }
    },
    ty::InstanceDef::DropGlue(_, Some(_)) => {
      output(create_fn_mono_item(instance));
    },
    ty::InstanceDef::ClosureOnceShim { .. } |
    ty::InstanceDef::Item(..) |
    ty::InstanceDef::FnPtrShim(..) |
    ty::InstanceDef::CloneShim(..) => {
      output(create_fn_mono_item(instance));
    },
  }
}

/// For given pair of source and target type that occur in an unsizing coercion,
/// this function finds the pair of types that determines the vtable linking
/// them.
///
/// For example, the source type might be `&SomeStruct` and the target type\
/// might be `&SomeTrait` in a cast like:
///
/// let src: &SomeStruct = ...;
/// let target = src as &SomeTrait;
///
/// Then the output of this function would be (SomeStruct, SomeTrait) since for
/// constructing the `target` fat-pointer we need the vtable for that pair.
///
/// Things can get more complicated though because there's also the case where
/// the unsized type occurs as a field:
///
/// ```rust
/// struct ComplexStruct<T: ?Sized> {
///    a: u32,
///    b: f64,
///    c: T
/// }
/// ```
///
/// In this case, if `T` is sized, `&ComplexStruct<T>` is a thin pointer. If `T`
/// is unsized, `&SomeStruct` is a fat pointer, and the vtable it points to is
/// for the pair of `T` (which is a trait) and the concrete type that `T` was
/// originally coerced from:
///
/// let src: &ComplexStruct<SomeStruct> = ...;
/// let target = src as &ComplexStruct<SomeTrait>;
///
/// Again, we want this `find_vtable_types_for_unsizing()` to provide the pair
/// `(SomeStruct, SomeTrait)`.
///
/// Finally, there is also the case of custom unsizing coercions, e.g. for
/// smart pointers such as `Rc` and `Arc`.
fn find_vtable_types_for_unsizing<'tcx>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                                        source_ty: ty::Ty<'tcx>,
                                        target_ty: ty::Ty<'tcx>)
  -> (ty::Ty<'tcx>, ty::Ty<'tcx>) {
  let ptr_vtable = |inner_source: ty::Ty<'tcx>, inner_target: ty::Ty<'tcx>| {
    let type_has_metadata = |ty: ty::Ty<'tcx>| -> bool {
      use syntax_pos::DUMMY_SP;
      if ty.is_sized(tcx.at(DUMMY_SP), ty::ParamEnv::reveal_all()) {
        return false;
      }
      let tail = tcx.struct_tail(ty);
      match tail.sty {
        ty::Foreign(..) => false,
        ty::Str | ty::Slice(..) | ty::Dynamic(..) => true,
        _ => bug!("unexpected unsized tail: {:?}", tail.sty),
      }
    };
    if type_has_metadata(inner_source) {
      (inner_source, inner_target)
    } else {
      tcx.struct_lockstep_tails(inner_source, inner_target)
    }
  };

  match (&source_ty.sty, &target_ty.sty) {
    (&ty::Ref(_, a, _),
      &ty::Ref(_, b, _)) |
    (&ty::Ref(_, a, _),
      &ty::RawPtr(ty::TypeAndMut { ty: b, .. })) |
    (&ty::RawPtr(ty::TypeAndMut { ty: a, .. }),
      &ty::RawPtr(ty::TypeAndMut { ty: b, .. })) => {
      ptr_vtable(a, b)
    },
    (&ty::Adt(def_a, _), &ty::Adt(def_b, _)) if def_a.is_box() && def_b.is_box() => {
      ptr_vtable(source_ty.boxed_ty(), target_ty.boxed_ty())
    },

    (&ty::Adt(source_adt_def, source_substs),
      &ty::Adt(target_adt_def, target_substs)) => {
      assert_eq!(source_adt_def, target_adt_def);

      let kind =
        monomorphize::custom_coerce_unsize_info(tcx, source_ty, target_ty);

      let coerce_index = match kind {
        CustomCoerceUnsized::Struct(i) => i
      };

      let source_fields = &source_adt_def.non_enum_variant().fields;
      let target_fields = &target_adt_def.non_enum_variant().fields;

      assert!(coerce_index < source_fields.len() &&
        source_fields.len() == target_fields.len());

      find_vtable_types_for_unsizing(tcx,
                                     source_fields[coerce_index].ty(tcx,
                                                                    source_substs),
                                     target_fields[coerce_index].ty(tcx,
                                                                    target_substs))
    },
    _ => bug!("find_vtable_types_for_unsizing: invalid coercion {:?} -> {:?}",
                  source_ty,
                  target_ty)
  }
}

pub fn create_fn_mono_item<'a, 'tcx>(instance: Instance<'tcx>) -> MonoItem<'tcx> {
  debug!("create_fn_mono_item(instance={})", instance);
  MonoItem::Fn(instance)
}

/// Creates a `MonoItem` for each method that is referenced by the vtable for
/// the given trait/impl pair. We don't end up codegen-ing these, but we need
/// to discover them so the host accelerator can setup it's index table with
/// these functions as entries.
fn create_mono_items_for_vtable_methods<'tcx, F>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                                                 trait_ty: ty::Ty<'tcx>,
                                                 impl_ty: ty::Ty<'tcx>,
                                                 output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  assert!(!trait_ty.needs_subst() && !trait_ty.has_escaping_bound_vars() &&
    !impl_ty.needs_subst() && !impl_ty.has_escaping_bound_vars());

  if let ty::Dynamic(ref trait_ty, ..) = trait_ty.sty {
    if let Some(principal) = trait_ty.principal() {
      let poly_trait_ref = principal.with_self_ty(tcx, impl_ty);
      assert!(!poly_trait_ref.has_escaping_bound_vars());

      // Walk all methods of the trait, including those of its supertraits
      let methods = tcx.vtable_methods(poly_trait_ref);
      let methods = methods.iter().cloned()
        .filter_map(|method| method)
        .map(|(def_id, substs)| ty::Instance::resolve_for_vtable(
          tcx,
          ty::ParamEnv::reveal_all(),
          def_id,
          substs).unwrap())
        .map(|instance| create_fn_mono_item(instance));
      for method in methods {
        output(method);
      }
    }
    // Also add the destructor
    visit_drop_use(tcx, impl_ty, false, output);
  }
}

fn collect_lazy_const<'a, 'tcx, F>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                   constant: &ty::LazyConst<'tcx>,
                                   param_substs: &'tcx Substs<'tcx>,
                                   output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  let (def_id, substs) = match *constant {
    ty::LazyConst::Evaluated(c) => return collect_const(tcx, c, output),
    ty::LazyConst::Unevaluated(did, substs) => (did, substs),
  };
  let param_env = ty::ParamEnv::reveal_all();
  let substs = tcx.subst_and_normalize_erasing_regions(
    param_substs,
    param_env,
    &substs,
  );
  let instance = ty::Instance::resolve(tcx,
                                       param_env,
                                       def_id,
                                       substs).unwrap();

  let cid = GlobalId {
    instance,
    promoted: None,
  };
  match tcx.const_eval(param_env.and(cid)) {
    Ok(val) => collect_const(tcx, val, output),
    Err(ErrorHandled::Reported) => {},
    Err(ErrorHandled::TooGeneric) => span_bug!(
            tcx.def_span(def_id), "collection encountered polymorphic constant",
        ),
  }
}

fn collect_const<'tcx, F>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                          constant: ty::Const<'tcx>,
                          output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  debug!("visiting const {:?}", constant);
  match constant.val {
    ConstValue::ScalarPair(Scalar::Ptr(a), Scalar::Ptr(b)) => {
      collect_miri(tcx, a.alloc_id, output);
      collect_miri(tcx, b.alloc_id, output);
    },
    ConstValue::ScalarPair(_, Scalar::Ptr(ptr)) |
    ConstValue::ScalarPair(Scalar::Ptr(ptr), _) |
    ConstValue::Scalar(Scalar::Ptr(ptr)) =>
      collect_miri(tcx, ptr.alloc_id, output),
    ConstValue::ByRef(_id, alloc, _offset) => {
      for &((), id) in alloc.relocations.values() {
        collect_miri(tcx, id, output);
      }
    },
    _ => {},
  }
}
/// Scan the miri alloc in order to find function calls, closures, and drop-glue
fn collect_miri<'tcx, F>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                         alloc_id: AllocId,
                         output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  let alloc_type = tcx.alloc_map.lock().get(alloc_id);
  match alloc_type {
    Some(AllocKind::Static(did)) => {
      trace!("collecting static {:?}", did);
      output(MonoItem::Static(did));
    },
    Some(AllocKind::Memory(alloc)) => {
      trace!("collecting {:?} with {:#?}", alloc_id, alloc);
      for &((), inner) in alloc.relocations.values() {
        collect_miri(tcx,inner, output);
      }
    },
    Some(AllocKind::Function(fn_instance)) => {
      trace!("collecting {:?} with {:#?}", alloc_id, fn_instance);
      warn!("TODO function is ignored: {:?}", fn_instance);
      //output(create_fn_mono_item(fn_instance));
    },
    None => bug!("alloc id without corresponding allocation: {}", alloc_id),
  }
}

/// Scan the MIR in order to find function calls, closures, and drop-glue
fn collect_neighbours<'tcx, F, G>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                                  instance: Instance<'tcx>,
                                  indirect: &mut F,
                                  output: &mut G)
  where F: FnMut(MonoItem<'tcx>),
        G: FnMut(MonoItem<'tcx>),
{
  // Some special symbols (like the allocator functions) don't have MIR
  // available (these symbols typically rely on the linker to work).
  // Runtime codegen uses stubbed functions for these, but we still need
  // to "collect" these for the host.
  let mir_opt = match instance.def {
    ty::InstanceDef::Item(did) => {
      if tcx.is_mir_available(did) {
        Some(tcx.optimized_mir(did))
      } else {
        None
      }
    },
    ty::InstanceDef::Intrinsic(..) => {
      if let Some(mir) = tcx.custom_intrinsic_mir(instance) {
        Some(mir)
      } else {
        Some(tcx.mir_shims(instance.def))
      }
    },
    ty::InstanceDef::VtableShim(..) |
    ty::InstanceDef::FnPtrShim(..) |
    ty::InstanceDef::Virtual(..) |
    ty::InstanceDef::ClosureOnceShim { .. } |
    ty::InstanceDef::DropGlue(..) |
    ty::InstanceDef::CloneShim(..) => {
      Some(tcx.mir_shims(instance.def))
    },
  };

  if let Some(mir) = mir_opt {
    {
      let mut collector = MirNeighborCollector {
        tcx,
        mir: &mir,
        _indirect: indirect,
        output,
        substs: instance.substs,
      };
      collector.visit_mir(&mir);
    }

    let param_env = ParamEnv::reveal_all();
    for i in 0..mir.promoted.len() {
      let i = Promoted::new(i);
      let cid = GlobalId {
        instance,
        promoted: Some(i),
      };
      match tcx.const_eval(param_env.and(cid)) {
        Ok(val) => collect_const(tcx, val, output),
        Err(_) => {
          // would have erred on the host, ignore.
          unreachable!();
        },
      }
    }
  }
}


struct MirNeighborCollector<'b, 'tcx, F, G>
  where 'tcx: 'b,
        F: FnMut(MonoItem<'tcx>),
        G: FnMut(MonoItem<'tcx>),
{
  tcx: TyCtxt<'b, 'tcx, 'tcx>,
  mir: &'b mir::Mir<'tcx>,
  /// Some day, we might be able to support indirect functions calls, even if it's emulated
  /// by something like device side enqueue.
  _indirect: &'b mut F,
  output: &'b mut G,
  substs: &'tcx Substs<'tcx>,
}

impl<'b, 'tcx, F, G> mir::visit::Visitor<'tcx> for MirNeighborCollector<'b, 'tcx, F, G>
  where 'tcx: 'b,
        F: FnMut(MonoItem<'tcx>),
        G: FnMut(MonoItem<'tcx>),
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
      },
      mir::Rvalue::Cast(mir::CastKind::ReifyFnPointer, ref operand, _) => {
        let fn_ty = operand.ty(self.mir, self.tcx);
        let fn_ty = self.tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &fn_ty,
        );
        warn!("TODO: reify-fn-pointer cast: {:?}", fn_ty);
        //visit_fn_use(self.tcx.tcx, fn_ty, false, &mut self.output);
      },
      mir::Rvalue::Cast(mir::CastKind::ClosureFnPointer, ref operand, _) => {
        let source_ty = operand.ty(self.mir, self.tcx);
        let source_ty = self.tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &source_ty,
        );
        match source_ty.sty {
          ty::Closure(def_id, substs) => {
            let instance = monomorphize::resolve_closure(self.tcx,
                                                         def_id,
                                                         substs,
                                                         ty::ClosureKind::FnOnce);
            (self.output)(create_fn_mono_item(instance));
          }
          _ => bug!(),
        }
      },
      mir::Rvalue::NullaryOp(mir::NullOp::Box, _) => {
        let tcx = self.tcx;
        let exchange_malloc_fn_def_id = tcx
          .lang_items()
          .require(ExchangeMallocFnLangItem)
          .unwrap_or_else(|e| tcx.sess.fatal(&e));
        let instance = Instance::mono(tcx, exchange_malloc_fn_def_id);
        (self.output)(create_fn_mono_item(instance));
      },
      _ => { /* not interesting */ },
    }

    self.super_rvalue(rvalue, location);
  }

  fn visit_const(&mut self, constant: &&'tcx ty::LazyConst<'tcx>,
                 location: Location) {
    debug!("visiting const {:?} @ {:?}", *constant, location);

    collect_lazy_const(self.tcx, constant, self.substs, self.output);

    self.super_const(constant);
  }

  fn visit_terminator_kind(&mut self,
                           block: mir::BasicBlock,
                           kind: &mir::TerminatorKind<'tcx>,
                           location: Location) {
    debug!("visiting terminator {:?} @ {:?}", kind, location);

    let tcx = self.tcx;
    match *kind {
      mir::TerminatorKind::Call { ref func, .. } => {
        let callee_ty = func.ty(self.mir, tcx);
        let callee_ty = tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &callee_ty,
        );
        visit_fn_use(tcx, callee_ty, true, &mut self.output);
      },
      mir::TerminatorKind::Drop { ref location, .. } |
      mir::TerminatorKind::DropAndReplace { ref location, .. } => {
        let ty = location.ty(self.mir, tcx)
          .to_ty(tcx);
        let ty = tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &ty,
        );
        visit_drop_use(tcx, ty, true, self.output);
      },
      mir::TerminatorKind::Assert { ref msg, .. } => {
        match msg {
          &EvalErrorKind::BoundsCheck { .. } => {
            let li = lang_items::PanicBoundsCheckFnLangItem;
            let def_id = tcx.require_lang_item(li);
            let inst = Instance::mono(tcx, def_id);
            (self.output)(create_fn_mono_item(inst));
          },
          _ => {
            let li = lang_items::PanicFnLangItem;
            let def_id = tcx.require_lang_item(li);
            let inst = Instance::mono(tcx, def_id);
            (self.output)(create_fn_mono_item(inst));
          },
        }
      },
      mir::TerminatorKind::Goto { .. } |
      mir::TerminatorKind::SwitchInt { .. } |
      mir::TerminatorKind::Resume |
      mir::TerminatorKind::Abort |
      mir::TerminatorKind::Return |
      mir::TerminatorKind::Unreachable => {}
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

    // We use foreign items for things that are defined by the Vulkan impl
    // Also, we can't have LLVM use them for optimization.
    if !self.tcx.is_foreign_item(static_.def_id) {
      warn!("TODO: inject the contents of the statics using their value on the host: {:?}",
            Instance::mono(self.tcx, static_.def_id));
      (self.output)(MonoItem::Static(static_.def_id));
    }

    self.super_static(static_, context, location);
  }
}
