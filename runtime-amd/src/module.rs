
use std::alloc::Layout;
use std::error::Error;
use std::fmt;
use std::marker::{PhantomData, Unsize, };
use std::mem::{transmute, size_of, };
use std::num::NonZeroU64;
use std::ops::{CoerceUnsized, Deref, DerefMut, };
use std::ptr::{NonNull, };
use std::sync::{Arc, atomic, };
use std::sync::atomic::{AtomicUsize, Ordering, AtomicBool, };

use log::{error, };

use nd;

use hsa_core::kernel::{KernelInstance, };

use hsa_rt::agent::Agent;
use hsa_rt::executable::FrozenExecutable;
use hsa_rt::mem::region::{RegionBox, Region, };
use hsa_rt::queue::{DispatchPacket, IQueue, QueueKind, };

pub use hsa_rt::queue::{FenceScope, QueueError, };
pub use hsa_rt::queue::KernelMultiQueue as DeviceMultiQueue;
pub use hsa_rt::queue::KernelSingleQueue as DeviceSingleQueue;

use crate::lrt_core::{Device, Accelerator, AcceleratorId, };
use crate::lrt_core::codegen as core_codegen;
use crate::lrt_core::codegen::PKernelDesc;
use crate::lrt_core::context::{ModuleContextData, PlatformModuleData, };

use crate::HsaAmdGpuAccel;
use crate::codegen::{Codegenner, KernelDesc, CodegenDesc};
use crate::signal::{DeviceConsumable, DeviceSignal, HostConsumable, SignalHandle};
use hsa_rt::ext::amd::MemoryPoolPtr;
use hsa_rt::signal::SignalRef;

#[derive(Clone, Copy)]
pub struct Function<F>
  where F: ?Sized,
{
  instance: KernelInstance,
  context_data: ModuleContextData,
  /// Keep this at the end:
  f: F,
}

impl<F> Function<F>
  where F: ?Sized,
{
  pub fn new<Args, Ret>(f: F) -> Self
    where F: Fn<Args, Output = Ret> + Sized,
  {
    Function {
      instance: KernelInstance::get(&f),
      context_data: ModuleContextData::get(&f),
      f,
    }
  }
  pub fn desc<Args, Ret>(&self) -> PKernelDesc<Codegenner>
    where F: Fn<Args, Output = Ret>,
  {
    core_codegen::KernelDesc {
      instance: self.instance.clone(),
      platform_desc: KernelDesc::new(&self.f),
    }
  }
}
impl<F1, F2> CoerceUnsized<Function<F2>> for Function<F1>
  where F1: CoerceUnsized<F2> + ?Sized,
        F2: ?Sized,
{ }

