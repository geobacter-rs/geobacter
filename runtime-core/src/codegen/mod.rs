
use std::any::Any;
use std::cmp::{Eq, PartialEq, };
use std::collections::BTreeMap;
use std::fmt::{self, Debug, };
use std::hash;
use std::mem::size_of;
use std::ops::Deref;
use std::path::{Path, };
use std::slice::from_raw_parts;
use std::sync::Arc;

use rustc_hir::def_id::DefId;
use rustc_middle::middle::codegen_fn_attrs::CodegenFnAttrs;
use rustc_middle::ty::*;
use crate::rustc_data_structures::sync::{Lrc, };
use crate::syntax::ast;

use crate::gintrinsics::{GeobacterCustomIntrinsicMirGen, attrs::ConditionItem,
                         attrs::geobacter_cfg_attrs, };

use geobacter_core::kernel::{KernelInstance, OptionalFn, KernelInstanceRef,
                             KernelDesc as DynKernelDesc, };

use crate::{Accelerator, AcceleratorTargetDesc, };
use self::products::{PlatformCodegenDesc, };
use crate::codegen::products::PCodegenResults;

pub use self::worker::error;
pub use self::worker::DriverData;
pub use self::worker::{CodegenDriver, };

use crate::any_key::AnyHash;

pub mod help;
pub mod worker;
pub mod products;

#[derive(Clone, Debug)]
pub struct KernelDesc<P>
  where P: PlatformKernelDesc,
{
  pub instance: KernelInstance,
  pub spec_params: SpecParamsDesc,
  pub platform_desc: P,
}

impl<P> KernelDesc<P>
  where P: PlatformKernelDesc,
{
  pub fn new(instance: KernelInstance, platform: P) -> Self {
    KernelDesc {
      instance,
      spec_params: Default::default(),
      platform_desc: platform,
    }
  }
}

impl<P> Eq for KernelDesc<P>
  where P: PlatformKernelDesc + Eq,
{ }
impl<LP, RP> PartialEq<KernelDesc<RP>> for KernelDesc<LP>
  where LP: PlatformKernelDesc + PartialEq<RP>,
        RP: PlatformKernelDesc,
{
  fn eq(&self, rhs: &KernelDesc<RP>) -> bool {
    if self.instance != rhs.instance { return false; }

    self.platform_desc == rhs.platform_desc
  }
}
impl<P> hash::Hash for KernelDesc<P>
  where P: PlatformKernelDesc,
{
  fn hash<H>(&self, hasher: &mut H)
    where H: hash::Hasher,
  {
    ::std::hash::Hash::hash(&self.instance, hasher);
    let platform = &self.platform_desc as &dyn AnyHash;
    AnyHash::hash(platform, hasher);
  }
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct SpecParamsDesc(Option<Arc<BTreeMap<KernelInstance, Vec<u8>>>>);
impl SpecParamsDesc {
  fn get_mut(&mut self) -> &mut BTreeMap<KernelInstance, Vec<u8>> {
    let m = self.0.get_or_insert_with(Default::default);
    Arc::make_mut(m)
  }
  pub fn empty(&self) -> bool {
    self.0.is_none() || self.0.as_ref().unwrap().len() == 0
  }
  pub fn clear(&mut self) {
    if let Some(arc) = self.0.as_mut() {
      if let Some(map) = Arc::get_mut(arc) {
        // if we are the only owner, just clear in place.
        map.clear();
        return;
      }
    } else {
      return;
    }

    // else just set to None
    self.0 = None;
  }
  pub fn undefine<F, R>(&mut self, f: F)
    where F: Fn() -> R,
  {
    if self.0.is_none() { return; }
    let key = f.kernel_instance().unwrap();
    self.get_mut().remove(&key);
  }
  pub fn define<F, R>(&mut self, f: F, value: &R)
    where F: Fn() -> R,
          R: Copy + Unpin + 'static,
  {
    unsafe { self.define_raw(f, value) }
  }
  pub unsafe fn define_raw<F, R>(&mut self, f: F, value: &R)
    where F: Fn() -> R,
  {
    let key = f.kernel_instance().unwrap();

    let mut bytes = Vec::with_capacity(size_of::<R>());
    bytes.set_len(size_of::<R>());
    let value_bytes =
      from_raw_parts(value as *const R as *const u8,
                     size_of::<R>());
    bytes.copy_from_slice(value_bytes);

    self.get_mut().insert(key, bytes);
  }
  fn get<'a, 'b>(&'a self, key: KernelInstanceRef<'b>) -> Option<&'a [u8]> {
    Some(&**self.0.as_ref()?.get(&key)?)
  }
}

