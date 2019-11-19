
//! A runtime crate for AMDGPUs.
//!
//! Includes WIP support for management of device resources via signals
//!
//! TODO Use the `hsaKmt` API directly. The HSA runtime does some
//! global(!) locking which we could avoid because Rust is awesome. Since
//! HSA is pretty much AMD only, might as well go AMD only and program
//! their GPUs more directly.
//!

#![feature(rustc_private)]
#![feature(unboxed_closures)]
#![feature(arbitrary_self_types)]
#![feature(alloc_layout_extra, ptr_internals)]
#![feature(core_intrinsics)]
#![feature(specialization)]
#![feature(coerce_unsized, unsize)]
#![feature(slice_from_raw_parts)]
#![feature(raw)]

#![allow(incomplete_features)]
#![feature(const_generics)]
// #![warn(incomplete_features)] XXX can't just allow ^

extern crate any_key;
extern crate goblin;
extern crate log;
extern crate serde;
extern crate rmp_serde as rmps;

extern crate syntax;
extern crate rustc;
extern crate rustc_target;

extern crate geobacter_core as gcore;
extern crate hsa_rt;
extern crate geobacter_runtime_core as grt_core;
extern crate geobacter_shared_defs as shared_defs;
extern crate geobacter_intrinsics_common as intrinsics_common;
extern crate geobacter_amdgpu_intrinsics as intrinsics;

#[allow(unused_imports)]
#[macro_use]
extern crate geobacter_runtime_amd_macros;
#[doc(hidden)]
pub use geobacter_runtime_amd_macros::*;

use std::any::Any;
use std::cmp::max;
use std::collections::{BTreeMap, };
use std::error::Error;
use std::fmt;
use std::ptr::{NonNull, };
use std::str::FromStr;
use std::sync::{Arc, };

use log::{info, warn, };

use serde::{Deserialize, Serialize, };

use hsa_rt::ApiContext;
use hsa_rt::agent::{Agent, Profiles, DefaultFloatRoundingModes, IsaInfo,
                    DeviceType, Feature, };
use hsa_rt::code_object::CodeObjectReaderRef;
pub use hsa_rt::error::Error as HsaError;
use hsa_rt::executable::{Executable, CommonExecutable};
use hsa_rt::ext::amd::{MemoryPool, MemoryPoolPtr, AgentAccess, async_copy, unlock_memory};
use hsa_rt::mem::region::{Region, Segment};
use hsa_rt::queue::{KernelSingleQueue, KernelMultiQueue};
use hsa_rt::signal::{SignalRef, Signal};

use grt_core::{Accelerator, AcceleratorTargetDesc,
               PlatformTargetDesc, Device, };
use grt_core::codegen::{CodegenComms, CodegenUnsafeSyncComms, };
use grt_core::codegen::products::PCodegenResults;
use shared_defs::platform::{Platform, host_platform, hsa, };
use shared_defs::platform::hsa::AmdGpu;

use codegen::Codegenner;
use crate::module::HsaModuleData;

pub use grt_core::AcceleratorId;
pub use grt_core::context::Context;

use crate::async_copy::CopyDataObject;
use crate::boxed::{RawPoolBox, LocallyAccessiblePoolBox};
use crate::signal::{HostSignal, DeviceSignal, DeviceConsumable, SignalHandle};

pub mod async_copy;
pub mod boxed;
pub mod codegen;
pub mod module;
pub mod signal;

pub struct HsaAmdGpuAccel {
  id: AcceleratorId,

  ctx: Context,

  target_desc: Arc<AcceleratorTargetDesc>,

  platform: Platform,

  host_agent: Agent,
  host_lock_pool: MemoryPool,

  device_agent: Agent,
  kernarg_region: Region,
  alloc_pool: MemoryPool,

  // TODO need to create a `geobacter_runtime_host` crate
  //host_codegen: CodegenUnsafeSyncComms<Self>,
  self_codegen: Option<Arc<CodegenUnsafeSyncComms<Codegenner>>>,
}

