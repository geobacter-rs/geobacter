use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::mem::transmute;
use std::sync::{Weak, Arc, };
use std::path::Path;

use rustc_middle::mir::CustomIntrinsicMirGen;
use rustc_middle::ty::{self, TyCtxt, };
use rustc_session::config::OutputFilenames;
use rustc_data_structures::fx::{FxHashMap, };
use rustc_data_structures::sync::{Lrc, RwLock, ReadGuard, MappedReadGuard, };
use rustc_hir::def_id::{DefId, };
use rustc_span::symbol::{Symbol, };

use geobacter_core::kernel::{KernelInstanceRef, OptionalFn};

use gintrinsics::{DriverData as GIDriverData, stubbing::*, };
use gintrinsics::*;

use crate::{AcceleratorTargetDesc, };
use crate::codegen::*;
use crate::context::{Context};

use crate::codegen::products::{PCodegenResults, CodegenResults, EntryDesc};
use crate::codegen::worker::error::{PError, Error};

// TODO move this in with shared driver stuffs.

pub struct DriverData<'tcx, P>
  where P: PlatformCodegen,
{
  pub context: Context,
  pub accels: &'tcx [Weak<P::Device>],

  pub target_desc: &'tcx Arc<AcceleratorTargetDesc>,

  /// Needs to be initialized after the TyCtxt is created.
  roots: RwLock<Vec<PCodegenDesc<'tcx, P>>>,
  /// Needs to be initialized after the TyCtxt is created.
  root_conditions: RwLock<Vec<P::Condition>>,

  stubber: RwLock<Stubber>,

  pub replaced_def_ids: RwLock<FxHashMap<DefId, DefId>>,
  /// maps `LOCAL_CRATE` (ie generated MIR wrappers) to their type.
  /// The local crate provider for `Providers::type_of` uses the HIR
  /// which we don't have.
  /// TODO: `DefId::index` should be dense, try to exploit that fact.
  pub(super) type_of: RwLock<FxHashMap<DefId, ty::Ty<'tcx>>>,
  /// TODO: this is *only* modified before codegen starts. That is, before
  /// we start Rust's codegen module. This doesn't need to be locked before
  /// Rust codegen as there will only be one thread accessing it at that time.
  /// And afterwards its immutable.
  pub intrinsics: RwLock<FxHashMap<Symbol, Lrc<dyn CustomIntrinsicMirGen>>>,
}

