
use std::any::Any;
use std::cmp::{Eq, PartialEq, };
use std::error::Error;
use std::fmt::{self, Debug, };
use std::hash;
use std::ops::Deref;
use std::path::{Path, };
use std::sync::Arc;

use crate::rustc::hir::{def_id::DefId, CodegenFnAttrs, };
use crate::rustc::ty::*;
use crate::rustc_data_structures::sync::{Lrc, };
use crate::syntax::ast;

use crate::gintrinsics::{GeobacterCustomIntrinsicMirGen, attrs::ConditionItem,
                         attrs::geobacter_cfg_attrs, };

use crate::geobacter_core::kernel::KernelInstance;

use crate::{Accelerator, AcceleratorTargetDesc, };
use self::products::{PlatformCodegenDesc, };
use crate::codegen::products::PCodegenResults;

pub use self::worker::error;
pub use self::worker::DriverData;
pub use self::worker::{CodegenComms, CodegenUnsafeSyncComms, };

use crate::any_key::AnyHash;

pub mod help;
pub mod worker;
pub mod products;

#[derive(Clone, Debug)]
pub struct KernelDesc<P>
  where P: PlatformKernelDesc,
{
  pub instance: KernelInstance,
  pub platform_desc: P,
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
  type Device: Accelerator;
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
    -> Result<PCodegenDesc<'tcx, Self>, Box<dyn Error + Send + Sync + 'static>>;

  /// Compute the list of conditions to be used. Called once before codegen
  /// is started.
  fn root_conditions<'tcx>(&self,
                           root: &PCodegenDesc<'tcx, Self>,
                           tcx: TyCtxt<'tcx>,
                           dd: &DriverData<'tcx, Self>)
    -> Result<Vec<Self::Condition>, Box<dyn Error + Send + Sync + 'static>>;

  /// Called once the `tcx` is available. Use to populate DataDriver Stuff
  /// ahead of any of the query system queries, like types of generated
  /// DefIds, or adding extra kernel roots for device side enqueue.
  fn pre_codegen<'tcx>(&self,
                       tcx: TyCtxt<'tcx>,
                       dd: &DriverData<'tcx, Self>)
    -> Result<(), Box<dyn Error + Send + Sync + 'static>>;

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
    -> Result<(), Box<dyn Error + Send + Sync + 'static>>;

  // The following are all overrides for queries.

  /// Modify the provided `attrs` to suit platforms requirement, including
  /// filtering `geobacter_cfg_attr` based on present conditions.
  /// `attrs` is generated by vanilla Rustc.
  fn item_attrs<'tcx>(&self,
                      tcx: TyCtxt<'tcx>,
                      dd: &DriverData<'tcx, Self>,
                      attrs: Lrc<[ast::Attribute]>)
    -> Lrc<[ast::Attribute]>
  {
    let conditions = dd.root_conditions();
    geobacter_cfg_attrs(tcx, &attrs, &*conditions)
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
