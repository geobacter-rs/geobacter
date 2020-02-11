
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
use rustc::mir::{self, Location, visit::Visitor, AssertMessage, Local, };
use rustc::mir::interpret::{AllocId, ConstValue, GlobalAlloc, Scalar,
                            ErrorHandled, };
use rustc::mir::mono::{MonoItem, };
use rustc::ty::adjustment::{CustomCoerceUnsized, PointerCast};
use rustc::ty::{self, TyCtxt, Instance, ParamEnv, subst::SubstsRef,
                TypeFoldable, InstanceDef, };
use rustc::ty::subst::{InternalSubsts, };
use rustc_data_structures::fx::FxHashSet;
use rustc_mir::monomorphize::{self, collector::InliningMap, };

use crate::stubbing::Stubber;
use crate::DriverData;

pub fn collect_items_rec<'tcx>(tcx: TyCtxt<'tcx>,
                               stubber: &Stubber,
                               dd: &dyn DriverData,
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
          let inst = if let InstanceDef::Intrinsic(_) = inst.def {
            // We still want our intrinsics to be defined
            inst
          } else {
            let inst = stubber.map_instance(tcx, dd, inst);

            if tcx.is_foreign_item(inst.def_id()) {
              // Exclude extern "X" { } stuff, but only if the
              // stubber doesn't handle it.
              return;
            }

            inst
          };

          MonoItem::Fn(inst)
        },
        _ => item,
      };

      neighbors.push(item);
    };

    match start {
      MonoItem::Static(def_id) => {
        let instance = Instance::mono(tcx, def_id);
        let ty = instance.monomorphic_ty(tcx);
        visit_drop_use(tcx, ty, true, &mut output);

        if let Ok(val) = tcx.const_eval_poly(def_id) {
          collect_const(tcx, val, InternalSubsts::empty(),
                        &mut output);
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
    collect_items_rec(tcx, stubber, dd,
                      neighbour, visited,
                      inlining_map);
  }

  //info!("END collect_items_rec({})", start.to_string(tcx));
}

fn record_accesses<'tcx>(_tcx: TyCtxt<'tcx>,
                         caller: MonoItem<'tcx>,
                         callees: &[MonoItem<'tcx>],
                         inlining_map: &mut InliningMap<'tcx>) {
  let accesses = callees.into_iter()
    .map(|mono_item| {
      (*mono_item, true)
    })
    .collect::<Vec<_>>();

  inlining_map.record_accesses(caller, &accesses);
}

fn visit_drop_use<'tcx, F>(tcx: TyCtxt<'tcx>,
                           ty: ty::Ty<'tcx>,
                           is_direct_call: bool,
                           output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  let instance = Instance::resolve_drop_in_place(tcx, ty);
  visit_instance_use(tcx, instance, is_direct_call, output);
}
fn visit_fn_use<'tcx, F>(tcx: TyCtxt<'tcx>,
                         ty: ty::Ty<'tcx>,
                         is_direct_call: bool,
                         output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  if let ty::FnDef(def_id, substs) = ty.kind {
    let instance = Instance::resolve(tcx, ParamEnv::reveal_all(),
                                     def_id, substs)
      .unwrap();
    visit_instance_use(tcx, instance, is_direct_call, output);
  }
}

