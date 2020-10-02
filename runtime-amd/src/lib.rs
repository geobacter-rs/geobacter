
//! A runtime crate for AMDGPUs. Most of this API should be considered
//! unstable; many details related to making these APIs safe are still
//! up in the air.
//!
//! Includes WIP support for management of device resources via signals
//!
//! TODO Use the `hsaKmt` API directly. The HSA runtime does some
//! global(!) locking which we could avoid because Rust is awesome. Since
//! HSA is pretty much AMD only, might as well go AMD only and program
//! their GPUs more directly.
//!

#![feature(rustc_private)]
#![feature(arbitrary_self_types)]
#![feature(alloc_layout_extra, ptr_internals)]
#![feature(core_intrinsics)]
#![feature(specialization)]
#![feature(coerce_unsized, unsize)]
#![feature(raw)]
#![feature(dropck_eyepatch)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![feature(geobacter)]

// For textures:
#![feature(repr_simd)]
#![feature(link_llvm_intrinsics)]
#![feature(simd_ffi)]
#![feature(associated_type_defaults)]

// TODO: make the Geobacter attributes "known" to rustc.
#![feature(register_attr)]
#![register_attr(geobacter, geobacter_attr)]

#![allow(incomplete_features)]
#![feature(const_generics)]
// #![warn(incomplete_features)] XXX can't just allow ^

extern crate any_key;
extern crate goblin;
extern crate tracing as log;
extern crate serde;
extern crate rmp_serde as rmps;

extern crate rustc_ast;
extern crate rustc_attr;
extern crate rustc_data_structures;
extern crate rustc_geobacter;
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_target;
extern crate rustc_serialize;
extern crate rustc_span;

extern crate hsa_rt;
extern crate geobacter_runtime_core as grt_core;

#[allow(unused_imports)]
#[macro_use]
extern crate geobacter_runtime_amd_macros;
#[doc(hidden)]
pub use geobacter_runtime_amd_macros::*;

use std::any::Any;
use std::cell::UnsafeCell;
use std::cmp::max;
use std::collections::{BTreeMap, };
use std::convert::*;
use std::error::{Error as StdError};
use std::fmt;
use std::geobacter::platform::{Platform, hsa, };
use std::geobacter::platform::hsa::AmdGcn;
use std::ptr::{NonNull, };
use std::str::FromStr;
use std::sync::{Arc, };

use log::{info, warn, error, };

use serde::{Deserialize, Serialize, };

use smallvec::SmallVec;

use hsa_rt::ApiContext;
use hsa_rt::agent::{Agent, Profiles, DefaultFloatRoundingModes, IsaInfo,
                    DeviceType, Feature, };
use hsa_rt::code_object::CodeObjectReaderRef;
pub use hsa_rt::error::Error as HsaError;
use hsa_rt::executable::{Executable, CommonExecutable};
use hsa_rt::ext::amd::{MemoryPool, MemoryPoolPtr, async_copy, unlock_memory,
                       MemoryPoolAlloc, GlobalFlags, };
use hsa_rt::mem::region::{RegionAlloc, };
use hsa_rt::queue::{KernelSingleQueue, KernelMultiQueue};
use hsa_rt::signal::{Signal, SignalBinops};

use grt_core::{Accelerator, AcceleratorTargetDesc,
               PlatformTargetDesc, Device, };
use grt_core::codegen::CodegenDriver;
use grt_core::codegen::products::PCodegenResults;

use codegen::Codegenner;

pub use grt_core::AcceleratorId;
pub use grt_core::context::Context;

pub use crate::error::Error;

use crate::alloc::*;
use crate::boxed::{RawPoolBox, };
use crate::mem::*;
use crate::module::{HsaModuleData, Deps};
use crate::signal::{HostSignal, DeviceSignal, SignalHandle};

pub mod alloc;
pub mod boxed;
pub mod codegen;
pub mod error;
pub mod lds;
pub mod mem;
pub mod module;
pub mod signal;
pub mod texture;

pub mod prelude {
  pub use std::ops::{Range, RangeInclusive, RangeTo, RangeToInclusive, };