#[derive(Clone, Copy)]
pub struct FuncModule<F, MD = Arc<HsaModuleData>>
  where F: ?Sized,
        MD: Deref<Target = HsaModuleData>,
{
  /// XXX this isn't actually used for checking anything
  /// This is because only the device queue is used for dispatching;
  /// the device object is never needed.
  expected_accel: AcceleratorId,

  module_data: MD,

  pub dynamic_group_size: u32,
  pub dynamic_private_size: u32,

  /// Keep these private; improper usage can result in the use of
  /// stale data/allocations
  begin_fence: FenceScope,
  end_fence: FenceScope,

  /// Keep this at the end:
  f: Function<F>,
}
impl<F> FuncModule<F>
  where F: ?Sized,
{
  pub fn new<Args, Ret>(accel: &Arc<HsaAmdGpuAccel>, f: F)
    -> Result<Self, Box<dyn Error>>
    where F: Fn<Args, Output = Ret> + Sized,
  {
    let f = Function::new(f);
    let context_data = f.context_data
      .get_cache_data(accel.ctx());
    let module_data = context_data
      .compile(accel, f.desc(),
               &accel.codegen())?;

    Ok(FuncModule {
      expected_accel: accel.id(),
      module_data,
      dynamic_group_size: 0,
      dynamic_private_size: 0,
      begin_fence: FenceScope::System,
      end_fence: FenceScope::System,
      f,
    })
  }

  fn set_acquire_fence(&mut self, scope: FenceScope) {
    self.begin_fence = scope;
  }
  pub fn acquire_fence(&self) -> FenceScope { self.begin_fence }
  /// Have the GPU CP execute a system level memory fence *before*
  /// the dispatch waves are launched.
  /// This is safe, if not as fast, option, and is the default.
  pub fn system_acquire_fence(&mut self) {
    self.set_acquire_fence(FenceScope::System);
  }
  /// Have the GPU CP execute a device level memory fence *before*
  /// the dispatch waves are launched. This will not synchronize
  /// host memory writes to GPU mapped memory with the GPU, but in
  /// contrast with no fence, writes by other waves will be visible. You are
  /// responsible for coding around this fact.
  pub unsafe fn device_acquire_fence(&mut self) {
    self.set_acquire_fence(FenceScope::Agent);
  }
  /// The GPU CP executes no fence before launching waves. This is the fastest option,
  /// but is also the most dangerous!
  /// This will not synchronize any writes made prior to launch.
  pub unsafe fn no_acquire_fence(&mut self) {
    self.set_acquire_fence(FenceScope::None);
  }


  fn set_release_fence(&mut self, scope: FenceScope) {
    self.begin_fence = scope;
  }
  pub fn release_fence(&self) -> FenceScope { self.end_fence }
  /// Execute a system scope fence *before* the dispatch
  /// completion signal is signaled (or rather decremented).
  /// This ensures writes to host visible memory have made it all the
  /// way to host visible memory.
  /// This is, again, the safest option, if not the fastest. Use this
  /// if unsure.
  pub fn system_release_fence(&mut self) {
    self.set_release_fence(FenceScope::System);
  }
  /// Device level memory fence only before signal completion.
  /// This is unsafe! Your code must manually ensure that writes are visible.
  /// If unsure, use a system scope fence.
  pub unsafe fn device_release_fence(&mut self) {
    self.set_release_fence(FenceScope::Agent);
  }
  /// No fence prior to signal completion.
  /// This is unsafe! Your code must manually ensure that writes are visible.
  /// If unsure, use a system scope fence.
  pub unsafe fn no_release_fence(&mut self) {
    self.set_release_fence(FenceScope::None);
  }
}
impl<F1, F2, MD> CoerceUnsized<FuncModule<F2, MD>> for FuncModule<F1, MD>
  where F1: CoerceUnsized<F2> + ?Sized,
        F2: ?Sized,
        MD: Deref<Target = HsaModuleData>,
{ }

#[derive(Debug)]
pub enum CallError {
  Queue(QueueError),
  Compile(Box<dyn Error>),
  Oom,
  CompletionSignal(Box<dyn Error>),
}
impl fmt::Display for CallError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self) // TODO
  }
}
impl From<QueueError> for CallError {
  fn from(v: QueueError) -> Self {
    CallError::Queue(v)
  }
}

/// A trait so downstream crates don't need ndarray to use generic dims.
pub trait LaunchDims: nd::IntoDimension + Clone {
  fn default_unit() -> Self;
}
// HSA/AMDGPUs only support up to 3d, so that's all we're going to support here.
impl LaunchDims for (usize, ) {
  fn default_unit() -> Self { (1, ) }
}
impl LaunchDims for (usize, usize, ) {
  fn default_unit() -> Self { (1, 1, ) }
}
impl LaunchDims for (usize, usize, usize, ) {
  fn default_unit() -> Self { (1, 1, 1, ) }
}

#[derive(Clone)]
pub struct Invoc<F, Dim, S = DeviceSignal, MD = Arc<HsaModuleData>>
  where F: ?Sized,
        MD: Deref<Target = HsaModuleData>,
        Dim: LaunchDims,
        S: DeviceConsumable,
{
  pub workgroup_dim: Dim,
  pub grid_dim: Dim,

  deps: Vec<S>,

  /// Keep this at the end:
  fmod: FuncModule<F, MD>,
}