impl HsaAmdGpuAccel {
  pub fn new(ctx: &Context,
             host_agent: Agent,
             device_agent: Agent)
    -> Result<Arc<Self>, Box<dyn Error>>
  {
    let kernarg_region = device_agent.all_regions()?
      .into_iter()
      .filter(|region| {
        region.runtime_alloc_allowed()
          .unwrap_or(false)
      })
      .find(|region| {
        match region.global_flags() {
          Ok(Some(flags)) => flags.kernel_arg() && flags.fine_grained(),
          _ => false,
        }
      })
      .ok_or_else(|| "no kernel argument region")?;

    let host_lock_pool = host_agent.amd_memory_pools()?
      .into_iter()
      .filter(|pool| {
        pool.alloc_allowed()
      })
      .filter(|pool| {
        pool.segment()
          .map(|seg| seg == Segment::Global)
          .unwrap_or_default()
      })
      .max_by_key(|pool| pool.total_size().unwrap_or_default())
      .ok_or_else(|| "no allocatable host local global pool")?;

    let alloc_pool = device_agent.amd_memory_pools()?
      .into_iter()
      .filter(|pool| {
        pool.alloc_allowed()
      })
      .find(|pool| {
        pool.segment()
          .map(|seg| seg == Segment::Global)
          .unwrap_or_default()
      })
      .ok_or_else(|| "no allocatable device local global pool")?;

    let isa = device_agent.isas()?
      .get(0)
      .ok_or("device has no available ISA")?
      .info()?;
    let target_desc = TargetDesc {
      isa,
    };

    let mut out = HsaAmdGpuAccel {
      id: ctx.take_accel_id()?,

      ctx: ctx.clone(),

      // reinitialized later:
      platform: host_platform(),

      target_desc: Arc::new(AcceleratorTargetDesc::new(target_desc)),

      host_agent,
      host_lock_pool,

      device_agent,
      kernarg_region,
      alloc_pool,

      self_codegen: None,
    };
    out.init_target_desc()?;

    let gpu = &out.target_desc.target.options.cpu;
    let gpu = AmdGpu::from_str(gpu)
      .map_err(|()| format!("unknown AMDGPU: {}", gpu))?;
    out.platform = Platform::Hsa(hsa::Device::AmdGcn(gpu));

    let mut out = Arc::new(out);

    ctx.initialize_accel(&mut out)?;

    Ok(out)
  }

  /// Find and return the first GPU found.
  pub fn first_device(ctx: &Context) -> Result<Arc<Self>, Box<dyn Error>> {
    let hsa_context = ApiContext::try_upref()?;
    let agents = hsa_context.agents()?;
    let host = agents.iter()
      .find(|agent| Some(DeviceType::Cpu) == agent.device_type().ok())
      .ok_or("no CPU agent found")?;
    let device = agents.iter()
      .find(|agent| agent.feature().ok() == Some(Feature::Kernel))
      .ok_or("no accelerator found")?;

    Self::new(ctx, host.clone(),
              device.clone())
  }
  pub fn nth_device(ctx: &Context, n: usize) -> Result<Arc<Self>, Box<dyn Error>> {
    let hsa_context = ApiContext::try_upref()?;
    let agents = hsa_context.agents()?;
    let host = agents.iter()
      .find(|agent| Some(DeviceType::Cpu) == agent.device_type().ok())
      .ok_or("no CPU agent found")?
      .clone();

    let agent = agents.into_iter()
      .filter(|agent| agent.feature().ok() == Some(Feature::Kernel))
      .nth(n)
      .ok_or("404")?;

    Self::new(ctx, host, agent)
  }
  pub fn all_devices(ctx: &Context) -> Result<Vec<Arc<Self>>, Box<dyn Error>> {
    let hsa_context = ApiContext::try_upref()?;
    let agents = hsa_context.agents()?;
    let host = agents.iter()
      .find(|agent| Some(DeviceType::Cpu) == agent.device_type().ok())
      .ok_or("no CPU agent found")?
      .clone();
    let devices = agents.into_iter()
      .filter(|agent| agent.feature().ok() == Some(Feature::Kernel))
      .filter_map(|agent| {
        Self::new(ctx, host.clone(), agent).ok()
      })
      .collect();
    Ok(devices)
  }

  pub fn ctx(&self) -> &Context { &self.ctx }
  pub fn isa_info(&self) -> &IsaInfo { self.target_desc.isa_info() }
  pub fn agent(&self) -> &Agent { &self.device_agent }
  pub fn kernargs_region(&self) -> &Region { &self.kernarg_region }