#[derive(Clone, Eq, PartialEq, Debug, Hash)]
pub struct CodegenKernelInstance {
  pub name: String,
  pub instance: Vec<u8>,
}
impl From<KernelInstance> for CodegenKernelInstance {
  fn from(v: KernelInstance) -> Self {
    CodegenKernelInstance {
      name: v.name().unwrap_or_default().to_owned(),
      instance: v.instance.to_owned(),
    }
  }
}

/// This type is used in the codegen worker context to communicate
/// with platform runtime crate. It is analogous to `KernelDesc` above.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CodegenDesc<'tcx, P> {
  pub instance: Instance<'tcx>,
  pub kernel_instance: CodegenKernelInstance,
  pub spec_params: SpecParamsDesc,
  pub platform_desc: P,
}

impl<'tcx, P> CodegenDesc<'tcx, P> {
  pub fn def_id(&self) -> DefId { self.instance.def_id() }
}

impl<'tcx, P> Deref for CodegenDesc<'tcx, P> {
  type Target = P;
  fn deref(&self) -> &P { &self.platform_desc }
}


/// Platform specific data which is derived at compile time.
/// Typically, a description of used resources and their binding locations,
/// runtime device requirements.
/// Currently, this data is required to be statically allocated.
pub trait PlatformKernelDesc: AnyHash + Any + Debug + Eq + Send + Sync + 'static { }

/// Helper type to reduce typing.
pub type PKernelDesc<P> = KernelDesc<<P as PlatformCodegen>::KernelDesc>;
pub type PCodegenDesc<'tcx, P> = CodegenDesc<'tcx, <P as PlatformCodegen>::CodegenDesc>;

/// Helper trait for geobacter specific custom mirgen impls.
pub trait PlatformIntrinsicInsert {
  fn insert<T>(&mut self, intrinsic: T)
    where T: GeobacterCustomIntrinsicMirGen + fmt::Display
  {
    let name = format!("{}", intrinsic);
    self.insert_name(&name, intrinsic)
  }
  fn insert_name<T>(&mut self, name: &str, intrinsic: T)
    where T: GeobacterCustomIntrinsicMirGen;
}