impl<F, Dim, S> Invoc<F, Dim, S, Arc<HsaModuleData>>
  where F: ?Sized,
        Dim: LaunchDims,
        S: DeviceConsumable,
{
  pub fn new<Args>(accel: &Arc<HsaAmdGpuAccel>, f: F)
    -> Result<Self, Box<dyn Error>>
    where F: Fn<Args, Output=()> + Sized,
          Args: Sized,
  {
    let fmod = FuncModule::new(accel, f)?;
    Ok(Invoc {
      workgroup_dim: Dim::default_unit(),
      grid_dim: Dim::default_unit(),
      deps: Vec::new(),
      fmod,
    })
  }
  pub fn new_dims<Args>(accel: &Arc<HsaAmdGpuAccel>,
                        f: F, wg: Dim, grid: Dim)
    -> Result<Self, Box<dyn Error>>
    where F: Fn<Args, Output=()> + Sized,
          Args: Sized,
  {
    let fmod = FuncModule::new(accel, f)?;
    Ok(Invoc {
      workgroup_dim: wg,
      grid_dim: grid,
      deps: Vec::new(),
      fmod,
    })
  }
}

impl<F, Dim, S, MD> Invoc<F, Dim, S, MD>
  where F: ?Sized,
        MD: Deref<Target = HsaModuleData>,
        Dim: LaunchDims,
        S: DeviceConsumable,
{
  pub fn from<A>(fmod: FuncModule<F, MD>,
                 wg: Dim, grid: Dim) -> Self
    where F: Fn<A, Output = ()> + Sized,
          A: Sized,
  {
    Invoc {
      workgroup_dim: wg,
      grid_dim: grid,
      deps: Vec::new(),
      fmod,
    }
  }


  pub fn workgroup_dims(&mut self, dim: Dim) -> &mut Self
    where Dim: LaunchDims + Copy,
  {
    self.workgroup_dim = dim;
    self
  }
  pub fn grid_dims(&mut self, dim: Dim) -> &mut Self
    where Dim: LaunchDims + Copy,
  {
    self.grid_dim = dim;
    self
  }

  /// TODO: use `DepSignal` here to return dep guarded resources
  /// so they can be used as kernel args. Currently no way
  /// to ensure that the resources are used *only* for kernel
  /// arguments for *this* invocation.
  pub fn add_dep(&mut self, signal: S) {
    self.deps.push(signal);
  }
  pub fn deps(&self) -> &[S] { &self.deps }
  pub fn replace_deps(&mut self, deps: Vec<S>) {
    assert_eq!(self.deps.len(), 0, "must have no existing deps");
    self.deps = deps;
  }

  /// `completion` must already have the correct value set, eg set to `1`.
  /// This function does no argument checking to ensure, eg, you're not passing
  /// anything by mutable reference or something with drop code by value.
  pub unsafe fn unchecked_call_async<P, A, Q, T, K, CS>(&mut self,
                                                        args: A,
                                                        queue: Q,
                                                        completion: CS,
                                                        args_pool: P)
    -> Result<InvocCompletion<P, A, Q, S, CS>, CallError>
    where F: Fn<A, Output = ()>,
          A: Copy + Sized + Unpin,
          P: Deref<Target = ArgsPool>,
          Q: Deref<Target = T>,
          T: IQueue<K>,
          K: QueueKind,
          CS: SignalHandle,
  {
    let mut args = Some(args);
    let mut queue = Some(queue);
    let mut completion = Some(completion);
    self.try_unchecked_call_async(&mut args, &mut queue, &mut completion,
                                  args_pool)
  }
  /// Kernarg allocation can fail, so this function allows you re-call without having
  /// to also recreate the arguments (since we move).
  /// If allocation fails, the provided arguments will still be `Some()`.
  pub unsafe fn try_unchecked_call_async<P, A, Q, T, K, CS>(&mut self,
                                                            args: &mut Option<A>,
                                                            queue: &mut Option<Q>,
                                                            completion: &mut Option<CS>,
                                                            args_pool: P)
    -> Result<InvocCompletion<P, A, Q, S, CS>, CallError>
    where F: Fn<A, Output = ()>,
          A: Copy + Sized + Unpin,
          P: Deref<Target = ArgsPool>,
          Q: Deref<Target = T>,
          T: IQueue<K>,
          K: QueueKind,
          CS: SignalHandle,
  {
    let kernel = &*self.fmod.module_data;

    assert_eq!(kernel.desc.kernarg_segment_size, size_of::<A>());

    let kernargs = args_pool.alloc::<A>();
    if kernargs.is_none() {
      return Err(CallError::Oom);
    }

    let Invoc {
      workgroup_dim,
      grid_dim,
      deps,
      ref fmod,
    } = self;

    // enqueue the dep barriers. this is done after kernarg allocation
    // so that this step isn't repeated if we're called again as a result
    // of kernarg alloc failure.
    {
      let mut deps = deps.iter().map(|dep| {
        dep.mark_consumed();
        dep.signal_ref()
      });
      let q = queue.as_ref().expect("provide a queue pls");
      while deps.len() > 0 {
        // no completion signal, which will cause the barrier packet to
        // be ordered (that is, the CP won't process later packets till
        // the barrier is complete).
        q.try_enqueue_barrier_and(&mut deps, None)?;
      }
    }

    let mut kernargs = kernargs.unwrap();
    ::std::ptr::write(kernargs.as_ptr(),
                      args.take().unwrap());
    // Ensure the writes to the kernel args are all the way to memory:
    atomic::fence(Ordering::SeqCst);

    {
      let q = queue.as_ref().expect("provide a completion signal pls");
      let c = completion.as_ref()
        .map(|c| c.signal_ref() );

      let group = kernel.desc.group_segment_size as u32;
      let private = kernel.desc.private_segment_size as u32;
      let dispatch = DispatchPacket {
        workgroup_size: workgroup_dim.clone(),
        grid_size: grid_dim.clone(),
        group_segment_size: fmod.dynamic_group_size + group,
        private_segment_size: fmod.dynamic_private_size + private,
        scaquire_scope: fmod.begin_fence.clone(),
        screlease_scope: fmod.end_fence.clone(),
        ordered: c.is_none(),
        kernel_object: kernel.kernel_object.get(),
        kernel_args: kernargs.as_mut(),
        completion_signal: c,
      };

      q
        .try_enqueue_kernel_dispatch(dispatch)?;
    }

    let inner = InvocCompletionInner {
      storage: args_pool,
      queue: queue.take().unwrap(),
      deps: deps.drain(..).collect(),
      signal: completion.take().unwrap(),
      waited: AtomicBool::new(false),
      args: kernargs,
      _lt: PhantomData,
    };
    Ok(InvocCompletion(Some(inner)))
  }
}
impl<F, WGDim, S, MD> Deref for Invoc<F, WGDim, S, MD>
  where F: ?Sized,
        MD: Deref<Target = HsaModuleData>,
        WGDim: LaunchDims,
        S: DeviceConsumable,
{
  type Target = FuncModule<F, MD>;
  fn deref(&self) -> &FuncModule<F, MD> { &self.fmod }
}
impl<F, WGDim, S, MD> DerefMut for Invoc<F, WGDim, S, MD>
  where F: ?Sized,
        MD: Deref<Target = HsaModuleData>,
        WGDim: LaunchDims,
        S: DeviceConsumable,
{
  fn deref_mut(&mut self) -> &mut FuncModule<F, MD> { &mut self.fmod }
}