/// This shouldn't exist.
pub struct PlatformDriverData<'tcx, P>
  where P: PlatformCodegen,
{
  /// Data which doesn't directly depend on `P`.
  pub(super) driver_data: DriverData<'tcx, P>,
  pub(super) platform: &'tcx P,
}
impl<'tcx, P> PlatformDriverData<'tcx, P>
  where P: PlatformCodegen,
{
  pub(crate) fn new(context: Context,
                    accels: &'tcx [Weak<P::Device>],
                    target_desc: &'tcx Arc<AcceleratorTargetDesc>,
                    intrinsics: FxHashMap<Symbol, Lrc<dyn CustomIntrinsicMirGen>>,
                    platform: &'tcx P)
    -> Self
  {
    let dd = DriverData {
      context,
      accels,
      target_desc,

      // XXX? never initialized for host codegen query mode.
      roots: RwLock::new(vec![]),
      root_conditions: RwLock::new(vec![]),

      // TODO allow the platform to customize
      stubber: RwLock::new(Default::default()),

      replaced_def_ids: RwLock::new(Default::default()),
      type_of: RwLock::new(Default::default()),
      intrinsics: RwLock::new(intrinsics),
    };

    PlatformDriverData {
      driver_data: dd,
      platform,
    }
  }

  pub fn dd(&self) -> &DriverData<'tcx, P> { &self.driver_data }

  pub(super) fn init_root(&self,
                          desc: PKernelDesc<P>,
                          tcx: TyCtxt<'tcx>)
    -> Result<(), PError<P>>
  {
    let instance = tcx.convert_kernel_instance(desc.instance)
      .ok_or_else(|| Error::ConvertKernelInstance(desc.instance))?;
    let root = self.platform
      .root(desc, instance, tcx, self.dd())
      .map_err(Error::InitRoot)?;
    self.driver_data.roots
      .write()
      .push(root);
    Ok(())
  }
  pub(super) fn init_conditions(&'tcx self, tcx: TyCtxt<'tcx>)
    -> Result<(), PError<P>>
  {
    let conds = {
      let root = self.driver_data.root();
      self.platform
        .root_conditions(&*root, tcx, &self.driver_data)
        .map_err(Error::InitConditions)?
    };
    self.driver_data.root_conditions.write()
      .extend(conds.into_iter());

    Ok(())
  }
  pub(super) fn pre_codegen(&self, tcx: TyCtxt<'tcx>)
    -> Result<(), PError<P>>
  {
    let dd = unsafe { transmute(&self.driver_data) };
    self.platform.pre_codegen(tcx, dd)
      .map_err(Error::PreCodegen)
  }
  pub(super) fn post_codegen(&self, tcx: TyCtxt<'tcx>,
                             tmpdir: &Path,
                             out: &OutputFilenames)
    -> Result<PCodegenResults<P>, PError<P>>
  {
    use rustc_session::config::*;

    let mut outputs = BTreeMap::new();
    for &output_type in out.outputs.keys() {
      match output_type {
        OutputType::Assembly |
        OutputType::LlvmAssembly => {
          // skip these, they are only used for debugging.
          continue;
        },
        _ => { },
      }
      let filename = Path::new("codegen.elf")
        .with_extension(output_type.extension());
      let output = tmpdir.join(filename);
      debug!("reading output {}", output.display());
      let mut file = File::open(output)?;
      let mut data = Vec::new();
      file.read_to_end(&mut data)?;

      outputs.insert(output_type, data);
    }

    let mut results: PCodegenResults<P> = CodegenResults::new();
    results.outputs = outputs;

    {
      let mut roots = self.dd().roots.write();
      for root in roots.drain(..) {
        let kernel_symbol = tcx.symbol_name(root.instance);
        debug!("kernel symbol for def_id {:?}: {}",
              root.instance.def_id(),
              kernel_symbol);
        let symbol = format!("{}", kernel_symbol);
        let entry = EntryDesc {
          kernel_instance: root.kernel_instance,
          symbol,
          platform: root.platform_desc,
        };
        results.entries.push(entry);
      }
    }

    Ok(results)
  }

  pub fn with<F, R>(tcx: TyCtxt<'tcx>, f: F) -> R
    where F: FnOnce(TyCtxt<'tcx>, &'tcx PlatformDriverData<'tcx, P>) -> R,
  {
    tcx
      .with_driver_data(move |tcx, pd| {
        pd.downcast_ref::<PlatformDriverData<'static, P>>()
          .map(|pd| {
            // force the correct lifetime:
            let pd: &'tcx PlatformDriverData<'tcx, P> = unsafe {
              ::std::mem::transmute(pd)
            };
            pd
          })
          .map(move |pd| {
            f(tcx, pd)
          })
      })
      .and_then(|a| a )
      .expect("unexpected type in tcx driver data")
  }
}

impl<'tcx, P> DriverData<'tcx, P>
  where P: PlatformCodegen,
{
  pub fn instance_of<F, Args, Ret>(&self, tcx: TyCtxt<'tcx>,
                                   f: &F) -> Instance<'tcx>
    where F: Fn<Args, Output = Ret> + OptionalFn<Args>,
  {
    let ki = f.kernel_instance().unwrap();
    tcx.convert_kernel_instance(ki)
      .expect("instance decode failure")
  }

  /// Gets first root only.
  pub fn root(&self) -> MappedReadGuard<PCodegenDesc<P>> {
    ReadGuard::map(self.roots.read(),
                   |opt| opt.get(0).expect("root desc uninitialized") )
  }
  pub fn roots(&self) -> MappedReadGuard<[PCodegenDesc<P>]> {
    ReadGuard::map(self.roots.read(), |v| &v[..] )
  }
  pub fn root_conditions(&self) -> MappedReadGuard<[P::Condition]> {
    ReadGuard::map(self.root_conditions.read(), |v| &v[..] )
  }
  /// Adds a root to the roots array. Does not check for duplicates
  pub fn add_root(&self, root: PCodegenDesc<'tcx, P>) {
    self.roots.write().push(root);
  }
  pub fn is_root(&self, def_id: DefId) -> bool {
    self.roots.read()
      .iter()
      .any(|root| root.def_id() == def_id )
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

impl<'tcx, P> GIDriverData for DriverData<'tcx, P>
  where P: PlatformCodegen,
{
  fn spec_param_data_raw(&self, instance: KernelInstanceRef) -> Option<MappedReadGuard<[u8]>> {
    MappedReadGuard::try_map(self.root(), |root| {
      root.spec_params
        .get(instance)
    }).ok()
  }
  fn stubber(&self) -> Option<MappedReadGuard<dyn DynStubber>> {
    Some(ReadGuard::map(self.stubber.read(), |s| s as &dyn DynStubber ))
  }
}
