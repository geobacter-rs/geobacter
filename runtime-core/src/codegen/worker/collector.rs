
//! Figure out what needs codegening. Some of this is redundant, particularly
//! the evaluation of statics, in fact we should copy *exactly* the output of
//! the host evaluation. This collects items in a slightly different way to the
//! unmodified driver
//!
//! This contains code from the relevant parts of `rustc`.

use rustc_data_structures::fx::{FxHashMap, FxHashSet};
use rustc_errors::ErrorReported;
use rustc_hir::def_id::{CrateNum, LOCAL_CRATE, DefIdSet};
use rustc_hir::lang_items::LangItem;
use rustc_middle::mir::{self, Location, visit::Visitor, AssertMessage, Local};
use rustc_middle::mir::interpret::{AllocId, ConstValue, GlobalAlloc, Scalar,
                                   ErrorHandled, };
use rustc_middle::mir::mono::{CodegenUnit, MonoItem, Linkage, Visibility};
use rustc_middle::ty::{self, TyCtxt, Instance, ParamEnv, TypeFoldable, InstanceDef};
use rustc_middle::ty::adjustment::{CustomCoerceUnsized, PointerCast};
use rustc_mir::monomorphize::{self, collector::InliningMap,
                              partitioning::partition};

use crate::codegen::PlatformCodegen;
use super::driver_data::DriverData;

