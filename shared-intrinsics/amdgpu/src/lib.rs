
#![feature(rustc_private, platform_intrinsics,
           rustc_diagnostic_macros)]
#![feature(link_llvm_intrinsics)]
#![feature(intrinsics)]
#![feature(unboxed_closures)]

// Note: don't try to depend on `legionella_std` or the amdgpu runtime crate.

// TODO move the workitem id stuff into the common intrinsics crate, as
// they will be provided in some form by every platform.

extern crate rustc;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_metadata;
extern crate rustc_mir;
extern crate rustc_codegen_utils;
extern crate rustc_data_structures;
extern crate rustc_target;
extern crate syntax;
extern crate syntax_pos;

#[macro_use]
extern crate log;

extern crate hsa_core;
extern crate rustc_intrinsics;
extern crate legionella_intrinsics_common as common;

use std::fmt;

use hsa_core::kernel::{KernelId, OptionalFn, KernelInstance};

use crate::rustc::mir::{self, CustomIntrinsicMirGen, };
use crate::rustc::ty::{self, TyCtxt, Instance, InstanceDef, };
use crate::rustc_data_structures::sync::{Lrc, };

use crate::common::{DefIdFromKernelId, LegionellaCustomIntrinsicMirGen,
                    stubbing, GetDefIdFromKernelId, LegionellaMirGen, };

pub mod attrs;

pub fn insert_all_intrinsics<F, U>(marker: &U, mut into: F)
  where F: FnMut(String, Lrc<dyn CustomIntrinsicMirGen>),
        U: GetDefIdFromKernelId + Send + Sync + 'static,
{
  for intr in AxisId::permutations() {
    let (k, v) = LegionellaMirGen::new(intr, marker);
    into(k, v);
  }
  let (k, v) = LegionellaMirGen::new(DispatchPtr, marker);
  into(k, v);
  let (k, v) = LegionellaMirGen::new(Barrier, marker);
  into(k, v);
  let (k, v) = LegionellaMirGen::new(WaveBarrier, marker);
  into(k, v);
}

// Note: there are no generic bounds for these intrinsics, so
// we don't need the full instance
fn kernel_id_for<F, Args, Ret>(f: &F) -> KernelId
  where F: Fn<Args, Output=Ret> + OptionalFn<Args, Output=Ret>,
{
  KernelInstance::get(f).kernel_id
}

#[derive(Debug, Clone, Copy)]
pub enum Dim {
  X,
  Y,
  Z,
}
impl Dim {
  fn name(&self) -> &'static str {
    match self {
      &Dim::X => "x",
      &Dim::Y => "y",
      &Dim::Z => "z",
    }
  }
}
impl fmt::Display for Dim {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.write_str(self.name())
  }
}
#[derive(Debug, Clone, Copy)]
pub enum BlockLevel {
  Item,
  Group,
}
impl BlockLevel {
  fn name(&self) -> &'static str {
    match self {
      &BlockLevel::Item => "workitem",
      &BlockLevel::Group => "workgroup",
    }
  }
}
impl fmt::Display for BlockLevel {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.write_str(self.name())
  }
}

