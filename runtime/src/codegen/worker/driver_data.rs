use std::any::Any;
use std::sync::{Weak, };

use rustc::hir::def_id::{DefId, };
use rustc::mir::CustomIntrinsicMirGen;
use rustc::ty::{self, TyCtxt, };
use rustc::ty::item_path::{with_forced_absolute_paths};
use rustc_data_structures::fx::{FxHashMap, };
use rustc_data_structures::sync::{Lrc, RwLock, };
use rustc_metadata::cstore::CStore;
use syntax_pos::symbol::{InternedString, };

use hsa_core::kernel::KernelId;

use legionella_intrinsics::{DefIdFromKernelId, };

use {Accelerator, };
use context::Context;
use passes::{Pass, PassType, };

pub struct DriverData<'tcx> {
  pub context: Context,
  pub accels: &'tcx [Weak<Accelerator>],

  pub root: DefId,

  pub passes: &'tcx [Box<dyn Pass>],

  pub replaced_def_ids: RwLock<FxHashMap<DefId, DefId>>,
  /// maps `LOCAL_CRATE` (ie generated MIR wrappers) to their type.
  /// The local crate provider for `Providers::type_of` uses the HIR
  /// which we don't have.
  /// TODO: `DefId::index` should be dense, try to exploit that fact.
  pub(super) type_of: RwLock<FxHashMap<DefId, ty::Ty<'tcx>>>,
  pub intrinsics: &'tcx FxHashMap<InternedString, Lrc<dyn CustomIntrinsicMirGen>>,
}

impl<'tcx> DriverData<'tcx> {
  pub fn with<F, R>(tcx: TyCtxt<'_, 'tcx, 'tcx>, f: F) -> R
    where F: for<'b> FnOnce(TyCtxt<'_, 'tcx, 'tcx>,
                            &'b DriverData<'tcx>) -> R,
  {
    tcx
      .with_driver_data(move |tcx, dd| {
        dd.downcast_ref::<DriverData<'static>>()
          .map(|dd| {
            // force the correct lifetime:
            let dd: &DriverData<'tcx> = unsafe {
              ::std::mem::transmute(dd)
            };
            dd
          })
          .map(move |dd| {
            f(tcx, dd)
          })
      })
      .and_then(|a| a )
      .expect("unexpected type in tcx driver data")
  }

  pub fn passes(&self) -> &[Box<dyn Pass>] { self.passes.as_ref() }
  pub fn as_def_id(&self, id: KernelId) -> Result<DefId, String> {
    self.convert_kernel_id(id)
      .ok_or_else(|| {
        format!("crate metadata missing for `{}`", id.crate_name)
      })
  }
  pub fn replace_def_id(&self, tcx: TyCtxt<'_, 'tcx, 'tcx>,
                        id: DefId) -> DefId {
    let replaced = {
      let r = self.replaced_def_ids.read();
      r.get(&id).cloned()
    };
    if let Some(replaced) = replaced {
      return replaced;
    }

    info!("running passes on {}", {
      with_forced_absolute_paths(|| tcx.item_path_str(id) )
    });

    let mut new_def_id = id;
    for pass in self.passes.iter() {
      let ty = pass.pass_type();
      let replaced = match ty {
        PassType::Replacer(f) => {
          f(tcx, self, new_def_id)
        },
      };
      match replaced {
        Some(id) => {
          new_def_id = id;
          break;
        },
        None => { continue; }
      }
    }

    let new_def_id = new_def_id;

    self.replaced_def_ids
      .write()
      .insert(id, new_def_id);
    new_def_id
  }
  pub fn is_root(&self, def_id: DefId) -> bool {
    self.root == def_id
  }

  pub fn expect_type_of(&self, def_id: DefId) -> ty::Ty<'tcx> {
    let r = self.type_of.read();
    r.get(&def_id)
      .cloned()
      .unwrap_or_else(|| {
        bug!("generated def id {:?} was not given a type", def_id);
      })
  }
}

impl<'tcx> DefIdFromKernelId for DriverData<'tcx> {
  fn get_cstore(&self) -> &CStore {
    self.context.cstore()
  }
}
