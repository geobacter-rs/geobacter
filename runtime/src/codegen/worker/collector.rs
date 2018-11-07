
//! Figure out what needs codegening. Some of this is redundant, particularly
//! the evaluation of statics, in fact we should copy *exactly* the output of
//! the host evaluation.
//!
//! This contains code from the relevant parts of `rustc`.
//!
//! We proceed by gathering all possible mono items, then go back over the
//! items, possibly applying transforms specific to device capabilities.

use rustc::hir::def_id::{CrateNum, LOCAL_CRATE, };
use rustc::middle::lang_items::{self, ExchangeMallocFnLangItem, };
use rustc::mir::{self, Location, Promoted, visit::Visitor, };
use rustc::mir::interpret::{AllocId, ConstValue, GlobalId, AllocType,
                            Scalar, EvalErrorKind, ErrorHandled, };
use rustc::mir::mono::{CodegenUnit, MonoItem, Linkage, Visibility, };
use rustc::ty::adjustment::CustomCoerceUnsized;
use rustc::ty::query::Providers;
use rustc::ty::{self, TyCtxt, Instance, ParamEnv, subst::Substs,
                TypeFoldable, };
use rustc::util::nodemap::{DefIdSet, FxHashSet, };
use rustc_mir::monomorphize::{self, collector::InliningMap,
                              partitioning::partition,
                              partitioning::PartitioningStrategy,
                              MonoItemExt, };
use rustc_data_structures::indexed_vec::Idx;
use rustc_data_structures::fx::{FxHashMap};

use std::sync::{Arc, };

use super::driver_data::DriverData;

pub fn provide(providers: &mut Providers) {
  providers.collect_and_partition_mono_items =
    collect_and_partition_mono_items;
}