#[derive(Debug, Clone, Copy)]
pub struct AxisId {
  block: BlockLevel,
  dim: Dim,
}
impl AxisId {
  pub fn permutations() -> Vec<Self> {
    let mut out = vec![];
    for &block in [BlockLevel::Group, BlockLevel::Item, ].into_iter() {
      for &dim in [Dim::X, Dim::Y, Dim::Z, ].into_iter() {
        out.push(AxisId {
          block,
          dim,
        });
      }
    }

    out
  }
  fn kernel_id(&self) -> KernelId {
    match self {
      &AxisId {
        block: BlockLevel::Item,
        dim: Dim::X,
      } => {
        kernel_id_for(&amdgcn_intrinsics::amdgcn_workitem_id_x)
      },
      &AxisId {
        block: BlockLevel::Item,
        dim: Dim::Y,
      } => {
        kernel_id_for(&amdgcn_intrinsics::amdgcn_workitem_id_y)
      },
      &AxisId {
        block: BlockLevel::Item,
        dim: Dim::Z,
      } => {
        kernel_id_for(&amdgcn_intrinsics::amdgcn_workitem_id_z)
      },
      &AxisId {
        block: BlockLevel::Group,
        dim: Dim::X,
      } => {
        kernel_id_for(&amdgcn_intrinsics::amdgcn_workgroup_id_x)
      },
      &AxisId {
        block: BlockLevel::Group,
        dim: Dim::Y,
      } => {
        kernel_id_for(&amdgcn_intrinsics::amdgcn_workgroup_id_y)
      },
      &AxisId {
        block: BlockLevel::Group,
        dim: Dim::Z,
      } => {
        kernel_id_for(&amdgcn_intrinsics::amdgcn_workgroup_id_z)
      },
    }
  }
  fn instance<'tcx>(&self,
                    kid_did: &dyn DefIdFromKernelId,
                    tcx: TyCtxt<'tcx>)
    -> Option<Instance<'tcx>>
  {
    // panic if not running on an AMDGPU
    match &tcx.sess.target.target.arch[..] {
      "amdgpu" => { },
      _ => { return None; },
    };

    let id = self.kernel_id();
    let def_id = kid_did.convert_kernel_id(id)
      .expect("failed to convert kernel id to def id");

    let def = InstanceDef::Intrinsic(def_id);
    let substs = tcx.intern_substs(&[]);

    Some(Instance {
      def,
      substs,
    })
  }
}
impl LegionellaCustomIntrinsicMirGen for AxisId {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   kid_did: &dyn DefIdFromKernelId,
                                   tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   mir: &mut mir::Body<'tcx>)
  {
    info!("mirgen intrinsic {}", self);

    common::redirect_or_panic(tcx, mir, move || {
      self.instance(kid_did, tcx)
    });
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    0
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    tcx.intern_type_list(&[])
  }
  /// The return type.

  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    return tcx.types.u32;
  }
}

impl fmt::Display for AxisId {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__legionella_{}_{}_id", self.block, self.dim)
  }
}

pub struct DispatchPtr;

impl DispatchPtr {
  // WOWIE. Okay, so the intrinsic wrappers can't be referenced in
  // anything other than "trivial" functions.
  // For example, if the body of this function is placed in the closure
  // passed to `redirect_or_panic` in `DispatchPtr::mirgen_simple_intrinsic`,
  // rustc tries to codegen `amdgcn_intrinsics::amdgcn_dispatch_ptr`, which
  // contains a call to the platform specific intrinsic `amdgcn_dispatch_ptr`
  // which can't be defined correctly, as on, eg, x86_64, the readonly address
  // space is not 4. LLVM then reports a fatal error b/c the intrinsic has an
  // incorrect type.
  // When referenced in a function like this, the wrapper function isn't
  // codegenned.
  fn amdgcn_kernel_id(&self) -> KernelId {
    kernel_id_for(&amdgcn_intrinsics::amdgcn_dispatch_ptr)
  }
}

impl LegionellaCustomIntrinsicMirGen for DispatchPtr {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   kid_did: &dyn DefIdFromKernelId,
                                   tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   mir: &mut mir::Body<'tcx>)
  {
    info!("mirgen intrinsic {}", self);

    common::redirect_or_panic(tcx, mir, move || {
      // panic if not running on an AMDGPU
      match &tcx.sess.target.target.arch[..] {
        "amdgpu" => { },
        _ => { return None; },
      };

      let id = self.amdgcn_kernel_id();

      let def_id = kid_did.convert_kernel_id(id)
        .expect("failed to convert kernel id to def id");
      let def = InstanceDef::Intrinsic(def_id);
      let substs = tcx.intern_substs(&[]);

      Some(Instance {
        def,
        substs,
      })
    });
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    0
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    tcx.intern_type_list(&[])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    tcx.mk_imm_ptr(tcx.types.u8)
  }
}

impl fmt::Display for DispatchPtr {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__legionella_dispatch_ptr")
  }
}

pub struct Barrier;
impl Barrier {
  fn kernel_id(&self) -> KernelId {
    kernel_id_for(&amdgcn_intrinsics::amdgcn_barrier)
  }
}
impl LegionellaCustomIntrinsicMirGen for Barrier {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   kid_did: &dyn DefIdFromKernelId,
                                   tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   mir: &mut mir::Body<'tcx>)
  {
    info!("mirgen intrinsic {}", self);

    common::redirect_or_panic(tcx, mir, move || {
      // panic if not running on an AMDGPU
      match &tcx.sess.target.target.arch[..] {
        "amdgpu" => { },
        _ => { return None; },
      };

      let id = self.kernel_id();

      let def_id = kid_did.convert_kernel_id(id)
        .expect("failed to convert kernel id to def id");
      let def = InstanceDef::Intrinsic(def_id);
      let substs = tcx.intern_substs(&[]);

      Some(Instance {
        def,
        substs,
      })
    });
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    0
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    tcx.intern_type_list(&[])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    tcx.types.unit
  }
}