  pub use grt_core::AcceleratorId;
  pub use grt_core::context::Context;

  pub use geobacter_runtime_amd_macros::*;

  pub use crate::{lds, HsaAmdGpuAccel, };
  pub use crate::alloc::*;
  pub use crate::error::Error;
  pub use crate::mem::*;
  pub use crate::module::*;
  pub use crate::signal::{*, completion::Completion, };
  pub use crate::texture::*;
  pub use crate::lds::{
    Lds,
    Shared as LdsShared,
    Unique as LdsUnique,
    Singleton as LdsSingleton,
  };
}

mod utils;

// For `#[derive(GeobacterDeps)]`.
mod geobacter_runtime_amd {
  pub use crate::*;
}

#[derive(Debug)]
struct HsaAmdNode {
  agent: Agent,
  coarse: MemoryPoolAlloc,
  fine: Option<MemoryPoolAlloc>,
}

pub struct HsaAmdGpuAccel {
  id: AcceleratorId,

  ctx: Context,

  target_desc: Arc<AcceleratorTargetDesc>,

  platform: Platform,

  /// One for every NUMA node.
  host_nodes: Vec<HsaAmdNode>,
  device: HsaAmdNode,
  kernarg_region: RegionAlloc,

  // TODO need to create a `geobacter_runtime_host` crate
  //host_codegen: CodegenUnsafeSyncComms<Self>,
  self_codegen: Option<Arc<CodegenDriver<Codegenner>>>,
}

impl HsaAmdGpuAccel {
  pub fn new(ctx: &Context,
             host_agents: &[&Agent],
             device_agent: Agent)
    -> Result<Arc<Self>, Error>
  {
    if host_agents.len() == 0 {
      return Err(Error::NoCpuAgent);
    }
    #[cfg(target_endian = "big")] {
      if ::std::env::var_os("GEOBACTER_IGNORE_ENDIANNESS").is_none() {
        return Err(Error::UnsupportedBigEndianHost);
      } else {
        warn!("ignoring host/device endianness mismatch; you're on your own!");
      }
    }

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
      .ok_or(Error::MissingKernelArgumentsRegion)?
      .try_into()?;

