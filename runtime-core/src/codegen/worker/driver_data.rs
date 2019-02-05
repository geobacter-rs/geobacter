use std::mem::transmute;
use std::sync::{Weak, Arc, mpsc::Sender, mpsc::channel, };

use rustc::hir::def_id::{DefId, };
use rustc::mir::CustomIntrinsicMirGen;
use rustc::ty::{self, TyCtxt, };
use rustc::ty::item_path::{with_forced_absolute_paths};
use rustc_data_structures::fx::{FxHashMap, };
use rustc_data_structures::sync::{Lrc, RwLock, ReadGuard, MappedReadGuard, };
use rustc_metadata::cstore::CStore;
use rustc_target::abi::{LayoutDetails, };
use syntax_pos::symbol::{InternedString, };

use crossbeam::sync::WaitGroup;

use hsa_core::kernel::KernelId;
use hsa_core::kernel::kernel_id_for;

use lintrinsics::{DefIdFromKernelId, };
use lintrinsics::attrs::{Root, Condition, };

use {Accelerator, AcceleratorTargetDesc, };
use context::Context;
use passes::{Pass, PassType, };

use super::HostQueryMessage;
use utils::UnsafeSyncSender;

// TODO move this in with shared driver stuffs.

pub struct DriverData<'tcx> {
  pub context: Context,
  pub accels: &'tcx [Weak<Accelerator>],

  /// DO NOT USE DIRECTLY. Use `dd.host_codegen()`
  host_codegen: Option<UnsafeSyncSender<HostQueryMessage>>,

  pub target_desc: &'tcx Arc<AcceleratorTargetDesc>,

  /// Needs to be initialized after the TyCtxt is created.
  root: RwLock<Option<Root>>,
  pub root_conditions: Vec<Condition>,

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
  pub(crate) fn new(context: Context,
                    accels: &'tcx [Weak<Accelerator>],
                    host_codegen: Option<Sender<HostQueryMessage>>,
                    target_desc: &'tcx Arc<AcceleratorTargetDesc>,
                    root_conditions: Vec<Condition>,
                    passes: &'tcx [Box<dyn Pass>],
                    intrinsics: &'tcx FxHashMap<InternedString, Lrc<dyn CustomIntrinsicMirGen>>)
    -> Self
  {
    DriverData {
      context,
      accels,
      target_desc,

      host_codegen: host_codegen.map(|h| UnsafeSyncSender(h) ),

      // XXX? never initialized for host codegen query mode.
      root: RwLock::new(None),
      root_conditions,

      passes,

      replaced_def_ids: RwLock::new(Default::default()),
      type_of: RwLock::new(Default::default()),
      intrinsics,
    }
  }
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

  fn host_codegen(&self) -> Sender<HostQueryMessage> {
    let sync = self.host_codegen
      .as_ref()
      .expect("no host codegen (is this a host codegen worker?)")
      .clone();

    let UnsafeSyncSender(unsync) = sync;
    unsync
  }
  pub fn host_layout_of(&self, ty: ty::Ty<'tcx>) -> LayoutDetails {
    let (tx, rx) = channel();
    let wait = WaitGroup::new();
    let msg = HostQueryMessage::TyLayout {
      ty: unsafe { transmute(ty) },
      wait: wait.clone(),
      ret: tx,
    };

    let host = self.host_codegen();
    host.send(msg)
      .expect("host codegen crashed?");

    wait.wait();

    // We *MUST* wait here. Otherwise we risk a segfault.
    rx.recv()
      .expect("host codegen crashed?")
      .expect("host type layout failed")
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
  pub fn def_id_for<F, Args, Ret>(&self, f: &F) -> DefId
    where F: Fn<Args, Output = Ret>,
  {
    let kid = kernel_id_for(f);
    self.convert_kernel_id(kid).unwrap()
  }

  pub fn root(&self) -> MappedReadGuard<Root> {
    ReadGuard::map(self.root.read(),
                   |opt| opt.as_ref().expect("root desc uninitialized") )
  }
  pub fn init_root(&self, root: Root) {
    assert!(self.root.read().is_none(), "root desc already initialized");
    *self.root.write() = Some(root);
  }
  pub fn is_root(&self, def_id: DefId) -> bool {
    self.root().did == def_id
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