/// Codegen details specific to a platform. Some platform are particular
/// about many codegen details (like Vulkan); this trait provides a place
/// for the runtime trait to customize codegen to suit its needs.
/// This is a completely unstable interface.
/// Note: it is assumed that errors are caught and reported at (host)
/// compile time.
/// XXX a lot of code assumes this is just a marker type (ie contains no
/// data)
pub trait PlatformCodegen: Sized + Clone + Debug + Send + Sync + 'static {
  type Device: crate::Device<Codegen = Self> + Accelerator;
  /// Info which is compiled by the host compiler
  type KernelDesc: PlatformKernelDesc + Clone;
  /// Info which is compiled by the runtime compiler. This is used, eg,
  /// for storing the link names of globals.
  /// Note there can be more than one `CodegenDesc` per `KernelDesc`.
  /// Kernels can contain more than one root (though, note that
  /// we only codegen starting at a single root) due to device side
  /// enqueue.
  type CodegenDesc: PlatformCodegenDesc + Clone;

  type Condition: ConditionItem + Send + Sync;

  /// Is the device object provided associated with this platform?
  fn is_device(accel: &dyn Accelerator) -> bool {
    <Self::Device as Accelerator>::downcast_ref(accel).is_some()
  }

  /// Change Rust `Session` options to suit.
  fn modify_rustc_session_options(&self, _target_desc: &Arc<AcceleratorTargetDesc>,
                                  _opts: &mut rustc_session::config::Options)
  { }

  /// Add intrinsics which don't depend on the kernel. This is done once at
  /// startup.
  fn insert_intrinsics<T>(&self,
                          target_desc: &Arc<AcceleratorTargetDesc>,
                          into: &mut T)
    where T: PlatformIntrinsicInsert;

  /// Insert intrinsics which depend on the kernel. For example, the Vulkan
  /// runtime can run many types of graphics "kernels" in addition to the
  /// usual `GlCompute` shader. However which `ExeModel` used (currently)
  /// depends on which special vulkan intrinsic is called to get the "kernel"
  /// instance.
  /// This functions serves to allow adding those intrinsics to the driver.
  fn insert_kernel_intrinsics<T>(&self,
                                 _kernel: &PKernelDesc<Self>,
                                 _into: &mut T)
    where T: PlatformIntrinsicInsert
  { }

  /// Get the platform specific codegen root. This type will at the very least
  /// contain the DefId of the kernel. Extra info will likely include statically
  /// computable info about the kernel (eg required capabilities for VK).
  fn root<'tcx>(&self,
                desc: PKernelDesc<Self>,
                instance: Instance<'tcx>,
                tcx: TyCtxt<'tcx>,
                dd: &DriverData<'tcx, Self>)
    -> Result<PCodegenDesc<'tcx, Self>, <Self::Device as crate::Device>::Error>;

  /// Compute the list of conditions to be used. Called once before codegen
  /// is started.
  fn root_conditions<'tcx>(&self,
                           root: &PCodegenDesc<'tcx, Self>,
                           tcx: TyCtxt<'tcx>,
                           dd: &DriverData<'tcx, Self>)
    -> Result<Vec<Self::Condition>, <Self::Device as crate::Device>::Error>;

  /// Called once the `tcx` is available. Use to populate DataDriver Stuff
  /// ahead of any of the query system queries, like types of generated
  /// DefIds, or adding extra kernel roots for device side enqueue.
  fn pre_codegen<'tcx>(&self,
                       tcx: TyCtxt<'tcx>,
                       dd: &DriverData<'tcx, Self>)
    -> Result<(), <Self::Device as crate::Device>::Error>;

  /// Take the "raw" codegen results (typically in the form of LLVM bitcode),
  /// and run it through any platform specific *codegen* steps. This doesn't
  /// include loading into the platform runtime.
  /// The target should insert a `OutputType::Exe` entry into
  /// `codegen.outputs` with the results.
  /// Currently run inside the codegen worker, however in the future this will
  /// likely be sent to a thread pool.
  fn post_codegen(&self,
                  target_desc: &Arc<AcceleratorTargetDesc>,
                  tdir: &Path,
                  codegen: &mut PCodegenResults<Self>)
    -> Result<(), <Self::Device as crate::Device>::Error>;

  // The following are all overrides for queries.

  /// Modify the provided `attrs` to suit platforms requirement, including
  /// filtering `geobacter_cfg_attr` based on present conditions.
  /// `attrs` is generated by vanilla Rustc.
  fn item_attrs<'tcx>(&self,
                      tcx: TyCtxt<'tcx>,
                      dd: &DriverData<'tcx, Self>,
                      _def_id: DefId,
                      attrs: Lrc<[ast::Attribute]>)
    -> Lrc<[ast::Attribute]>
  {
    let conditions = dd.root_conditions();
    geobacter_cfg_attrs(tcx, attrs, &*conditions)
  }

  /// Modify the provided `attrs` to suit the needs of the platform.
  /// `attrs` is generated by vanilla Rustc.
  fn codegen_fn_attrs<'tcx>(&self,
                            _tcx: TyCtxt<'tcx>,
                            _dd: &DriverData<'tcx, Self>,
                            _id: DefId,
                            _attrs: &mut CodegenFnAttrs)
  { }
}