    fn find_pool_flags<'a, F>(pools: &'a [MemoryPool], select_flags: F)
      -> Result<Option<MemoryPoolAlloc>, HsaError>
      where F: Fn(GlobalFlags) -> bool,
    {
      let r = pools.iter()
        .filter(|pool| {
          pool.alloc_allowed()
        })
        .filter(|pool| {
          pool.global_flags().ok()
            .and_then(|f| f )
            .map(&select_flags)
            .unwrap_or_default()
        })
        .max_by_key(|pool| pool.total_size().unwrap_or_default());
      if let Some(r) = r {
        Ok(Some(unsafe {
          r.allocator()?
        }))
      } else {
        Ok(None)
      }
    };
    fn find_fine_pool(pools: &[MemoryPool])
      -> Result<Option<MemoryPoolAlloc>, HsaError>
    {
      find_pool_flags(pools, |flags| flags.fine_grained())
    }
    fn find_coarse_pool(pools: &[MemoryPool])
      -> Result<Option<MemoryPoolAlloc>, HsaError>
    {
      find_pool_flags(pools, |flags| flags.coarse_grained())
    }

    let mut host_nodes = Vec::with_capacity(host_agents.len());
    for &agent in host_agents {
      let pools = agent.amd_memory_pools()?;
      let fine = find_fine_pool(&pools)?
        .expect("no allocatable host local fine grained global pool");
      let node = HsaAmdNode {
        agent: agent.clone(),
        fine: Some(fine),
        coarse: find_coarse_pool(&pools)?
          .expect("no allocatable host local coarse grained global pool"),
      };
      host_nodes.push(node);
    }
    let device_pools = device_agent.amd_memory_pools()?;

    let isa = device_agent.isas()?
      .get(0)
      .ok_or(Error::NoGpuAgentIsa)?
      .info()?;
    let target_desc = TargetDesc {
      isa,
    };

    let mut out = HsaAmdGpuAccel {
      id: ctx.take_accel_id(),

      ctx: ctx.clone(),

      // reinitialized later:
      platform: Platform::default(),

      target_desc: Arc::new(AcceleratorTargetDesc::new(target_desc)),

      host_nodes,
      device: HsaAmdNode {
        agent: device_agent,
        fine: find_fine_pool(&device_pools)?,
        coarse: find_coarse_pool(&device_pools)?
          .expect("no allocatable device local coarse grained global pool"),
      },
      kernarg_region,

      self_codegen: None,
    };
    out.init_target_desc()?;

    let gpu = &out.target_desc.target.options.cpu;
    let gpu = AmdGcn::from_str(gpu)
      .map_err(|()| Error::UnknownAmdGpuArch(gpu.to_string()) )?;
    out.platform = Platform::Hsa(hsa::AmdGpu::AmdGcn(gpu));

    let mut out = Arc::new(out);

    ctx.initialize_accel(&mut out)?;

    Ok(out)
  }

  /// Find and return the first GPU found.
  pub fn first_device(ctx: &Context) -> Result<Arc<Self>, Error> {
    let hsa_context = ApiContext::try_upref()?;
    let agents = hsa_context.agents()?;
    let hosts = agents.iter()
      .filter(|agent| Some(DeviceType::Cpu) == agent.device_type().ok() )
      .collect::<Vec<_>>();

    let device = agents.iter()
      .find(|agent| agent.feature().ok() == Some(Feature::Kernel))
      .ok_or(Error::NoGpuAgent)?;

    Self::new(ctx, &hosts,
              device.clone())
  }
  pub fn nth_device(ctx: &Context, n: usize) -> Result<Arc<Self>, Error> {
    let hsa_context = ApiContext::try_upref()?;
    let agents = hsa_context.agents()?;
    let hosts = agents.iter()
      .filter(|agent| Some(DeviceType::Cpu) == agent.device_type().ok() )
      .collect::<Vec<_>>();

    let agent = agents.iter()
      .filter(|agent| agent.feature().ok() == Some(Feature::Kernel))
      .nth(n)
      .ok_or(Error::NoGpuAgent)?;

    Self::new(ctx, &hosts, agent.clone())
  }
  pub fn all_devices(ctx: &Context) -> Result<Vec<Arc<Self>>, Error> {
    let hsa_context = ApiContext::try_upref()?;
    let agents = hsa_context.agents()?;
    let hosts = agents.iter()
      .filter(|agent| Some(DeviceType::Cpu) == agent.device_type().ok() )
      .collect::<Vec<_>>();

    let devices = agents.iter()
      .filter(|agent| agent.feature().ok() == Some(Feature::Kernel))
      .filter_map(|agent| {
        Self::new(ctx, &hosts, agent.clone())
          .ok()
      })
      .collect();
    Ok(devices)
  }

  pub fn ctx(&self) -> &Context { &self.ctx }
  pub fn isa_info(&self) -> &IsaInfo { self.target_desc.isa_info() }
  pub fn agent(&self) -> &Agent { &self.device.agent }
  pub fn kernargs_region(&self) -> &RegionAlloc { &self.kernarg_region }

  pub fn numa_node_len(&self) -> u32 { self.host_nodes().len() as _ }

  /// Returns an allocator interface for allocating in the provided NUMA node.
  /// This allocator will allocate coarse memory regions. GPU writes to this
  /// region *are not cache-coherent* with the CPU, thus this is unsafe.
  pub unsafe fn coarse_lap_node_alloc(&self, node: u32) -> alloc::LapAlloc {
    alloc::LapAlloc {
      id: self.id(),
      device_agent: self.agent().clone(),
      pool: self.host_nodes()[node as usize].coarse.clone(),
      accessible: UnsafeCell::new(None),
    }
  }
  /// Returns an allocator interface for allocating in the provided NUMA node
  /// This allocator will allocate fine memory regions. GPU writes to this
  /// region are cache-coherent with the CPU.
  pub fn fine_lap_node_alloc(&self, node: u32) -> alloc::LapAlloc {
    alloc::LapAlloc {
      id: self.id(),
      device_agent: self.agent().clone(),
      pool: self.host_nodes[node as usize]
        .fine
        .as_ref()
        .unwrap()
        .clone(),
      accessible: UnsafeCell::new(None),
    }
  }

  fn first_host_node(&self) -> &HsaAmdNode { &self.host_nodes()[0] }
  pub fn first_host_agent(&self) -> &Agent { &self.first_host_node().agent }
  pub fn host_pool(&self) -> &MemoryPoolAlloc {
    self.first_host_node().fine
      .as_ref().unwrap()
  }

  /// Returns the list of NUMA nodes present for this device.
  fn host_nodes(&self) -> &[HsaAmdNode] {
    &self.host_nodes
  }

  /// Returns the handle to the host allocatable global memory pool.
  /// Use this pool for allocating device local memory.
  pub fn device_pool(&self) -> &MemoryPool {
    &self.device.coarse
  }
  /// Returns the handle to the host allocatable global memory pool.
  /// Use this pool for allocating device local memory. This memory will be
  /// visible to the host.
  pub fn device_pool_fine(&self) -> Option<&MemoryPoolAlloc> {
    self.device.fine.as_ref()
  }

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
  pub unsafe fn unchecked_async_copy_into<T, U, D, CS>(&self,
                                                       from: &T,
                                                       into: &mut U,
                                                       deps: &D,
                                                       completion: &CS)
    -> Result<(), Error>
    where T: BoxPoolPtr,
          U: BoxPoolPtr,
          D: ?Sized + Deps,
          CS: SignalHandle,
  {
    let from = from.pool_ptr();
    let into = into.pool_ptr();
    if from.is_none() || into.is_none() {
      // nothing to do
      completion.signal_ref().subtract_screlease(1);
      return Ok(());
    }
    let from = from.unwrap();
    let into = into.unwrap();

    let mut signals: SmallVec<[_; 32]> = SmallVec::new();
    deps.iter_deps(&mut |dep| {
      let signal = dep.signal_ref();
      signals.push(signal);
      Ok(())
    })?;

    let dst_agent = self.agent();
    let src_agent = self.first_host_agent();

    let from_len = from.len();
    let into_len = into.len();
    let bytes = ::std::cmp::min(from_len, into_len);

    async_copy(into, from, bytes,
               dst_agent, src_agent,
               &signals, completion.signal_ref())?;

    Ok(())
  }
  pub unsafe fn unchecked_async_copy_from<T, U, D, CS>(&self,
                                                       from: &T,
                                                       into: &mut U,
                                                       deps: &D,
                                                       completion: &CS)
    -> Result<(), Error>
    where T: BoxPoolPtr,
          U: BoxPoolPtr,
          D: ?Sized + Deps,
          CS: SignalHandle,
  {
    let from = from.pool_ptr();
    let into = into.pool_ptr();
    if from.is_none() || into.is_none() {
      // nothing to do
      completion.signal_ref().subtract_screlease(1);
      return Ok(());
    }
    let from = from.unwrap();
    let into = into.unwrap();

    let mut signals: SmallVec<[_; 32]> = SmallVec::new();
    deps.iter_deps(&mut |dep| {
      let signal = dep.signal_ref();
      signals.push(signal);
      Ok(())
    })?;

    let dst_agent = self.first_host_agent();
    let src_agent = self.agent();

    let from_len = from.len();
    let into_len = into.len();
    let bytes = ::std::cmp::min(from_len, into_len);

    async_copy(into, from, bytes, dst_agent, src_agent,
               &signals, completion.signal_ref())?;

    Ok(())
  }
  pub unsafe fn unchecked_async_copy_from_p2p<T, U, D, CS>(&self,
                                                           from: &T,
                                                           into_dev: &HsaAmdGpuAccel,
                                                           into: &mut U,
                                                           deps: &D,
                                                           completion: &CS)
    -> Result<(), Error>
    where T: BoxPoolPtr,
          U: BoxPoolPtr,
          D: ?Sized + Deps,
          CS: SignalHandle,
  {
    let from = from.pool_ptr();
    let into = into.pool_ptr();
    if from.is_none() || into.is_none() {
      // nothing to do
      completion.signal_ref().subtract_screlease(1);
      return Ok(());
    }
    let from = from.unwrap();
    let into = into.unwrap();

    let mut signals: SmallVec<[_; 32]> = SmallVec::new();
    deps.iter_deps(&mut |dep| {
      let signal = dep.signal_ref();
      signals.push(signal);
      Ok(())
    })?;

    let dst_agent = into_dev.agent();
    let src_agent = self.agent();

    let from_len = from.len();
    let into_len = into.len();
    let bytes = ::std::cmp::min(from_len, into_len);

    async_copy(into, from, bytes, dst_agent, src_agent,
               &signals, completion.signal_ref())?;

    Ok(())
  }

  pub fn new_device_signal(&self, initial: signal::Value) -> Result<DeviceSignal, HsaError> {
    Signal::new(initial, &[self.agent().clone()])
      .map(|s| DeviceSignal(s, self.id()))
  }

  pub fn new_host_signal(&self, initial: signal::Value) -> Result<HostSignal, HsaError> {
    Signal::new(initial, &[self.first_host_agent().clone()])
      .map(HostSignal)
  }

  /// Allocate some device local memory. This memory may not be visible from the host,
  /// and may not be cache-coherent with host CPUs!
  ///
  /// If this returns `Ok(..)`, the pointer will not be null. The returned device
  /// memory will be uninitialized. The pointer will be aligned to the page size.
  pub unsafe fn alloc_device_local_slice<T>(&self, count: usize)
    -> Result<RawPoolBox<[T]>, Error>
    where T: Sized,
  {
    RawPoolBox::new_uninit_slice(self.device.coarse.clone(), count)
  }

  pub unsafe fn alloc_host_visible_slice<T>(self: &Arc<Self>, count: usize)
    -> Result<LapBox<[T]>, Error>
    where T: Sized + Unpin,
  {
    let mut v = LapVec::try_with_capacity_in(count,
                                             self.clone().into())?;
    v.set_len(count);
    let mut v = v.try_into_boxed_slice()?;
    v.add_access(&*self)?;
    Ok(v)
  }
  pub fn alloc_host_visible<T>(self: &Arc<Self>, v: T) -> Result<LapBox<T>, Error>
    where T: Sized,
  {
    let mut v = LapBox::new_in(v, self.clone().into());
    v.add_access(&*self)?;
    Ok(v)
  }

  /// Lock memory and give this device access. This memory is not able to be used
  /// for any async copies, sadly, due to HSA runtime limitations.
  pub unsafe fn lock_sized_to_host<T>(&self, ptr: NonNull<T>, count: usize)
    -> Result<MemoryPoolPtr<[T]>, HsaError>
    where T: Sized,
  {
    self.first_host_node().coarse
      .lock(ptr.cast(), count,
            &[self.agent().clone()])
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
    let size_range = self.agent().queue_size()?;
    let queue_size = if let Some(min) = min {
      max(size_range.start, min)
    } else {
      size_range.end / 4
    };
    let q = self.agent()
      .new_kernel_queue(queue_size, None,
                        None)?;
    Ok(q)
  }
  pub fn create_single_queue2(&self, min: Option<u32>,
                              private: u32,
                              group: u32)
    -> Result<KernelSingleQueue, HsaError>
  {
    let size_range = self.agent().queue_size()?;
    let queue_size = if let Some(min) = min {
      max(size_range.start, min)
    } else {
      size_range.end / 4
    };
    let q = self.agent()
      .new_kernel_queue(queue_size, Some(private),
                        Some(group))?;
    Ok(q)
  }
  pub fn create_multi_queue(&self, min: Option<u32>)
    -> Result<KernelMultiQueue, HsaError>
  {
    let size_range = self.agent().queue_size()?;
    let queue_size = if let Some(min) = min {
      max(size_range.start, min)
    } else {
      size_range.end / 4
    };
    let q = self.agent()
      .new_kernel_multi_queue(queue_size, None,
                              None)?;
    Ok(q)
  }
  pub fn create_multi_queue2(&self, min: Option<u32>,
                             private: u32,
                             group: u32)
    -> Result<KernelMultiQueue, HsaError>
  {
    let size_range = self.agent().queue_size()?;
    let queue_size = if let Some(min) = min {
      max(size_range.start, min)
    } else {
      size_range.end / 4
    };
    let q = self.agent()
      .new_kernel_multi_queue(queue_size, Some(private),
                              Some(group))?;
    Ok(q)
  }
}
impl Accelerator for HsaAmdGpuAccel {
  fn id(&self) -> AcceleratorId { self.id.clone() }