/// Use this to invoc in a loop without allocating every iteration
/// AND without running amuck of Rust's borrow checker.
/// XXX Use the AMD vendor extensions to get the cacheline size, instead of
/// hardcoding to 1 << 6 here.
pub struct ArgsPool {
  /// Keep the region handle alive
  _kernargs_region: Region,
  /// Only `None` after dropping
  base: Option<RegionBox<[u8]>>,
  allocated: AtomicUsize,
}
impl ArgsPool {
  /// Create storage for `n` function calls for use on the provided accelerator.
  pub fn new<Args>(accel: &Arc<HsaAmdGpuAccel>, count: usize)
    -> Result<Self, Box<dyn Error>>
    where Args: Sized,
  {
    use std::cmp::max;

    let kernargs_region = accel.kernargs_region().clone();
    let layout = Layout::new::<Args>();
    let pool_alignment = kernargs_region.runtime_alloc_alignment()?;
    if pool_alignment < layout.align() {
      return Err("pool allocation alignment is < less than arg alignment".into())
    }
    let (layout, _) = layout.repeat(count)?;
    let layout = layout.align_to(pool_alignment)?;
    let bytes = layout.size();
    let pool_min_alloc = kernargs_region.runtime_alloc_granule()?;
    let pool_max_alloc = kernargs_region.alloc_max_size()?;
    // bump the size to the minimum allocation size:
    let bytes = max(pool_min_alloc, bytes);

    // ensure we don't allocate more than allowed:
    if bytes > pool_max_alloc {
      return Err("maximum pool allocation size exceeded".into());
    }

    let base = unsafe {
      RegionBox::uninitialized_slice(&kernargs_region, bytes)?
    };

    Ok(ArgsPool {
      allocated: AtomicUsize::new(base.as_ptr() as usize),
      base: Some(base),
      _kernargs_region: kernargs_region,
    })
  }