fn collect_and_partition_mono_items<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                              cnum: CrateNum)
  -> (Arc<DefIdSet>, Arc<Vec<Arc<CodegenUnit<'tcx>>>>)
{
  DriverData::with(tcx, move |tcx, dd| {
    collect_and_partition_mono_items_(tcx, dd, cnum)
  })
}
fn collect_and_partition_mono_items_<'tcx>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                                           dd: &DriverData<'tcx>,
                                           cnum: CrateNum)
  -> (Arc<DefIdSet>, Arc<Vec<Arc<CodegenUnit<'tcx>>>>)
{
  assert_eq!(cnum, LOCAL_CRATE);

  // normally, we start by collecting crate roots (non-generic items
  // to seed monomorphization).
  // We, however, already know the root: it's the root function passed to
  // the kernel_id intrinsic. Plus, we *only* want to codegen what is
  // actually used in the function.

  let root = Instance::mono(tcx, dd.root);
  let mono_root = create_fn_mono_item(root);

  let mut visited: FxHashSet<_> = Default::default();
  let mut inlining_map = InliningMap::new();

  {
    collect_items_rec(tcx, dd, mono_root, &mut visited,
                      &mut inlining_map);
  }

  let strategy = PartitioningStrategy::FixedUnitCount(1);
  let items = visited;
  let mut units = partition(tcx,
                            items.iter().cloned(),
                            strategy,
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

  let units: Vec<Arc<CodegenUnit>> = units
    .into_iter()
    .map(Arc::new)
    .collect();

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
        let mut output = i.to_string(tcx);
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

  (Arc::new(mono_items), Arc::new(units))
}

fn collect_items_rec<'tcx>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                           dd: &DriverData<'tcx>,
                           start: MonoItem<'tcx>,
                           visited: &mut FxHashSet<MonoItem<'tcx>>,
                           inlining_map: &mut InliningMap<'tcx>)
{
  if !visited.insert(start.clone()) {
    return;
  }
  //debug!("BEGIN collect_items_rec({})", start.to_string(tcx.tcx));

  let mut indirect_neighbors = Vec::new();
  let mut neighbors = Vec::new();

  match start {
    MonoItem::Static(def_id) => {
      let instance = Instance::mono(tcx, def_id);
      let ty = instance.ty(tcx);
      visit_drop_use(tcx, ty, true, &mut neighbors);

      let cid = GlobalId {
        instance,
        promoted: None,
      };
      let param_env = ParamEnv::reveal_all();
      if let Ok(val) = tcx.const_eval(param_env.and(cid)) {
        collect_const(tcx, dd, val, instance.substs,
                      &mut neighbors);
      }
    },
    MonoItem::Fn(instance) => {
      // don't check recursion limits or type len limits: if they were
      // an issue, `rustc` targeting the host would have erred.
      collect_neighbours(tcx, dd, instance,
                         &mut indirect_neighbors,
                         &mut neighbors);
    },
    MonoItem::GlobalAsm(..) => {
      unimplemented!();
    },
  }

  record_accesses(tcx,
                  dd, start,
                  &neighbors[..],
                  inlining_map);

  for neighbour in neighbors {
    collect_items_rec(tcx, dd, neighbour, visited,
                      inlining_map);
  }

  //debug!("END collect_items_rec({})", start.to_string(tcx.tcx));
}

fn record_accesses<'a, 'tcx>(_tcx: TyCtxt<'a, 'tcx, 'tcx>,
                             _dd: &DriverData<'tcx>,
                             caller: MonoItem<'tcx>,
                             callees: &[MonoItem<'tcx>],
                             inlining_map: &mut InliningMap<'tcx>) {
  let accesses = callees.into_iter()
    .map(|mono_item| {
      (*mono_item, true)
    });

  inlining_map.record_accesses(caller, accesses);
}

fn visit_drop_use<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                            ty: ty::Ty<'tcx>,
                            is_direct_call: bool,
                            output: &mut Vec<MonoItem<'tcx>>)
{
  let instance = monomorphize::resolve_drop_in_place(tcx, ty);
  visit_instance_use(tcx, instance, is_direct_call, output);
}
fn visit_fn_use<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                          ty: ty::Ty<'tcx>,
                          is_direct_call: bool,
                          output: &mut Vec<MonoItem<'tcx>>)
{
  if let ty::FnDef(def_id, substs) = ty.sty {
    let instance = Instance::resolve(tcx, ParamEnv::reveal_all(),
                                     def_id, substs)
      .unwrap();
    visit_instance_use(tcx, instance, is_direct_call, output);
  } else {
    error!("visit_fn_use: fn type {:?} not implemented!", ty);
  }
}

fn visit_instance_use<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                instance: ty::Instance<'tcx>,
                                is_direct_call: bool,
                                output: &mut Vec<MonoItem<'tcx>>)
{
  debug!("visit_item_use({:?}, is_direct_call={:?})", instance, is_direct_call);

  //assert!(is_direct_call, "TODO: generate wrappers for sending calls to the queue");

  match instance.def {
    ty::InstanceDef::Intrinsic(def_id) => {
      if !is_direct_call {
        bug!("intrinsic {:?} being reified", def_id);
      }
      if let Some(_mir) = tcx.custom_intrinsic_mir(instance) {
        output.push(create_fn_mono_item(instance));
      }
    }
    ty::InstanceDef::VtableShim(..) |
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
                                        dd: &DriverData<'tcx>,
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
    }
    (&ty::Adt(def_a, _), &ty::Adt(def_b, _)) if def_a.is_box() && def_b.is_box() => {
      ptr_vtable(source_ty.boxed_ty(), target_ty.boxed_ty())
    }

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

      find_vtable_types_for_unsizing(tcx, dd,
                                     source_fields[coerce_index].ty(tcx,
                                                                    source_substs),
                                     target_fields[coerce_index].ty(tcx,
                                                                    target_substs))
    }
    _ => bug!("find_vtable_types_for_unsizing: invalid coercion {:?} -> {:?}",
                  source_ty,
                  target_ty)
  }
}

fn create_fn_mono_item<'a, 'tcx>(instance: Instance<'tcx>) -> MonoItem<'tcx> {
  debug!("create_fn_mono_item(instance={})", instance);
  MonoItem::Fn(instance)
}

/// Creates a `MonoItem` for each method that is referenced by the vtable for
/// the given trait/impl pair. We don't end up codegen-ing these, but we need
/// to discover them so the host accelerator can setup it's index table with
/// these functions as entries.
fn create_mono_items_for_vtable_methods<'tcx>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                                              _dd: &DriverData<'tcx>,
                                              trait_ty: ty::Ty<'tcx>,
                                              impl_ty: ty::Ty<'tcx>,
                                              output: &mut Vec<MonoItem<'tcx>>) {
  assert!(!trait_ty.needs_subst() && !trait_ty.has_escaping_regions() &&
    !impl_ty.needs_subst() && !impl_ty.has_escaping_regions());

  if let ty::Dynamic(ref trait_ty, ..) = trait_ty.sty {
    let poly_trait_ref = trait_ty.principal().with_self_ty(tcx, impl_ty);
    assert!(!poly_trait_ref.has_escaping_regions());

    // Walk all methods of the trait, including those of its supertraits
    let methods = tcx.vtable_methods(poly_trait_ref);
    let methods = methods.iter().cloned()
      .filter_map(|method| method)
      .map(|(def_id, substs)| ty::Instance::resolve(
        tcx,
        ty::ParamEnv::reveal_all(),
        def_id,
        substs).unwrap())
      .map(|instance| create_fn_mono_item(instance));
    output.extend(methods);
    // Also add the destructor
    visit_drop_use(tcx, impl_ty, false, output);
  }
}

fn collect_const<'tcx>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                       dd: &DriverData<'tcx>,
                       constant: &ty::Const<'tcx>,
                       param_substs: &'tcx Substs<'tcx>,
                       output: &mut Vec<MonoItem<'tcx>>)
{
  debug!("visiting const {:?}", *constant);

  let val = match constant.val {
    ConstValue::Unevaluated(def_id, substs) => {
      let param_env = ParamEnv::reveal_all();
      let substs = tcx.subst_and_normalize_erasing_regions(param_substs,
                                                           param_env,
                                                           &substs);
      let instance = Instance::resolve(tcx, param_env,
                                       def_id, substs)
        .unwrap();

      let cid = GlobalId {
        instance,
        promoted: None,
      };
      match tcx.const_eval(param_env.and(cid)) {
        Ok(val) => val.val,
        Err(ErrorHandled::Reported) => return,
        Err(ErrorHandled::TooGeneric) => span_bug!(
                    tcx.def_span(def_id), "collection encountered polymorphic constant",
                ),
      }
    },
    _ => constant.val,
  };
  match val {
    ConstValue::Unevaluated(..) => bug!("const eval yielded unevaluated const"),
    ConstValue::ScalarPair(Scalar::Ptr(a), Scalar::Ptr(b)) => {
      collect_miri(tcx, dd, a.alloc_id, output);
      collect_miri(tcx, dd, b.alloc_id, output);
    }
    ConstValue::ScalarPair(_, Scalar::Ptr(ptr)) |
    ConstValue::ScalarPair(Scalar::Ptr(ptr), _) |
    ConstValue::Scalar(Scalar::Ptr(ptr)) =>
      collect_miri(tcx, dd, ptr.alloc_id, output),
    ConstValue::ByRef(_id, alloc, _offset) => {
      for &((), id) in alloc.relocations.values() {
        collect_miri(tcx, dd, id, output);
      }
    }
    _ => {},
  }
}
/// Scan the miri alloc in order to find function calls, closures, and drop-glue
fn collect_miri<'tcx>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                      dd: &DriverData<'tcx>,
                      alloc_id: AllocId,
                      output: &mut Vec<MonoItem<'tcx>>)
{
  let alloc_type = tcx.alloc_map.lock().get(alloc_id);
  match alloc_type {
    Some(AllocType::Static(did)) => {
      let instance = Instance::mono(tcx, did);
      trace!("collecting static {:?}", did);
      error!("TODO static is ignored: {:?}", instance);
      output.push(MonoItem::Static(did));
    }
    Some(AllocType::Memory(alloc)) => {
      trace!("collecting {:?} with {:#?}", alloc_id, alloc);
      for &((), inner) in alloc.relocations.values() {
        collect_miri(tcx, dd, inner, output);
      }
    },
    Some(AllocType::Function(fn_instance)) => {
      trace!("collecting {:?} with {:#?}", alloc_id, fn_instance);
      error!("TODO function is ignored: {:?}", fn_instance);
      //output.push(create_fn_mono_item(fn_instance));
    }
    None => bug!("alloc id without corresponding allocation: {}", alloc_id),
  }
}

/// Scan the MIR in order to find function calls, closures, and drop-glue
fn collect_neighbours<'tcx>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                            dd: &DriverData<'tcx>,
                            instance: Instance<'tcx>,
                            indirect: &mut Vec<MonoItem<'tcx>>,
                            output: &mut Vec<MonoItem<'tcx>>)
{
  let mir = tcx.instance_mir(instance);

  {
    let mut collector = MirNeighborCollector {
      tcx,
      dd,
      mir: &mir,
      indirect,
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
      Ok(val) => collect_const(tcx, dd, val, instance.substs, output),
      Err(_) => {
        // would have erred on the host, ignore.
        unreachable!();
      },
    }
  }
}


struct MirNeighborCollector<'b, 'tcx>
  where 'tcx: 'b,
{
  tcx: TyCtxt<'b, 'tcx, 'tcx>,
  dd: &'b DriverData<'tcx>,
  mir: &'b mir::Mir<'tcx>,
  /// Items which we don't define. The runtime needs these so it can
  /// issue calls to the HSA framework.
  indirect: &'b mut Vec<MonoItem<'tcx>>,
  output: &'b mut Vec<MonoItem<'tcx>>,
  substs: &'tcx Substs<'tcx>,
}

impl<'b, 'tcx> mir::visit::Visitor<'tcx> for MirNeighborCollector<'b, 'tcx>
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
                                                                    self.dd,
                                                                    source_ty,
                                                                    target_ty);
        // This could also be a different Unsize instruction, like
        // from a fixed sized array to a slice. But we are only
        // interested in things that produce a vtable.
        if target_ty.is_trait() && !source_ty.is_trait() {
          error!("TODO: unsizing: target_ty {:?}, source_ty {:?}",
                 target_ty, source_ty);
          create_mono_items_for_vtable_methods(self.tcx,
                                               self.dd,
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
        error!("TODO: reify-fn-pointer cast: {:?}", fn_ty);
        //visit_fn_use(self.tcx.tcx, fn_ty, false, &mut self.output);
      }
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
            self.output.push(create_fn_mono_item(instance));
          }
          _ => bug!(),
        }
      }
      mir::Rvalue::NullaryOp(mir::NullOp::Box, _) => {
        let tcx = self.tcx;
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

    collect_const(self.tcx, self.dd, constant, self.substs, self.output);

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
      }
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
            self.output.push(create_fn_mono_item(inst));
          },
          _ => {
            let li = lang_items::PanicFnLangItem;
            let def_id = tcx.require_lang_item(li);
            let inst = Instance::mono(tcx, def_id);
            self.output.push(create_fn_mono_item(inst));
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

    let tcx = self.tcx;
    let instance = Instance::mono(tcx, static_.def_id);
    error!("TODO: inject the contents of the statics using their value on the host: {:?}",
           instance);
    self.output.push(MonoItem::Static(static_.def_id));

    self.super_static(static_, context, location);
  }
}