  pub fn host_pool(&self) -> &MemoryPool { &self.host_lock_pool }

  /// Returns the handle to the host allocatable global memory pool.
  /// Use this pool for allocating device local memory.
  pub fn device_pool(&self) -> &MemoryPool { &self.alloc_pool }

  /// Asynchronously copy `from` bytes from the host locked device pointer into the
  /// device local `into`. `into` should be a region in one of this device's memory
  /// pools. You must call `MemoryPoolPtr::grant_agent_access` yourself as needed, and you
  /// must also *not* ungrant access until the copy is complete; generally this happens
  /// by calling `MemoryPoolPtr::grant_agent_access` again without including this agent
  /// in the list of agents.
  ///
  /// Unsafe because this device could be using the destination. Its safe only
  /// when signals are used to ensure every workitem of every dispatch is finished
  /// with the destination resource (`into`).
  ///
  /// This function does not ensure all the provided signals live as long as they need
  /// to. You will also need to ensure the `deps` signals are able to be waited on
  /// by this device.
  ///
  /// Additionally, you must ensure the last dispatch using `from` has run a system
  /// scope release memory fence beforehand (in this case you must perform a
  /// atomic release fence on the host) AND the receiving device.
  ///
  /// If the dep or completion signals are dropped before the async copy finishes,
  /// then the result is undefined.
  ///
  /// The HSA runtime internally uses a device queue to implement waiting on `deps`.
  pub unsafe fn unchecked_async_copy_into<T, U, R, CS>(&self,
                                                       from: T,
                                                       into: U,
                                                       deps: &[&dyn DeviceConsumable],
                                                       completion: &CS)
    -> Result<(), Box<dyn Error>>
    where T: CopyDataObject<R>,
          U: CopyDataObject<R>,
          R: ?Sized + Unpin,
          CS: SignalHandle,
  {
    let from = from.pool_copy_region();
    let into = into.pool_copy_region();
    if from.is_none() || into.is_none() {
      // nothing to do
      completion.signal_ref().subtract_screlease(1);
      return Ok(());
    }
    let from = from.unwrap();
    let into = into.unwrap();

    // check that `into` is in our pool.
    match into.pool().agent_access(self.agent())? {
      AgentAccess::DefaultAllowed => {},
      _ => {
        return Err("destination pool is not owned by this device".into());
      },
    }
    // ditto `from`. `from` has to be locked, and should be a device pointer.
    match from.pool().agent_access(self.agent())? {
      AgentAccess::DefaultAllowed => {},
      AgentAccess::DefaultDisallowed => {
        // we assume the user has granted us access. We'll error in `async_copy` if
        // not.
      },
      access => {
        return Err(format!("source pool is not accessible by this device: {:?}",
                           access).into());
      },
    }

    // TODO avoid allocation.
    let deps: Vec<&SignalRef> = deps.iter().map(|dep| dep.signal_ref()).collect();

    let dst_agent = self.agent();
    let src_agent = &self.host_agent;

    let from_len = from.len();
    let into_len = into.len();
    let bytes = ::std::cmp::min(from_len, into_len);

    async_copy(into, from, bytes,
               dst_agent, src_agent,
               &deps, completion.signal_ref())?;

    Ok(())
  }
  pub unsafe fn unchecked_async_copy_from<T, U, R, CS>(&self,
                                                       from: T,
                                                       into: U,
                                                       deps: &[&dyn DeviceConsumable],
                                                       completion: &CS)
    -> Result<(), Box<dyn Error>>
    where T: CopyDataObject<R>,
          U: CopyDataObject<R>,
          R: ?Sized + Unpin,
          CS: SignalHandle,
  {
    let from = from.pool_copy_region();
    let into = into.pool_copy_region();
    if from.is_none() || into.is_none() {
      // nothing to do
      completion.signal_ref().subtract_screlease(1);
      return Ok(());
    }
    let from = from.unwrap();
    let into = into.unwrap();

    match into.pool().agent_access(self.agent())? {
      AgentAccess::DefaultAllowed => {},
      AgentAccess::DefaultDisallowed => {
        // we assume the user has granted us access. We'll error in `async_copy` if
        // not.
      },
      _ => {
        return Err("destination pool is not accessible by this device".into());
      },
    }
    match from.pool().agent_access(self.agent())? {
      AgentAccess::DefaultAllowed => {},
      access => {
        return Err(format!("source pool is not owned by this device: {:?}",
                           access).into());
      },
    }

    // TODO avoid allocation.
    let deps: Vec<&SignalRef> = deps.iter().map(|dep| dep.signal_ref()).collect();

    let dst_agent = &self.host_agent;
    let src_agent = self.agent();

    let from_len = from.len();
    let into_len = into.len();
    let bytes = ::std::cmp::min(from_len, into_len);

    async_copy(into, from, bytes, dst_agent, src_agent,
               &deps, completion.signal_ref())?;

    Ok(())
  }
  pub unsafe fn unchecked_async_copy_from_p2p<T, U, R, CS>(&self,
                                                           from: T,
                                                           into_dev: &HsaAmdGpuAccel,
                                                           into: U,
                                                           deps: &[&dyn DeviceConsumable],
                                                           completion: &CS)
    -> Result<(), Box<dyn Error>>
    where T: CopyDataObject<R>,
          U: CopyDataObject<R>,
          R: ?Sized + Unpin,
          CS: SignalHandle,
  {
    let from = from.pool_copy_region();
    let into = into.pool_copy_region();
    if from.is_none() || into.is_none() {
      // nothing to do
      completion.signal_ref().subtract_screlease(1);
      return Ok(());
    }
    let from = from.unwrap();
    let into = into.unwrap();

    // check that `into` is in our pool.
    match into.pool().agent_access(self.agent())? {
      AgentAccess::DefaultAllowed => {},
      AgentAccess::DefaultDisallowed => {
        // we assume the user has granted us access. We'll error in `async_copy` if
        // not.
      },
      access => {
        return Err(format!("destination pool is not accessible by this device: {:?}",
                           access).into());
      },
    }
    // ditto `from`. `from` has to be locked, and should be a device pointer.
    match from.pool().agent_access(self.agent())? {
      AgentAccess::DefaultAllowed => {},
      AgentAccess::DefaultDisallowed => {
        // we assume the user has granted us access. We'll error in `async_copy` if
        // not
      },
      access => {
        return Err(format!("source pool is not accessible by this device: {:?}",
                           access).into());
      },
    }

    // TODO avoid allocation.
    let deps: Vec<&SignalRef> = deps.iter().map(|dep| dep.signal_ref()).collect();

    let dst_agent = into_dev.agent();
    let src_agent = self.agent();

    let from_len = from.len();
    let into_len = into.len();
    let bytes = ::std::cmp::min(from_len, into_len);

    async_copy(into, from, bytes, dst_agent, src_agent,
               &deps, completion.signal_ref())?;

    Ok(())
  }