  pub fn new_arena(accel: &Arc<HsaAmdGpuAccel>, bytes: usize)
    -> Result<Self, Box<dyn Error>>
  {
    use std::cmp::max;

    let kernargs_region = accel.kernargs_region().clone();
    let pool_min_alloc = kernargs_region.runtime_alloc_granule()?;
    let pool_max_alloc = kernargs_region.alloc_max_size()?;
    // bump the size to the minimum allocation size:
    let bytes = max(pool_min_alloc, bytes);
    // ensure we don't allocate more than allowed:
    if bytes > pool_max_alloc {
      return Err("maximum pool allocation size exceeded".into());
    }

    let base = unsafe {
      RegionBox::uninitialized_slice(&kernargs_region, bytes)?
    };

    Ok(ArgsPool {
      allocated: AtomicUsize::new(base.as_ptr() as usize),
      base: Some(base),
      _kernargs_region: kernargs_region,
    })
  }

  fn base(&self) -> &RegionBox<[u8]> {
    self.base.as_ref()
      .expect("dropped?")
  }
  pub fn size(&self) -> usize { self.base().len() }

  fn start_byte(&self) -> usize { self.base().as_ptr() as usize }
  fn end_byte(&self) -> usize {
    self.start_byte() + self.size()
  }

  /// Allocate a single `Args` block. Returns `None` when out of space.
  /// `args` must be Some. If allocation is successful, the returned
  /// pointer will be uninitialized.
  pub unsafe fn alloc<Args>(&self) -> Option<NonNull<Args>>
    where Args: Sized,
  {
    fn alignment_padding(size: usize, align: usize) -> usize {
      (align - (size - 1) % align) - 1
    }

    let layout = Layout::new::<Args>()
      // force alignment to at least the cacheline size to avoid false
      // sharing.
      .align_to(64) // XXX hardcoded. could use this fact to avoid the loop below
      // TODO return an error here so users don't think it's OOM.
      .ok()?;

    let mut allocated_start = self.allocated.load(Ordering::Acquire);

    loop {
      let padding = alignment_padding(allocated_start,
                                      layout.align());

      let alloc_size = layout.size() + padding;

      if allocated_start + alloc_size > self.end_byte() {
        // no more space available, bail.
        return None;
      }

      match self.allocated.compare_exchange_weak(allocated_start,
                                                 allocated_start + alloc_size,
                                                 Ordering::SeqCst,
                                                 Ordering::Relaxed) {
        Ok(_) => {
          // ensure the start of the allocation is actually aligned
          allocated_start += padding;
        },
        Err(new_allocated_start) => {
          allocated_start = new_allocated_start;
          continue;
        }
      }

      let ptr: *mut Args = transmute(allocated_start);
      return Some(NonNull::new_unchecked(ptr));
    }
  }

  /// Reset the allocation ptr to the base. The mutable requirement ensures
  /// no device calls are in flight.
  pub fn wash(&mut self) {
    let base = self.base().as_ptr() as usize;
    *self.allocated.get_mut() = base;
  }
}

impl Drop for ArgsPool {
  fn drop(&mut self) {
    // we don't need to dtor any arguments, as `InvocCompletion` handles that
    // and will ensure we stay alive.
    let base = self.base.take().unwrap();

    if let Err(e) = base.checked_drop() {
      error!("failed to deallocate kernel arg storage: {:?}", e);
    }
  }
}

