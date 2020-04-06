
//! Figure out what needs codegening. Some of this is redundant, particularly
//! the evaluation of statics, in fact we should copy *exactly* the output of
//! the host evaluation.
//!
//! This contains code from the relevant parts of `rustc`.
//!
//! We proceed by gathering all possible mono items, then go back over the
//! items, possibly applying transforms specific to device capabilities.

use rustc_middle::mir::mono::{CodegenUnit, MonoItem, Linkage, Visibility, };
use rustc_middle::ty::{TyCtxt, };
use rustc_hir::def_id::{CrateNum, LOCAL_CRATE, DefIdSet, };
use rustc_mir::monomorphize::{collector::InliningMap,
                              partitioning::partition, };
use rustc_data_structures::fx::{FxHashMap, FxHashSet};

use std::sync::{Arc, };

use crate::codegen::PlatformCodegen;
use super::driver_data::DriverData;

use gintrinsics::collector::{collect_items_rec, create_fn_mono_item, };
use gintrinsics::stubbing::Stubber;

pub fn collect_and_partition_mono_items<'tcx, P>(tcx: TyCtxt<'tcx>,
                                                 dd: &'tcx DriverData<'tcx, P>,
                                                 cnum: CrateNum)
  -> (Arc<DefIdSet>, Arc<Vec<Arc<CodegenUnit<'tcx>>>>)
  where P: PlatformCodegen,
{
  assert_eq!(cnum, LOCAL_CRATE);

  // normally, we start by collecting crate roots (non-generic items
  // to seed monomorphization).
  // We, however, already know the root: it's the root function passed to
  // the kernel_id intrinsic. Plus, we *only* want to codegen what is
  // actually used in the function.

  let root = dd.root().instance.clone();
  let mono_root = create_fn_mono_item(root);

  let mut visited: FxHashSet<_> = Default::default();
  let mut inlining_map = Some(InliningMap::new());

  let stubber = Stubber::default();

  {
    collect_items_rec(tcx, &stubber, dd,
                      mono_root,
                      &mut visited,
                      &mut inlining_map);
  }
  let inlining_map = inlining_map.unwrap();

  let items = visited;
  let mut units = partition(tcx, items.iter().cloned(),
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
        let mut output = i.to_string(tcx, false);
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