  pub fn new_device_signal(&self, initial: signal::Value) -> Result<DeviceSignal, HsaError> {
    Signal::new(initial, &[self.device_agent.clone()])
      .map(|s| DeviceSignal(s, self.id()))
  }

  pub fn new_host_signal(&self, initial: signal::Value) -> Result<HostSignal, HsaError> {
    Signal::new(initial, &[self.host_agent.clone()])
      .map(HostSignal)
  }

  /// Allocate some device local memory. This memory may not be visible from the host,
  /// and may not be cache-coherent with host CPUs!
  ///
  /// If this returns `Ok(..)`, the pointer will not be null. The returned device
  /// memory will be uninitialized. The pointer will be aligned to the page size.
  pub unsafe fn alloc_device_local_slice<T>(&self, count: usize)
    -> Result<RawPoolBox<[T]>, HsaError>
    where T: Sized,
  {
    RawPoolBox::new_uninit_slice(self.alloc_pool.clone(),
                                 count)
  }

  pub unsafe fn alloc_host_visible_slice<T>(&self, count: usize)
    -> Result<LocallyAccessiblePoolBox<[T]>, HsaError>
    where T: Sized,
  {
    let rb = RawPoolBox::new_uninit_slice(self.host_lock_pool.clone(),
                                          count)?;
    Ok(LocallyAccessiblePoolBox::from_raw_box_unchecked(rb))
  }
  pub fn alloc_host_visible<T>(&self, v: T) -> Result<LocallyAccessiblePoolBox<T>, HsaError>
    where T: Sized,
  {
    let rb = unsafe { RawPoolBox::new_uninit(self.host_lock_pool.clone())? };
    let mut lapb: LocallyAccessiblePoolBox<T> = unsafe {
      LocallyAccessiblePoolBox::from_raw_box_unchecked(rb)
    };
    unsafe { lapb.overwrite(v); }
    Ok(lapb)
  }