pub fn collect_and_partition_mono_items<'tcx, P>(tcx: TyCtxt<'tcx>,
                                                 dd: &'tcx DriverData<'tcx, P>,
                                                 cnum: CrateNum)
  -> (&'tcx DefIdSet, &'tcx [CodegenUnit<'tcx>])
  where P: PlatformCodegen,
{
  assert_eq!(cnum, LOCAL_CRATE);

  // normally, we start by collecting crate roots (non-generic items
  // to seed monomorphization).
  // We, however, already know the root: it's the root function passed to
  // the kernel_id intrinsic. Plus, we *only* want to codegen what is
  // actually used in the function.

  let root = dd.root().instance;
  let mono_root = create_fn_mono_item(root);

  let mut visited: FxHashSet<_> = Default::default();
  let mut inlining_map = Some(InliningMap::new());

  collect_items_rec(tcx, mono_root,
                    &mut visited,
                    &mut inlining_map);
  let inlining_map = inlining_map.unwrap();

  let items = visited;
  let mut units = partition(tcx, &mut items.iter().cloned(),
                            tcx.sess.codegen_units(),
                            &inlining_map);

  // force the root to have an external linkage:
  for unit in units.iter_mut() {
    for mut item in unit.items_mut().iter_mut() {
      if item.0 == &mono_root {
        (item.1).0 = Linkage::External;
      } else {
        (item.1).0 = Linkage::Internal;
      }
      (item.1).1 = Visibility::Default;
    }
  }

  let units: Vec<CodegenUnit> = units;

  let mono_items: DefIdSet = items.iter()
    .filter_map(|mono_item| {
      match *mono_item {
        MonoItem::Fn(ref instance) => Some(instance.def_id()),
        MonoItem::Static(def_id) => Some(def_id),
        _ => None,
      }
    })
    .collect();

  if tcx.sess.opts.debugging_opts.print_mono_items.is_some() {
    let mut item_to_cgus: FxHashMap<_, Vec<_>> = Default::default();

    for cgu in &units {
      for (&mono_item, &linkage) in cgu.items() {
        item_to_cgus.entry(mono_item)
          .or_default()
          .push((cgu.name().clone(), linkage));
      }
    }

    let mut item_keys: Vec<_> = items
      .iter()
      .map(|i| {
        let mut output = ty::print::with_no_trimmed_paths(|| i.to_string());
        output.push_str(" @@");
        let mut empty = Vec::new();
        let cgus = item_to_cgus.get_mut(i).unwrap_or(&mut empty);
        cgus.as_mut_slice().sort_by_key(|&(ref name, _)| name.clone());
        cgus.dedup();
        for &(ref cgu_name, (linkage, _)) in cgus.iter() {
          output.push_str(" ");
          output.push_str(&cgu_name.as_str());

          let linkage_abbrev = match linkage {
            Linkage::External => "External",
            Linkage::AvailableExternally => "Available",
            Linkage::LinkOnceAny => "OnceAny",
            Linkage::LinkOnceODR => "OnceODR",
            Linkage::WeakAny => "WeakAny",
            Linkage::WeakODR => "WeakODR",
            Linkage::Appending => "Appending",
            Linkage::Internal => "Internal",
            Linkage::Private => "Private",
            Linkage::ExternalWeak => "ExternalWeak",
            Linkage::Common => "Common",
          };

          output.push_str("[");
          output.push_str(linkage_abbrev);
          output.push_str("]");
        }
        output
      })
      .collect();

    item_keys.sort();

    for item in item_keys {
      println!("MONO_ITEM {}", item);
    }
  }

  (tcx.arena.alloc(mono_items), tcx.arena.alloc_from_iter(units.into_iter()))
}

pub fn collect_items_rec<'tcx>(tcx: TyCtxt<'tcx>,
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
            let inst = tcx.stubbed_instance(inst);

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
        let ty = instance.ty(tcx, ParamEnv::reveal_all());
        visit_drop_use(tcx, ty, true, &mut output);

        if let Ok(val) = tcx.const_eval_poly(def_id) {
          collect_const_value(tcx, val, &mut output);
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
    collect_items_rec(tcx, neighbour, visited,
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
  if let ty::FnDef(def_id, substs) = *ty.kind() {
    let instance = if is_direct_call {
      ty::Instance::resolve(tcx, ty::ParamEnv::reveal_all(), def_id, substs).unwrap().unwrap()
    } else {
      ty::Instance::resolve_for_fn_ptr(tcx, ty::ParamEnv::reveal_all(), def_id, substs)
        .unwrap()
    };
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
      match tail.kind() {
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

  match (source_ty.kind(), target_ty.kind()) {
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

  if let ty::Dynamic(ref trait_ty, ..) = trait_ty.kind() {
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

fn collect_const_value<'tcx, F>(tcx: TyCtxt<'tcx>,
                                value: ConstValue<'tcx>,
                                output: &mut F)
  where F: FnMut(MonoItem<'tcx>),
{
  match value {
    ConstValue::Scalar(Scalar::Ptr(ptr)) => collect_miri(tcx, ptr.alloc_id, output),
    ConstValue::Slice { data: alloc, start: _, end: _ } | ConstValue::ByRef { alloc, .. } => {
      for &((), id) in alloc.relocations().values() {
        collect_miri(tcx, id, output);
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
  match tcx.global_alloc(alloc_id) {
    GlobalAlloc::Static(did) => {
      trace!("collecting static {:?}", did);
      output(MonoItem::Static(did));
    },
    GlobalAlloc::Memory(alloc) => {
      trace!("collecting {:?} with {:#?}", alloc_id, alloc);
      for &((), inner) in alloc.relocations().values() {
        rustc_data_structures::stack::ensure_sufficient_stack(|| {
          collect_miri(tcx, inner, output);
        });
      }
    },
    GlobalAlloc::Function(fn_instance) => {
      trace!("collecting {:?} with {:#?}", alloc_id, fn_instance);
      output(create_fn_mono_item(fn_instance));
    },
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
    ty::InstanceDef::Item(def) => {
      if let Some((did, param_did)) = def.as_const_arg() {
        Some(tcx.optimized_mir_of_const_arg((did, param_did)))
      } else if tcx.is_mir_available(def.did) {
        Some(tcx.optimized_mir(def.did))
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
        instance,
        _indirect: indirect,
        output,
      };
      collector.visit_body(&mir);
    }
  }
}


struct MirNeighborCollector<'a, 'tcx, F, G>
  where F: FnMut(MonoItem<'tcx>),
        G: FnMut(MonoItem<'tcx>),
{
  tcx: TyCtxt<'tcx>,
  instance: Instance<'tcx>,
  mir: &'tcx mir::Body<'tcx>,
  /// Some day, we might be able to support indirect functions calls, even if it's emulated
  /// by something like device side enqueue.
  _indirect: &'a mut F,
  output: &'a mut G,
}

impl<'a, 'tcx, F, G> MirNeighborCollector<'a, 'tcx, F, G>
  where F: FnMut(MonoItem<'tcx>),
        G: FnMut(MonoItem<'tcx>),
{
  pub fn monomorphize<T>(&self, value: T) -> T
    where T: TypeFoldable<'tcx>,
  {
    debug!("monomorphize: self.instance={:?}", self.instance);
    if let Some(substs) = self.instance.substs_for_mir_body() {
      self.tcx.subst_and_normalize_erasing_regions(substs, ty::ParamEnv::reveal_all(), &value)
    } else {
      self.tcx.normalize_erasing_regions(ty::ParamEnv::reveal_all(), value)
    }
  }
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
        let origin_target_ty = self.monomorphize(target_ty);
        let source_ty = operand.ty(self.mir, self.tcx);
        let origin_source_ty = self.monomorphize(source_ty);
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
        let fn_ty = self.monomorphize(fn_ty);
        trace!("reify-fn-pointer cast: {:?}", fn_ty);
        visit_fn_use(self.tcx, fn_ty, false, self.output);
      },
      mir::Rvalue::Cast(
        mir::CastKind::Pointer(PointerCast::ClosureFnPointer(_)), ref operand, _
      ) => {
        let source_ty = operand.ty(self.mir, self.tcx);
        let source_ty = self.monomorphize(source_ty);
        match *source_ty.kind() {
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
          .require(LangItem::ExchangeMalloc)
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

    let substituted_constant = self.monomorphize(*constant);
    let param_env = ty::ParamEnv::reveal_all();

    match substituted_constant.val {
      ty::ConstKind::Value(val) => collect_const_value(self.tcx, val, self.output),
      ty::ConstKind::Unevaluated(def_id, substs, promoted) => {
        match self.tcx.const_eval_resolve(param_env, def_id, substs, promoted, None) {
          Ok(val) => collect_const_value(self.tcx, val, self.output),
          Err(ErrorHandled::Reported(ErrorReported)) |
          Err(ErrorHandled::Linted) => {}
          Err(ErrorHandled::TooGeneric) => span_bug!(
                        self.mir.source_info(location).span,
                        "collection encountered polymorphic constant",
                    ),
        }
      }
      _ => {}
    }

    self.super_const(constant);
  }

  fn visit_terminator(&mut self, terminator: &mir::Terminator<'tcx>, location: Location) {
    debug!("visiting terminator {:?} @ {:?}", terminator, location);

    let tcx = self.tcx;
    match terminator.kind {
      mir::TerminatorKind::Call { ref func, .. } => {
        let callee_ty = func.ty(self.mir, tcx);
        let callee_ty = self.monomorphize(callee_ty);
        visit_fn_use(tcx, callee_ty, true, &mut self.output);
      },
      mir::TerminatorKind::Drop { ref place, .. } |
      mir::TerminatorKind::DropAndReplace { ref place, .. } => {
        let ty = place.ty(self.mir, tcx).ty;
        let ty = self.monomorphize(ty);
        visit_drop_use(tcx, ty, true, self.output);
      },
      mir::TerminatorKind::InlineAsm { ref operands, .. } => {
        // Handling this is dubious.
        for op in operands {
          match *op {
            mir::InlineAsmOperand::SymFn { ref value } => {
              let fn_ty = self.monomorphize(value.literal.ty);
              visit_fn_use(self.tcx, fn_ty, false, &mut self.output);
            }
            mir::InlineAsmOperand::SymStatic { def_id } => {
              trace!("collecting asm sym static {:?}", def_id);
              (self.output)(MonoItem::Static(def_id));
            }
            _ => {}
          }
        }
      }
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
            let li = LangItem::PanicBoundsCheck;
            let def_id = tcx.require_lang_item(li, None);
            let inst = Instance::mono(tcx, def_id);
            (self.output)(create_fn_mono_item(inst));
          },
          _ => {
            let li = LangItem::Panic;
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
      mir::TerminatorKind::FalseEdge { .. } |
      mir::TerminatorKind::FalseUnwind { .. } => bug!(),
    }

    self.super_terminator(terminator, location);
  }

  fn visit_local(
    &mut self,
    _place_local: &Local,
    _context: mir::visit::PlaceContext,
    _location: Location,
  ) {
  }
}