  fn platform(&self) -> Option<Platform> {
    Some(self.platform.clone())
  }

  fn accel_target_desc(&self) -> &Arc<AcceleratorTargetDesc> {
    &self.target_desc
  }

  fn set_accel_target_desc(&mut self, desc: Arc<AcceleratorTargetDesc>) {
    self.target_desc = desc;
  }

  fn create_target_codegen(self: &mut Arc<Self>, ctxt: &Context)
    -> Result<Arc<dyn Any + Send + Sync + 'static>, Box<dyn StdError + Send + Sync + 'static>>
    where Self: Sized,
  {
    let cg = CodegenDriver::new(ctxt,
                                self.accel_target_desc().clone(),
                                Default::default())?;
    let cg_sync = Arc::new(cg);
    Arc::get_mut(self)
      .expect("there should only be a single ref at this point")
      .self_codegen = Some(cg_sync.clone());

    cg_sync.add_accel(self);

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
}
impl Device for HsaAmdGpuAccel {
  type Error = Error;
  type Codegen = codegen::Codegenner;
  type TargetDesc = TargetDesc;
  type ModuleData = module::HsaModuleData;

  fn codegen(&self) -> &Arc<CodegenDriver<Self::Codegen>> {
    self.self_codegen
      .as_ref()
      .expect("we are uninitialized?")
  }