  /// Lock memory and give this device access. This memory is not able to be used
  /// for any async copies, sadly, due to HSA runtime limitations.
  pub unsafe fn lock_sized_to_host<T>(&self, ptr: NonNull<T>, count: usize)
    -> Result<MemoryPoolPtr<[T]>, HsaError>
    where T: Sized,
  {
    self.host_lock_pool.lock(ptr.cast(), count,
                             &[self.device_agent.clone()])
  }
  pub unsafe fn unlock_sized_from_host<T>(&self, ptr: NonNull<T>, count: usize)
    -> Result<(), HsaError>
    where T: Sized,
  {
    unlock_memory(ptr, count)
  }

  pub fn create_single_queue(&self, min: Option<u32>)
    -> Result<KernelSingleQueue, HsaError>
  {
    let size_range = self.device_agent.queue_size()?;
    let queue_size = if let Some(min) = min {
      max(size_range.start, min)
    } else {
      size_range.end / 4
    };
    let q = self.device_agent
      .new_kernel_queue(queue_size, None,
                        None)?;
    Ok(q)
  }
  pub fn create_single_queue2(&self, min: Option<u32>,
                              private: u32,
                              group: u32)
    -> Result<KernelSingleQueue, HsaError>
  {
    let size_range = self.device_agent.queue_size()?;
    let queue_size = if let Some(min) = min {
      max(size_range.start, min)
    } else {
      size_range.end / 4
    };
    let q = self.device_agent
      .new_kernel_queue(queue_size, Some(private),
                        Some(group))?;
    Ok(q)
  }
  pub fn create_multi_queue(&self, min: Option<u32>)
    -> Result<KernelMultiQueue, HsaError>
  {
    let size_range = self.device_agent.queue_size()?;
    let queue_size = if let Some(min) = min {
      max(size_range.start, min)
    } else {
      size_range.end / 4
    };
    let q = self.device_agent
      .new_kernel_multi_queue(queue_size, None,
                              None)?;
    Ok(q)
  }
  pub fn create_multi_queue2(&self, min: Option<u32>,
                             private: u32,
                             group: u32)
    -> Result<KernelMultiQueue, HsaError>
  {
    let size_range = self.device_agent.queue_size()?;
    let queue_size = if let Some(min) = min {
      max(size_range.start, min)
    } else {
      size_range.end / 4
    };
    let q = self.device_agent
      .new_kernel_multi_queue(queue_size, Some(private),
                              Some(group))?;
    Ok(q)
  }
}
impl Accelerator for HsaAmdGpuAccel {
  fn id(&self) -> AcceleratorId { self.id.clone() }

  fn platform(&self) -> Platform {
    self.platform.clone()
  }

  fn accel_target_desc(&self) -> &Arc<AcceleratorTargetDesc> {
    &self.target_desc
  }

  fn set_accel_target_desc(&mut self, desc: Arc<AcceleratorTargetDesc>) {
    self.target_desc = desc;
  }

  fn create_target_codegen(self: &mut Arc<Self>, ctxt: &Context)
    -> Result<Arc<dyn Any + Send + Sync + 'static>, Box<dyn Error>>
    where Self: Sized,
  {
    let cg = CodegenComms::new(ctxt,
                               self.accel_target_desc().clone(),
                                Default::default())?;
    let cg_sync = Arc::new(unsafe { cg.clone().sync_comms() });
    Arc::get_mut(self)
      .expect("there should only be a single ref at this point")
      .self_codegen = Some(cg_sync.clone());

    cg.add_accel(self);

    Ok(cg_sync)
  }