fn visit_instance_use<'tcx, F>(tcx: TyCtxt<'tcx>,
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
    ty::InstanceDef::ReifyShim(..) |
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
fn find_vtable_types_for_unsizing<'tcx>(tcx: TyCtxt<'tcx>,
                                        source_ty: ty::Ty<'tcx>,
                                        target_ty: ty::Ty<'tcx>)
  -> (ty::Ty<'tcx>, ty::Ty<'tcx>) {
  let ptr_vtable = |inner_source: ty::Ty<'tcx>, inner_target: ty::Ty<'tcx>| {
    let param_env = ty::ParamEnv::reveal_all();
    let type_has_metadata = |ty: ty::Ty<'tcx>| -> bool {
      if ty.is_sized(tcx.at(rustc_span::DUMMY_SP), param_env) {
        return false;
      }
      let tail = tcx.struct_tail_erasing_lifetimes(ty, param_env);
      match tail.kind {
        ty::Foreign(..) => false,
        ty::Str | ty::Slice(..) | ty::Dynamic(..) => true,
        _ => bug!("unexpected unsized tail: {:?}", tail),
      }
    };
    if type_has_metadata(inner_source) {
      (inner_source, inner_target)
    } else {
      tcx.struct_lockstep_tails_erasing_lifetimes(inner_source, inner_target, param_env)
    }
  };

  match (&source_ty.kind, &target_ty.kind) {
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

pub fn create_fn_mono_item(instance: Instance<'_>) -> MonoItem<'_> {
  debug!("create_fn_mono_item(instance={})", instance);
  MonoItem::Fn(instance)
}

/// Creates a `MonoItem` for each method that is referenced by the vtable for
/// the given trait/impl pair. We don't end up codegen-ing these, but we need
/// to discover them so the host accelerator can setup it's index table with
/// these functions as entries.
fn create_mono_items_for_vtable_methods<'tcx, F>(tcx: TyCtxt<'tcx>,
                                                 trait_ty: ty::Ty<'tcx>,
                                                 impl_ty: ty::Ty<'tcx>,
                                                 output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  assert!(!trait_ty.needs_subst() && !trait_ty.has_escaping_bound_vars() &&
    !impl_ty.needs_subst() && !impl_ty.has_escaping_bound_vars());

  if let ty::Dynamic(ref trait_ty, ..) = trait_ty.kind {
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
        .map(|instance| create_fn_mono_item(instance) );
      for method in methods {
        output(method);
      }
    }
    // Also add the destructor
    visit_drop_use(tcx, impl_ty, false, output);
  }
}

fn collect_const<'tcx, F>(tcx: TyCtxt<'tcx>,
                          constant: &'tcx ty::Const<'tcx>,
                          param_substs: SubstsRef<'tcx>,
                          output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  debug!("visiting const {:?}", constant);

  let param_env = ty::ParamEnv::reveal_all();
  let substituted_constant = tcx.subst_and_normalize_erasing_regions(
    param_substs,
    param_env,
    &constant,
  );

  match substituted_constant.val {
    ty::ConstKind::Value(ConstValue::Scalar(Scalar::Ptr(ptr))) => {
      collect_miri(tcx, ptr.alloc_id, output)
    }
    ty::ConstKind::Value(ConstValue::Slice { data: alloc, start: _, end: _ })
    | ty::ConstKind::Value(ConstValue::ByRef { alloc, .. }) => {
      for &((), id) in alloc.relocations().values() {
        collect_miri(tcx, id, output);
      }
    }
    ty::ConstKind::Unevaluated(def_id, substs, promoted) => {
      match tcx.const_eval_resolve(param_env, def_id, substs, promoted, None) {
        Ok(val) => collect_const(tcx, val, param_substs, output),
        Err(ErrorHandled::Reported) => {}
        Err(ErrorHandled::TooGeneric) => {
          span_bug!(tcx.def_span(def_id), "collection encountered polymorphic constant",)
        }
      }
    }
    _ => {}
  }
}

/// Scan the miri alloc in order to find function calls, closures, and drop-glue
fn collect_miri<'tcx, F>(tcx: TyCtxt<'tcx>,
                         alloc_id: AllocId,
                         output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  let alloc_type = tcx.alloc_map.lock().get(alloc_id);
  match alloc_type {
    Some(GlobalAlloc::Static(did)) => {
      trace!("collecting static {:?}", did);
      output(MonoItem::Static(did));
    },
    Some(GlobalAlloc::Memory(alloc)) => {
      trace!("collecting {:?} with {:#?}", alloc_id, alloc);
      for &((), inner) in alloc.relocations().values() {
        collect_miri(tcx,inner, output);
      }
    },
    Some(GlobalAlloc::Function(fn_instance)) => {
      trace!("collecting {:?} with {:#?}", alloc_id, fn_instance);
      output(create_fn_mono_item(fn_instance));
    },
    None => bug!("alloc id without corresponding allocation: {}", alloc_id),
  }
}

/// Scan the MIR in order to find function calls, closures, and drop-glue
fn collect_neighbours<'tcx, F, G>(tcx: TyCtxt<'tcx>,
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
        tcx.custom_intrinsic_mir(instance)
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
    ty::InstanceDef::ReifyShim(..) |
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
      collector.visit_body(mir.unwrap_read_only());
    }
  }
}