  fn load_kernel(self: &Arc<Self>, codegen: &PCodegenResults<Self::Codegen>)
    -> Result<Arc<Self::ModuleData>, Error>
  {
    let profiles = Profiles::base();
    let rounding_mode = DefaultFloatRoundingModes::near();
    let exe = Executable::new(profiles, rounding_mode, "")?;

    let agent = self.agent();

    {
      let exe_bin = codegen.exe_ref().unwrap();
      let exe_reader = CodeObjectReaderRef::new(exe_bin.as_ref())?;
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
          let name = codegen.entries[0].kernel_instance.name.clone();
          Error::MissingKernelSymbol(name)
        })?;
      kernel_symbol.kernel_object()?
         .ok_or(Error::UnexpectedNullKernelObject)?
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
      .field("host_nodes", &self.host_nodes)
      .field("device", &self.device)
      .field("kernargs_region", &self.kernarg_region)
      .finish()
  }
}

// private methods
impl HsaAmdGpuAccel {
  fn init_target_desc(&mut self) -> Result<(), Error> {
    use rustc_target::spec::{PanicStrategy, abi::Abi, AddrSpaceKind,
                             AddrSpaceIdx, AddrSpaceProps, CodeModel};

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

    desc.target.target_endian = desc.host_target
      .target_endian
      .clone();
    desc.target.target_pointer_width = desc.host_target
      .target_pointer_width
      .clone();

    let target = &mut desc.target;

    target.arch = "amdgpu".into();
    target.data_layout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-\
                          p4:64:64-p5:32:32-p6:32:32-i64:64-v16:16-\
                          v24:32-v32:32-v48:64-v96:128-v192:256-\
                          v256:256-v512:512-v1024:1024-v2048:2048-\
                          n32:64-S32-A5-ni:7".into();
    target.options.panic_strategy = PanicStrategy::Abort;
    target.options.trap_unreachable = true;
    target.options.position_independent_executables = true;
    target.options.dynamic_linking = true;
    target.options.executables = true;
    target.options.requires_lto = false;
    target.options.atomic_cas = true;
    target.options.default_codegen_units = Some(1);
    target.options.obj_is_bitcode = false;
    target.options.is_builtin = false;
    target.options.simd_types_indirect = false;
    target.options.stack_probes = false;
    target.options.code_model = Some(CodeModel::Small);
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