/// Note, in contrast to above, this must not require Sized Args.
/// We really only require Args impl drop (which is given in Rust)
/// and don't want to constrain downstream if type erasure is desired.
#[must_use]
pub struct InvocCompletion<P, A, Q, DS, S>(Option<InvocCompletionInner<P, A, Q, DS, S>>)
  where P: Deref<Target = ArgsPool>,
        DS: SignalHandle,
        S: SignalHandle,
        A: ?Sized;

pub struct InvocCompletionInner<P, A, Q, DS, S>
  where P: Deref<Target = ArgsPool>,
        DS: SignalHandle,
        S: SignalHandle,
        A: ?Sized,
{
  storage: P,
  queue: Q,
  deps: Vec<DS>,
  signal: S,
  /// a flag which we will set when we get used as a dep in
  /// another invoc. Only used when our completion signal
  /// is a device signal.
  waited: AtomicBool,
  /// Keep this last so we can be unsized into `dyn Drop`.
  args: NonNull<A>,
  _lt: PhantomData<A>,
}
impl<P, A, Q, DS, S> InvocCompletion<P, A, Q, DS, S>
  where P: Deref<Target = ArgsPool>,
        DS: SignalHandle,
        S: SignalHandle,
        A: ?Sized,
{
  fn inner(&self) -> &InvocCompletionInner<P, A, Q, DS, S> {
    self.0.as_ref()
      .expect("dropped?")
  }
}
impl<P, A, Q, DS, S> InvocCompletion<P, A, Q, DS, S>
  where P: Deref<Target = ArgsPool>,
        DS: SignalHandle,
        S: HostConsumable,
        A: ?Sized,
{
  pub fn wait(mut self, active: bool) {
    let inner = self.0.take().unwrap();
    inner.signal.wait_for_zero(active)
      // XXX should we return a result here too?
      .expect("device signaled error via signal");

    // Run `self.args`' drop code
    unsafe { std::intrinsics::drop_in_place(inner.args.as_ptr()); }
  }
}
impl<P, A, Q, DS, S> InvocCompletion<P, A, Q, DS, S>
  where P: Deref<Target = ArgsPool>,
        DS: DeviceConsumable,
        S: DeviceConsumable + Into<DS>,
        A: ?Sized,
{
  /// Execute a device side device -> host copy after this dispatch
  /// completes. `completion` becomes the new invoc completion signal.
  pub unsafe fn async_copy_to_host<S2, T>(self,
                                          device: &HsaAmdGpuAccel,
                                          accel: MemoryPoolPtr<T>,
                                          host: MemoryPoolPtr<T>,
                                          completion: S2)
    -> Result<InvocCompletion<P, A, Q, DS, S2>, Box<dyn Error>>
    where S2: SignalHandle,
          T: ?Sized,
  {
    self.async_copies_to_host(device, Some((accel, host)).into_iter(),
                              completion)
  }
  /// Execute a device side device -> host copy after this dispatch
  /// completes. `completion` becomes the new invoc completion signal.
  pub unsafe fn async_copies_to_host<S2, T>(mut self,
                                            device: &HsaAmdGpuAccel,
                                            ptrs: impl Iterator<Item = (MemoryPoolPtr<T>,
                                                                        MemoryPoolPtr<T>)>,
                                            completion: S2)
    -> Result<InvocCompletion<P, A, Q, DS, S2>, Box<dyn Error>>
    where S2: SignalHandle,
          T: ?Sized,
  {
    let InvocCompletionInner {
      storage,
      queue,
      mut deps,
      signal,
      args,
      ..
    } = self.0.take().unwrap();

    for (accel, host) in ptrs {
      device.unchecked_async_copy_from(accel.into_bytes(),
                                       host.into_bytes(),
                                       &[&signal],
                                       &completion)?;
    }

    deps.push(signal.into());

    let inner = InvocCompletionInner {
      storage,
      queue,
      deps,
      signal: completion,
      waited: AtomicBool::new(false),
      args,
      _lt: PhantomData,
    };
    Ok(InvocCompletion(Some(inner)))
  }
}
// impl the signal traits so that invoc completions can be reused for multiple
// kernel dispatches.
impl<P, A, Q, DS, S> SignalHandle for InvocCompletion<P, A, Q, DS, S>
  where P: Deref<Target = ArgsPool>,
        DS: SignalHandle,
        S: SignalHandle,
        A: ?Sized,
{
  fn signal_ref(&self) -> &SignalRef {
    self.inner()
      .signal
      .signal_ref()
  }

  unsafe fn mark_consumed(&self) {
    self.inner()
      .waited
      .store(true, Ordering::Release)
  }

  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    self.inner()
      .signal
      .as_host_consumable()
  }
}
impl<P, A, Q, DS, S> DeviceConsumable for InvocCompletion<P, A, Q, DS, S>
  where P: Deref<Target = ArgsPool>,
        DS: SignalHandle,
        S: DeviceConsumable,
        A: ?Sized,
{
  fn usable_on_device(&self, id: AcceleratorId) -> bool {
    self.inner()
      .signal
      .usable_on_device(id)
  }
}
impl<P, A, Q, DS, S> HostConsumable for InvocCompletion<P, A, Q, DS, S>
  where P: Deref<Target = ArgsPool>,
        DS: SignalHandle,
        S: HostConsumable,
        A: ?Sized,
{
}