impl fmt::Display for Barrier {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__legionella_amdgpu_barrier")
  }
}
pub struct WaveBarrier;
impl WaveBarrier {
  fn kernel_id(&self) -> KernelId {
    kernel_id_for(&amdgcn_intrinsics::amdgcn_wave_barrier)
  }
}
impl LegionellaCustomIntrinsicMirGen for WaveBarrier {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   kid_did: &dyn DefIdFromKernelId,
                                   tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   mir: &mut mir::Body<'tcx>)
  {
    info!("mirgen intrinsic {}", self);

    common::redirect_or_panic(tcx, mir, move || {
      // panic if not running on an AMDGPU
      match &tcx.sess.target.target.arch[..] {
        "amdgpu" => { },
        _ => { return None; },
      };

      let id = self.kernel_id();

      let def_id = kid_did.convert_kernel_id(id)
        .expect("failed to convert kernel id to def id");
      let def = InstanceDef::Intrinsic(def_id);
      let substs = tcx.intern_substs(&[]);

      Some(Instance {
        def,
        substs,
      })
    });
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    0
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    tcx.intern_type_list(&[])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    tcx.types.unit
  }
}

impl fmt::Display for WaveBarrier {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__legionella_amdgpu_wave_barrier")
  }
}

pub struct AmdGcnKillDetail;
impl common::PlatformImplDetail for AmdGcnKillDetail {
  fn kernel_id() -> KernelId {
    fn kill() -> ! {
      // the real intrinsic needs a single argument.
      amdgcn_intrinsics::amdgcn_kill(false);
    }
    kernel_id_for(&kill)
  }
}

mod amdgcn_intrinsics {
  // unsafe functions don't implement `std::opts::Fn` (for good reasons,
  // but we need them to here).
  macro_rules! def_id_intrinsic {
    (fn $name:ident($($arg:ident: $arg_ty:ty),*) $(-> $ty:ty)? => $llvm_intrinsic:literal) => (
      pub(crate) fn $name($($arg: $arg_ty),*) $(-> $ty)? {
        // Rust no longer exposes these intrinsics directly. Instead it expects
        // us to use `#[link_name]` to call them.
        extern "C" {
          #[link_name = $llvm_intrinsic]
          fn $name($($arg: $arg_ty),*) $(-> $ty)?;
        }
        unsafe { $name($($arg),*) }
      }
    )
  }

  def_id_intrinsic!(fn amdgcn_workitem_id_x() -> u32 => "llvm.amdgcn.workitem.id.x");
  def_id_intrinsic!(fn amdgcn_workitem_id_y() -> u32 => "llvm.amdgcn.workitem.id.y");
  def_id_intrinsic!(fn amdgcn_workitem_id_z() -> u32 => "llvm.amdgcn.workitem.id.z");
  def_id_intrinsic!(fn amdgcn_workgroup_id_x() -> u32 => "llvm.amdgcn.workgroup.id.x");
  def_id_intrinsic!(fn amdgcn_workgroup_id_y() -> u32 => "llvm.amdgcn.workgroup.id.y");
  def_id_intrinsic!(fn amdgcn_workgroup_id_z() -> u32 => "llvm.amdgcn.workgroup.id.z");
  def_id_intrinsic!(fn amdgcn_barrier()      => "llvm.amdgcn.s.barrier");
  def_id_intrinsic!(fn amdgcn_wave_barrier() => "llvm.amdgcn.wave.barrier");
  def_id_intrinsic!(fn amdgcn_kill(b: bool) -> ! => "llvm.amdgcn.kill");

  /// This one is an actual Rust intrinsic; the LLVM intrinsic returns
  /// a pointer in the constant address space, which we can't correctly
  /// model here in Rust land (the Rust type system has no knowledge of
  /// address spaces), so we have to have the compiler help us by inserting
  /// a cast to the flat addr space.
  pub(crate) fn amdgcn_dispatch_ptr() -> *const u8 {
    extern "rust-intrinsic" {
      fn amdgcn_dispatch_ptr() -> *const u8;
    }
    unsafe { amdgcn_dispatch_ptr() }
  }
}