  fn set_target_codegen(self: &mut Arc<Self>,
                        codegen_comms: Arc<dyn Any + Send + Sync + 'static>)
    where Self: Sized,
  {
    let cg = codegen_comms
      .downcast()
      .expect("unexpected codegen type?");

    Arc::get_mut(self)
      .expect("there should only be a single ref at this point")
      .self_codegen = Some(cg);

    self.codegen().add_accel(self);
  }

  fn downcast_ref(this: &dyn Accelerator) -> Option<&Self>
    where Self: Sized,
  {
    use std::any::TypeId;
    use std::mem::transmute;
    use std::raw::TraitObject;

    if this.type_id() != TypeId::of::<Self>() {
      return None;
    }

    // We have to do this manually.
    let this: TraitObject = unsafe { transmute(this) };
    let this = this.data as *mut Self;
    Some(unsafe { &*this })
  }
  fn downcast_arc(this: &Arc<dyn Accelerator>) -> Option<Arc<Self>>
    where Self: Sized,
  {
    use std::any::TypeId;
    use std::mem::transmute;
    use std::raw::TraitObject;

    if this.type_id() != TypeId::of::<Self>() {
      return None;
    }

    // We have to do this manually.
    let this = this.clone();
    let this = Arc::into_raw(this);
    let this: TraitObject = unsafe { transmute(this) };
    let this = this.data as *mut Self;
    Some(unsafe { Arc::from_raw(this) })
  }
}
impl Device for HsaAmdGpuAccel {
  type Codegen = codegen::Codegenner;
  type TargetDesc = TargetDesc;
  type ModuleData = module::HsaModuleData;

  fn codegen(&self) -> CodegenComms<Self::Codegen> {
    (&**self.self_codegen
      .as_ref()
      .expect("we are uninitialized?"))
      .into()
  }

  fn load_kernel(self: &Arc<Self>, codegen: &PCodegenResults<Self::Codegen>)
    -> Result<Arc<Self::ModuleData>, Box<dyn Error>>
  {
    let profiles = Profiles::base();
    let rounding_mode = DefaultFloatRoundingModes::near();
    let exe = Executable::new(profiles, rounding_mode, "")?;

    let agent = self.agent();

    {
      let exe_bin = codegen.exe_ref().unwrap();
      let exe_reader = CodeObjectReaderRef::new(exe_bin.as_ref())
        .expect("CodeObjectReaderRef::new");

      exe.load_agent_code_object(agent, &exe_reader, "")?;
    }
    let exe = exe.freeze("")?;

    let root = codegen.root();

    let main_object = {
      let symbols = exe.agent_symbols(agent)?;
      let kernel_symbol = symbols.into_iter()
        .filter(|symbol| {
          symbol.is_kernel()
        })
        .find(|symbol| {
          match symbol.name() {
            Ok(ref n) if n == &root.symbol => { true },
            Ok(n) => {
              info!("ignoring symbol {}", n);
              false
            },
            Err(_) => {
              warn!("unnamed symbol; skipping");
              false
            },
          }
        })
        .ok_or_else(|| {
          format!("failed to find {}", root.symbol)
        })?;
      kernel_symbol.kernel_object()?
         .ok_or_else(|| "unexpected 0 for kernel object id" )?
    };

    Ok(Arc::new(HsaModuleData {
      agent: agent.clone(),
      exe,
      kernel_object: main_object,
      desc: root.platform.clone(),
    }))
  }
}
impl fmt::Debug for HsaAmdGpuAccel {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.debug_struct("HsaAmdGpuAccel")
      .field("id", &self.id)
      .field("target_desc", &self.target_desc)
      .field("platform", &self.platform)
      .field("host_agent", &self.host_agent)
      .field("device_agent", &self.device_agent)
      .field("kernargs_region", &self.kernarg_region)
      .finish()
  }
}