impl<P, A, Q, DS, S> Drop for InvocCompletion<P, A, Q, DS, S>
  where P: Deref<Target = ArgsPool>,
        DS: SignalHandle,
        S: SignalHandle,
        A: ?Sized,
{
  fn drop(&mut self) {
    if let Some(mut inner) = self.0.take() {
      let waited = inner.waited.get_mut();
      if !*waited {
        if let Some(host) = inner.signal.as_host_consumable() {
          if let Err(code) = host.wait_for_zero(false) {
            error!("got negative signal from dispatch: {}", code);
          }

          *waited = true;
        }
      }

      if *waited {
        // Run `self.args`' drop code
        unsafe { std::intrinsics::drop_in_place(inner.args.as_ptr()); }
        return;
      }

      if !*waited && !::std::thread::panicking() {
        // fail loudly
        panic!("InvocCompletion w/ device completion signal \
                uncomsumed (ie not used as a dep in another dispatch)!");
      } else {
        log::warn!("host panic has probably caused some leaked data! \
                    Program execution is undefined now!");
      }
    }
  }
}
impl<P, A1, A2, Q, DS, S> CoerceUnsized<InvocCompletionInner<P, A2, Q, DS, S>> for InvocCompletionInner<P, A1, Q, DS, S>
  where P: Deref<Target = ArgsPool>,
        DS: SignalHandle,
        S: SignalHandle,
        A1: Unsize<A2> + ?Sized,
        A2: ?Sized,
{ }

#[derive(Debug, Eq, PartialEq, Hash)]
pub struct HsaModuleData {
  /// We need to keep the agent handle alive so the exe handle
  /// is also kept alive.
  pub(crate) agent: Agent,
  pub(crate) exe: FrozenExecutable,
  pub(crate) kernel_object: NonZeroU64,
  pub(crate) desc: CodegenDesc,
}
impl PlatformModuleData for HsaModuleData {
  fn eq(&self, rhs: &dyn PlatformModuleData) -> bool {
    let rhs: Option<&Self> = Self::downcast_ref(rhs);
    if let Some(rhs) = rhs {
      self == rhs
    } else {
      false
    }
  }

  fn downcast_ref(this: &dyn PlatformModuleData) -> Option<&Self>
    where Self: Sized,
  {
    use std::any::TypeId;
    use std::raw::TraitObject;

    if this.type_id() != TypeId::of::<Self>() {
      return None;
    }

    // We have to do this manually.
    let this: TraitObject = unsafe { transmute(this) };
    let this = this.data as *mut Self;
    Some(unsafe { &*this })
  }
  fn downcast_arc(this: &Arc<dyn PlatformModuleData>) -> Option<Arc<Self>>
    where Self: Sized,
  {
    use std::any::TypeId;
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