struct MirNeighborCollector<'a, 'tcx, F, G>
  where F: FnMut(MonoItem<'tcx>),
        G: FnMut(MonoItem<'tcx>),
{
  tcx: TyCtxt<'tcx>,
  mir: &'tcx mir::Body<'tcx>,
  /// Some day, we might be able to support indirect functions calls, even if it's emulated
  /// by something like device side enqueue.
  _indirect: &'a mut F,
  output: &'a mut G,
  substs: SubstsRef<'tcx>,
}

impl<'a, 'tcx, F, G> mir::visit::Visitor<'tcx> for MirNeighborCollector<'a, 'tcx, F, G>
  where F: FnMut(MonoItem<'tcx>),
        G: FnMut(MonoItem<'tcx>),
{

  fn visit_rvalue(&mut self, rvalue: &mir::Rvalue<'tcx>, location: Location) {
    debug!("visiting rvalue {:?}", *rvalue);

    match *rvalue {
      // When doing an cast from a regular pointer to a fat pointer, we
      // have to instantiate all methods of the trait being cast to, so we
      // can build the appropriate vtable.
      mir::Rvalue::Cast(
        mir::CastKind::Pointer(PointerCast::Unsize), ref operand, target_ty
      ) => {
        let origin_target_ty = self.tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &target_ty,
        );
        let source_ty = operand.ty(self.mir, self.tcx);
        let origin_source_ty = self.tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &source_ty,
        );
        let (source_ty, target_ty) = find_vtable_types_for_unsizing(self.tcx,
                                                                    origin_source_ty,
                                                                    origin_target_ty);
        // This could also be a different Unsize instruction, like
        // from a fixed sized array to a slice. But we are only
        // interested in things that produce a vtable.
        trace!("possible vtable: target {:?}, src {:?}", target_ty, source_ty);
        if target_ty.is_trait() && !source_ty.is_trait() {
          trace!("(collection vtable methods...)");
          create_mono_items_for_vtable_methods(self.tcx,
                                               target_ty,
                                               source_ty,
                                               self.output);
        }
      },
      mir::Rvalue::Cast(
        mir::CastKind::Pointer(PointerCast::ReifyFnPointer), ref operand, _,
      ) => {
        let fn_ty = operand.ty(self.mir, self.tcx);
        let fn_ty = self.tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &fn_ty,
        );
        trace!("reify-fn-pointer cast: {:?}", fn_ty);
        visit_fn_use(self.tcx, fn_ty, false, self.output);
      },
      mir::Rvalue::Cast(
        mir::CastKind::Pointer(PointerCast::ClosureFnPointer(_)), ref operand, _
      ) => {
        let source_ty = operand.ty(self.mir, self.tcx);
        let source_ty = self.tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &source_ty,
        );
        match source_ty.kind {
          ty::Closure(def_id, substs) => {
            let instance = Instance::resolve_closure(self.tcx,
                                                     def_id,
                                                     substs,
                                                     ty::ClosureKind::FnOnce);
            (self.output)(create_fn_mono_item(instance));
          },
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

  fn visit_const(&mut self, constant: &&'tcx ty::Const<'tcx>,
                 location: Location) {
    debug!("visiting const {:?} @ {:?}", *constant, location);

    collect_const(self.tcx, constant, self.substs, self.output);

    self.super_const(constant);
  }

  fn visit_terminator_kind(&mut self,
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
        let ty = location.ty(self.mir, tcx).ty;
        let ty = tcx.subst_and_normalize_erasing_regions(
          self.substs,
          ty::ParamEnv::reveal_all(),
          &ty,
        );
        visit_drop_use(tcx, ty, true, self.output);
      },
      mir::TerminatorKind::Assert { ref msg, .. } => {
        // This differs from the vanilla driver:
        // it doesn't explicitly collect these lang items. This isn't
        // a problem there because these items get classified as roots
        // and so are always codegen-ed, and thus are always available
        // to downstream crates.
        // For us, however, these lang items need to get inserted into
        // the runtime codegen module.
        match msg {
          &AssertMessage::BoundsCheck { .. } => {
            let li = lang_items::PanicBoundsCheckFnLangItem;
            let def_id = tcx.require_lang_item(li, None);
            let inst = Instance::mono(tcx, def_id);
            (self.output)(create_fn_mono_item(inst));
          },
          _ => {
            let li = lang_items::PanicFnLangItem;
            let def_id = tcx.require_lang_item(li, None);
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

    self.super_terminator_kind(kind, location);
  }

  fn visit_place_base(
    &mut self,
    _place_local: &Local,
    _context: mir::visit::PlaceContext,
    _location: Location,
  ) {
  }
}