// private methods
impl HsaAmdGpuAccel {
  fn init_target_desc(&mut self) -> Result<(), Box<dyn Error>> {
    use rustc_target::spec::{PanicStrategy, abi::Abi, AddrSpaceKind,
                             AddrSpaceIdx, AddrSpaceProps, };

    let desc = Arc::get_mut(&mut self.target_desc).unwrap();

    desc.allow_indirect_function_calls = true;
    desc.kernel_abi = Abi::AmdGpuKernel;

    // we get the triple and gpu "cpu" from the name of the isa:
    let cpu = {
      let triple = &desc.isa_name();
      let idx = triple.rfind('-')
        .expect("expected at least one hyphen in the AMDGPU ISA name");
      assert_ne!(idx, triple.len(),
                 "AMDGPU ISA target triple has no cpu model, or something else weird");
      triple[idx + 1..].into()
    };
    desc.target.llvm_target = "amdgcn-amd-amdhsa-amdgiz".into();
    desc.target.options.cpu = cpu;

    desc.target.options.features = "+dpp,+s-memrealtime".into();
    desc.target.options.features.push_str(",+code-object-v3");
    if desc.isa_info().fast_f16 {
      desc.target.options.features.push_str(",+16-bit-insts");
    }


    let target = &mut desc.target;

    target.target_endian = "little".into(); // XXX big?
    target.target_pointer_width = "64".into(); // XXX 32bit?
    target.arch = "amdgpu".into();
    target.data_layout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-\
                          p4:64:64-p5:32:32-p6:32:32-i64:64-v16:16-\
                          v24:32-v32:32-v48:64-v96:128-v192:256-\
                          v256:256-v512:512-v1024:1024-v2048:2048-\
                          n32:64-S32-A5-ni:7".into();
    target.options.codegen_backend = "llvm".into();
    target.options.custom_unwind_resume = false;
    target.options.panic_strategy = PanicStrategy::Abort;
    target.options.trap_unreachable = true;
    target.options.position_independent_executables = true;
    target.options.dynamic_linking = true;
    target.options.executables = true;
    target.options.requires_lto = false;
    target.options.atomic_cas = true;
    target.options.default_codegen_units = Some(1);
    target.options.obj_is_bitcode = false;
    target.options.is_builtin = true;
    target.options.simd_types_indirect = false;
    target.options.stack_probes = false;
    {
      let addr_spaces = &mut target.options.addr_spaces;
      addr_spaces.clear();

      let flat = AddrSpaceKind::Flat;
      let flat_idx = AddrSpaceIdx(0);

      let global = AddrSpaceKind::ReadWrite;
      let global_idx = AddrSpaceIdx(1);

      let region = AddrSpaceKind::from_str("region").unwrap();
      let region_idx = AddrSpaceIdx(2);

      let local = AddrSpaceKind::from_str("local").unwrap();
      let local_idx = AddrSpaceIdx(3);

      let constant = AddrSpaceKind::ReadOnly;
      let constant_idx = AddrSpaceIdx(4);

      let private = AddrSpaceKind::Alloca;
      let private_idx = AddrSpaceIdx(5);

      let constant_32b = AddrSpaceKind::from_str("32bit constant").unwrap();
      let constant_32b_idx = AddrSpaceIdx(6);

      let props = AddrSpaceProps {
        index: flat_idx,
        shared_with: vec![private.clone(),
                          region.clone(),
                          local.clone(),
                          constant.clone(),
                          global.clone(),
                          constant_32b.clone(), ]
          .into_iter()
          .collect(),
      };
      addr_spaces.insert(flat.clone(), props);

      let insert_as = |addr_spaces: &mut BTreeMap<_, _>, kind,
                       idx| {
        let props = AddrSpaceProps {
          index: idx,
          shared_with: vec![flat.clone()]
            .into_iter()
            .collect(),
        };
        addr_spaces.insert(kind, props);
      };
      insert_as(addr_spaces, global.clone(), global_idx);
      insert_as(addr_spaces, region.clone(), region_idx);
      insert_as(addr_spaces, local.clone(), local_idx);
      insert_as(addr_spaces, constant.clone(), constant_idx);
      insert_as(addr_spaces, private.clone(), private_idx);
      insert_as(addr_spaces, constant_32b.clone(), constant_32b_idx);
    }

    Ok(())
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct TargetDesc {
  isa: IsaInfo,
}
impl PlatformTargetDesc for TargetDesc {
  fn as_any_hash(&self) -> &dyn any_key::AnyHash {
    self
  }
}

pub trait HsaAmdTargetDescHelper {
  fn platform_data(&self) -> &TargetDesc;

  fn isa_info(&self) -> &IsaInfo { &self.platform_data().isa }
  fn isa_name(&self) -> &str { &self.isa_info().name }
}
impl HsaAmdTargetDescHelper for AcceleratorTargetDesc {
  fn platform_data(&self) -> &TargetDesc {
    TargetDesc::downcast_ref(&*self.platform)
      .expect("accelerator target isn't HSA")
  }
}
