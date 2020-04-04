
use std::alloc::Layout;
use std::marker::{PhantomData, Unsize, };
use std::mem::{transmute, size_of, };
use std::num::NonZeroU64;
use std::ops::{CoerceUnsized, Deref, };
use std::pin::Pin;
use std::ptr::{self, Unique, };
use std::sync::{Arc, atomic, };
use std::sync::atomic::{AtomicUsize, Ordering, AtomicBool, };

use alloc_wg::boxed::Box;

use log::{error, };

use geobacter_core::kernel::{KernelInstance, OptionalFn, };
use gcore::ref_::*;

use hsa_rt::agent::Agent;
use hsa_rt::executable::FrozenExecutable;
use hsa_rt::queue::{DispatchPacket, RingQueue, };
use hsa_rt::signal::SignalRef;

use smallvec::SmallVec;

pub use hsa_rt::queue::{FenceScope, QueueError, };
pub use hsa_rt::queue::KernelMultiQueue as DeviceMultiQueue;
pub use hsa_rt::queue::KernelSingleQueue as DeviceSingleQueue;

use crate::grt_core::{Device, AcceleratorId, };
use crate::grt_core::codegen as core_codegen;
use crate::grt_core::codegen::PKernelDesc;
use crate::grt_core::context::{ModuleContextData, PlatformModuleData, ModuleData, };

use crate::{HsaAmdGpuAccel, Error};
use crate::codegen::{Codegenner, KernelDesc, CodegenDesc};
use crate::signal::{DeviceConsumable, HostConsumable, SignalHandle,
                    SignaledDeref, Value};

use self::args_pool::ArgsPoolAlloc;

pub use self::args::*;
pub use self::args_pool::ArgsPool;
pub use self::grid::*;
pub use self::deps::Deps;

pub mod args;
pub mod args_pool;
pub mod grid;
pub mod deps;

#[cfg(test)]
mod test;

// TODO refactor common stuff into runtime-core

/// This is the kernel that the GPU directly runs. This function should probably call
/// Self::kernel after doing some light bookkeeping.
fn launch_kernel<A>(this: &LaunchArgs<A>)
  where A: Kernel + Sized,
{
  let params = VectorParams::new_internal(&this.grid, &A::WORKGROUP);
  if let Some(params) = params {
    this.args.kernel(params)
  }
}

type InvocArgs<'a, A> = (A, LaunchArgs<'a, A>, &'a LaunchArgs<'a, A>, );

pub struct FuncModule<A>
  where A: Kernel,
{
  device: Arc<HsaAmdGpuAccel>,
  context_data: Arc<ModuleData>,
  module_data: Option<Arc<HsaModuleData>>,

  pub dynamic_group_size: u32,
  pub dynamic_private_size: u32,

  /// Keep these private; improper usage can result in the use of
  /// stale data/allocations
  begin_fence: FenceScope,
  end_fence: FenceScope,

  instance: KernelInstance,
  desc: KernelDesc,
  spec_params: core_codegen::SpecParamsDesc,

  /// To ensure we are only called with this argument type.
  _arg: PhantomData<*const A>,
}
impl<A> FuncModule<A>
  where A: Kernel,
{
  pub fn new(accel: &Arc<HsaAmdGpuAccel>) -> Self {
    let f = launch_kernel::<A>;

    FuncModule {
      device: accel.clone(),
      module_data: None,
      dynamic_group_size: 0,
      dynamic_private_size: 0,
      begin_fence: FenceScope::System,
      end_fence: FenceScope::System,

      instance: f.kernel_instance().unwrap(),
      context_data: ModuleContextData::get(&f)
        .get_cache_data(accel.ctx()),
      desc: KernelDesc { },
      spec_params: Default::default(),

      _arg: PhantomData,
    }
  }
  fn desc(&self) -> PKernelDesc<Codegenner> {
    core_codegen::KernelDesc {
      instance: self.instance.clone(),
      spec_params: self.spec_params.clone(),
      platform_desc: self.desc.clone(),
    }
  }

  pub fn group_size(&mut self) -> Result<u32, Error> {
    let module_data = self.compile_internal()?;
    Ok(module_data.desc.group_segment_size + self.dynamic_group_size)
  }
  pub fn private_size(&mut self) -> Result<u32, Error> {
    let module_data = self.compile_internal()?;
    Ok(module_data.desc.private_segment_size + self.dynamic_private_size)
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

  fn compile_internal(&mut self)
    -> Result<&HsaModuleData, Error>
  {
    if self.module_data.is_none() {
      let module_data = self.context_data
        .compile(&self.device, self.desc(),
                 &self.device.codegen(),
                 cfg!(test))?;
      self.module_data = Some(module_data);
    }
    Ok(self.module_data.as_ref().unwrap())
  }
  pub fn compile(&mut self) -> Result<(), Error> {
    self.compile_internal()?;
    Ok(())
  }
  pub fn compile_async(&self) {
    use rustc_data_structures::rayon::*;

    if self.module_data.is_none() {
      let context_data = self.context_data.clone();
      let device = self.device.clone();
      let desc = self.desc();
      spawn(move || {
        // ignore errors here; if an error does happen,
        // we'll compile again to get the actual error from codegen.
        let _ = context_data.compile(&device, desc, device.codegen(),
                                     cfg!(test));
      });
    }
  }

  /// Undefine all params. If this function was already compiled, it will be compiled
  /// again.
  pub fn clear_params(&mut self) {
    self.module_data.take();
    self.spec_params.clear();
  }
  /// Undefine a specialization entry. If the key (`f`) has no entry, this does nothing.
  ///
  /// If this function was already compiled, it will be compiled again.
  pub fn undefine_param<F, R>(&mut self, f: F)
    where F: Fn() -> R,
  {
    self.module_data.take();
    self.spec_params.undefine(f)
  }
  /// (Re-)Define a specialization param, keyed by `f`. Specialization params are like
  /// constants, but allow one to redefine them at will at runtime. Thus one can create multiple
  /// kernels from just a single function, which are specialized to the params defined here
  ///
  /// Call `geobacter_core::params::get_spec_param` in the kernel with the same function
  /// to get the value provided here.
  ///
  /// If this function was already compiled, it will be compiled again.
  ///
  /// Note: If you're going to replace every param, it's better to call `clear_params()`
  /// first.
  pub fn define_param<F, R>(&mut self, f: F, value: &R)
    where F: Fn() -> R,
          R: Copy + Unpin + 'static,
  {
    self.module_data.take();
    self.spec_params.define(f, value)
  }

  /// Attach a kernel args pool, in preparation for dispatching.
  pub fn invoc<P>(&self, pool: P) -> Invoc<A, P, Self>
    where P: Deref<Target = ArgsPool> + Clone,
  {
    Invoc {
      pool,
      f: self.clone(),
      _p: PhantomData,
    }
  }
  pub fn into_invoc<P>(self, pool: P) -> Invoc<A, P, Self>
    where P: Deref<Target = ArgsPool> + Clone,
  {
    Invoc {
      pool,
      f: self,
      _p: PhantomData,
    }
  }
  pub fn invoc_mut<P>(&mut self, pool: P) -> Invoc<A, P, &mut Self>
    where P: Deref<Target = ArgsPool> + Clone,
  {
    Invoc {
      pool,
      f: self,
      _p: PhantomData,
    }
  }
}

impl<A> Clone for FuncModule<A>
  where A: Kernel,
{
  fn clone(&self) -> Self {
    FuncModule {
      device: self.device.clone(),
      module_data: self.module_data.clone(),
      dynamic_group_size: self.dynamic_group_size,
      dynamic_private_size: self.dynamic_private_size,
      begin_fence: self.begin_fence,
      end_fence: self.end_fence,

      instance: self.instance,
      context_data: self.context_data.clone(),
      desc: self.desc.clone(),
      spec_params: self.spec_params.clone(),

      _arg: PhantomData,
    }
  }
}

/// Just a simple helper for owned and mutable types.
#[doc(hidden)]
pub trait FuncModuleMut<A>
  where A: Kernel,
{
  fn fm_mut(&mut self) -> &mut FuncModule<A>;
}
impl<A> FuncModuleMut<A> for FuncModule<A>
  where A: Kernel,
{
  fn fm_mut(&mut self) -> &mut FuncModule<A> { self }
}
impl<'a, A> FuncModuleMut<A> for &'a mut FuncModule<A>
  where A: Kernel,
{
  fn fm_mut(&mut self) -> &mut FuncModule<A> { self }
}

pub struct Invoc<A, P, FM>
  where A: Kernel,
        P: Deref<Target = ArgsPool> + Clone,
        FM: FuncModuleMut<A>,
{
  pool: P,
  f: FM,
  _p: PhantomData<*const A>,
}

impl<A, P, FM> Invoc<A, P, FM>
  where A: Kernel,
        P: Deref<Target = ArgsPool> + Clone,
        FM: FuncModuleMut<A>,
{
  /// The associated completion signal should already have the correct value set, eg set to `1`.
  pub unsafe fn unchecked_call_async(&mut self, grid: &A::Grid, args: A)
    -> Result<InvocCompletion<P, A, A::CompletionSignal>, Error>
    where A::Queue: RingQueue,
  {
    self.try_unchecked_call_async(grid, args)
      .map_err(|(err, _)| err )
  }
  /// Kernarg allocation can fail, so this function allows you re-call without having
  /// to also recreate the arguments (since we move them into a pinned box internally).
  pub unsafe fn try_unchecked_call_async(&mut self, grid: &A::Grid, args: A)
    -> Result<InvocCompletion<P, A, A::CompletionSignal>, (Error, A)>
    where A::Queue: RingQueue,
  {
    let mut args = Some(args);
    match self._try_unchecked_call_async(grid, &mut args) {
      Ok(v) => Ok(v),
      Err(err) => Err((err, args.take().unwrap())),
    }
  }
  unsafe fn _try_unchecked_call_async(&mut self, grid: &A::Grid, args: &mut Option<A>)
    -> Result<InvocCompletion<P, A, A::CompletionSignal>, Error>
    where A::Queue: RingQueue,
  {
    use num_traits::ops::checked::*;

    let wg_size = A::WORKGROUP.full_launch_grid()?;
    let grid_size = grid.full_launch_grid()?;

    if wg_size.x == 0 || wg_size.y == 0 || wg_size.z == 0
      || grid_size.x == 0 || grid_size.y == 0 || grid_size.z == 0
    {
      return Err(Error::ZeroGridLaunchAxis);
    }

    // Check the device kernel launch limits:
    // Do this first so errors won't waste args pool space.
    {
      let isa = self.f.fm_mut()
        .device
        .isa_info();
      let wg_max_dims = &isa.workgroup_max_dim;
      if wg_size.x > wg_max_dims[0] || wg_size.y > wg_max_dims[1]
        || wg_size.z > wg_max_dims[2]
      {
        return Err(Error::KernelWorkgroupDimTooLargeForDevice);
      }
      let wg_len = wg_size.as_::<u32>()
        .checked_linear_len()?;
      if wg_len > isa.workgroup_max_size {
        return Err(Error::KernelWorkgroupLenTooLargeForDevice);
      }
      let grid_max_dims = &isa.grid_max_dim;
      if grid_size.x > grid_max_dims[0] || grid_size.y > grid_max_dims[1]
        || grid_size.z > grid_max_dims[2]
      {
        return Err(Error::LaunchGridDimTooLargeForDevice);
      }
      let grid_len = grid_size.as_::<u64>()
        .checked_linear_len()?;
      if grid_len > isa.grid_max_size {
        return Err(Error::LaunchGridLenTooLargeForDevice);
      }
    }

    let grid_size: Dim3D<u32> = {
      // Round the grid size up a multiple of the workgroup size.
      // This can overflow, so all ops must be checked.
      let wg_size = wg_size.as_::<u32>();
      let one = Dim3D::from(1u32);
      ((grid_size - one) / wg_size) // can't over/under flow because we've already checked for zero
        .checked_add(&one).ok_or(Error::Overflow)?
        .checked_mul(&wg_size).ok_or(Error::Overflow)?
    };

    let kernel_object = {
      let kernel = self.f
        .fm_mut()
        .compile_internal()?;

      let kargs_size = kernel.desc.kernarg_segment_size as usize;
      assert!(kargs_size == size_of::<(&LaunchArgs<A>, )>(),
              "internal error: unexpected codegen argument size: \
              {} actual vs {} expected", kargs_size,
              size_of::<(&LaunchArgs<A>, )>());

      kernel
        .kernel_object
        .get()
    };

    args.as_ref().expect("provide args");

    let mut kernargs = self.pool.alloc::<InvocArgs<A>>()
      .ok_or(Error::KernelArgsPoolOom)?;
    let kargs = kernargs.cast();
    let launch_args = (&mut kernargs.as_mut().1) as *mut LaunchArgs<A>;
    let launch_args_ref = (&mut kernargs.as_mut().2) as *mut &LaunchArgs<A>;

    // enqueue the dep barriers. this is done after kernarg allocation
    // so that this step isn't repeated if we're called again as a result
    // of kernarg alloc failure.
    {
      let q = args.as_ref().unwrap().queue();
      let mut signals: SmallVec<[SignalRef; 5]> = SmallVec::new();
      {
        let mut f = |sig: &dyn DeviceConsumable| -> Result<(), Error> {
          sig.mark_consumed();
          let sig = ::std::mem::transmute_copy(sig.signal_ref());
          signals.push(sig);
          if signals.len() == 5 {
            {
              let mut deps = signals.iter();
              q.try_enqueue_barrier_and(&mut deps, None)?;
              assert_eq!(deps.len(), 0);
            }
            signals.clear();
          }

          Ok(())
        };
        args.as_ref().unwrap()
          .iter_arg_deps(&mut f)?;
      }
      if signals.len() > 0 {
        let mut deps = signals.iter();
        q.try_enqueue_barrier_and(&mut deps, None)?;
        assert_eq!(deps.len(), 0);
      }
    }

    ptr::write(kargs.as_ptr(), args.take().unwrap());
    ptr::write(launch_args, LaunchArgs {
      args: kargs.as_ref(),
      grid: grid.clone(),
    });
    ptr::write(launch_args_ref, launch_args.as_ref().unwrap());

    let dispatch = DispatchPacket {
      workgroup_size: (wg_size.x, wg_size.y, wg_size.z, ),
      grid_size: (grid_size.x, grid_size.y, grid_size.z, ),
      group_segment_size: self.f.fm_mut().group_size().unwrap(),
      private_segment_size: self.f.fm_mut().private_size().unwrap(),
      scaquire_scope: self.f.fm_mut().begin_fence.clone(),
      screlease_scope: self.f.fm_mut().end_fence.clone(),
      ordered: false,
      kernel_object,
      kernel_args: &*launch_args_ref,
      completion_signal: Some(kargs.as_ref()
        .completion()
        .signal_ref()),
    };

    // Ensure the writes to the kernel args are all the way to memory:
    atomic::fence(Ordering::SeqCst);

    match kargs.as_ref().queue().try_enqueue_kernel_dispatch(dispatch) {
      Ok(()) => { },
      Err(err) => {
        // return args:
        *args = Some(ptr::read(kargs.as_ptr()));
        return Err(err.into());
      }
    }

    let kargs = Box::from_raw_in(kargs.as_ptr(),
                                 ArgsPoolAlloc(self.pool.clone()));

    Ok(InvocCompletion {
      waited: AtomicBool::new(false),
      args: Box::into_pin(kargs),
    })
  }
}

pub type CallError = crate::error::Error;

#[must_use]
pub struct InvocCompletion<P, A, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle + ?Sized,
        A: Completion<CompletionSignal = S> + ?Sized,
{
  /// a flag which we will set when we get used as a dep in
  /// another invoc. Only used when our completion signal
  /// is a device signal.
  waited: AtomicBool,
  /// Keep this last so we can be unsized
  args: Pin<Box<A, args_pool::ArgsPoolAlloc<P>>>,
}
impl<P, A, S> InvocCompletion<P, A, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle + ?Sized,
        A: Completion<CompletionSignal = S> + ?Sized,
{
  pub unsafe fn wait_ref<F, R>(&self, f: F) -> SignaledDeref<R, &S>
    where F: FnOnce(Pin<&A>) -> R,
          S: HostConsumable,
  {
    let args = self.args.as_ref();
    let signal = self.args.completion();
    let r = f(args);

    SignaledDeref::new(r, signal)
  }

  pub fn ret<R>(self, ret: R) -> InvocCompletionReturn<P, A, S, R>
    where A: Sized,
  {
    InvocCompletionReturn {
      ret,
      invoc: self,
    }
  }
}
impl<P, A, S> InvocCompletion<P, A, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: HostConsumable,
        A: Completion<CompletionSignal = S> + ?Sized,
{ }
unsafe impl<P, A> deps::Deps for InvocCompletion<P, A, dyn DeviceConsumable>
  where P: Deref<Target = ArgsPool> + Clone,
        A: Completion<CompletionSignal = dyn DeviceConsumable> + ?Sized,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    f(self.args.completion())
  }
}
impl<P, A, S> Deref for InvocCompletion<P, A, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle,
        A: Completion<CompletionSignal = S> + ?Sized,
{
  type Target = A;
  fn deref(&self) -> &Self::Target {
    &self.args
  }
}
// impl the signal traits so that invoc completions can be reused for multiple
// kernel dispatches.
impl<P, A, S> SignalHandle for InvocCompletion<P, A, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle + ?Sized,
        A: Completion<CompletionSignal = S> + ?Sized,
{
  fn signal_ref(&self) -> &SignalRef {
    self.args
      .completion()
      .signal_ref()
  }

  unsafe fn mark_consumed(&self) {
    self.waited
      .store(true, Ordering::Release)
  }

  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    self.args
      .completion()
      .as_host_consumable()
  }
}
impl<P, A, S> DeviceConsumable for InvocCompletion<P, A, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: DeviceConsumable + ?Sized,
        A: Completion<CompletionSignal = S> + ?Sized,
{
  fn usable_on_device(&self, id: AcceleratorId) -> bool {
    self.args
      .completion()
      .usable_on_device(id)
  }
}
impl<P, A, S> HostConsumable for InvocCompletion<P, A, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: HostConsumable + ?Sized,
        A: Completion<CompletionSignal = S> + ?Sized,
{
  unsafe fn wait_for_zero_relaxed(&self, spin: bool) -> Result<(), Value> {
    self.args
      .completion()
      .wait_for_zero_relaxed(spin)?;
    self.mark_consumed();
    Ok(())
  }
  fn wait_for_zero(&self, spin: bool) -> Result<(), Value> {
    self.args
      .completion()
      .wait_for_zero(spin)?;
    unsafe { self.mark_consumed() };
    Ok(())
  }
}
impl<P, A, S> Drop for InvocCompletion<P, A, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle + ?Sized,
        A: Completion<CompletionSignal = S> + ?Sized,
{
  fn drop(&mut self) {
    let mut waited = self.waited.load(Ordering::Acquire);
    if !waited {
      if let Some(host) = self.args.completion().as_host_consumable() {
        if let Err(code) = host.wait_for_zero(false) {
          error!("got negative signal from dispatch: {}", code);
        }

        waited = true;
      }
    }

    if waited {
      return;
    }

    if !waited && !::std::thread::panicking() {
      // fail loudly; we can't safely run the argument drop code as the device could
      // still be using it!
      panic!("InvocCompletion w/ device completion signal \
              unconsumed (ie not used as a dep in another dispatch)!");
    } else {
      log::warn!("host panic has probably caused some leaked data! \
                  Program execution is undefined now!");
    }
  }
}
impl<P, A1, A2, S> CoerceUnsized<InvocCompletion<P, A2, S>> for InvocCompletion<P, A1, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle,
        A1: Unsize<A2> + Completion<CompletionSignal = S> + ?Sized,
        A2: Completion<CompletionSignal = S> + ?Sized,
{ }

pub struct InvocCompletionReturn<P, A, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle + ?Sized,
        A: Completion<CompletionSignal = S> + ?Sized,
{
  ret: R,
  invoc: InvocCompletion<P, A, S>,
}
impl<P, A, S, R> SignalHandle for InvocCompletionReturn<P, A, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle + ?Sized,
        A: Completion<CompletionSignal = S> + ?Sized,
{
  fn signal_ref(&self) -> &SignalRef {
    self.invoc.signal_ref()
  }

  unsafe fn mark_consumed(&self) {
    self.invoc.mark_consumed()
  }

  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    self.invoc.as_host_consumable()
  }
}
impl<P, A, S, R> DeviceConsumable for InvocCompletionReturn<P, A, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
        S: DeviceConsumable + ?Sized,
        A: Completion<CompletionSignal = S> + ?Sized,
{
  fn usable_on_device(&self, id: AcceleratorId) -> bool {
    self.invoc.usable_on_device(id)
  }
}
impl<P, A, S, R> HostConsumable for InvocCompletionReturn<P, A, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
        S: HostConsumable + ?Sized,
        A: Completion<CompletionSignal = S> + ?Sized,
{
  unsafe fn wait_for_zero_relaxed(&self, spin: bool) -> Result<(), Value> {
    self.invoc.wait_for_zero_relaxed(spin)
  }
  fn wait_for_zero(&self, spin: bool) -> Result<(), Value> {
    self.invoc.wait_for_zero(spin)
  }
}
unsafe impl<P, A, R> deps::Deps for InvocCompletionReturn<P, A, dyn DeviceConsumable, R>
  where P: Deref<Target = ArgsPool> + Clone,
        A: Completion<CompletionSignal = dyn DeviceConsumable> + ?Sized,
        R: deps::Deps,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    self.ret.iter_deps(f)?;
    self.invoc.iter_deps(f)
  }
}
/// We are only *safely* deref-able on the device, where the command process will ensure
/// this invocation is complete.
impl<P, A, S, R> Deref for InvocCompletionReturn<P, A, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
        S: DeviceConsumable + ?Sized,
        A: Completion<CompletionSignal = S> + ?Sized,
{
  type Target = AccelRefRaw<R>;
  fn deref(&self) -> &Self::Target {
    unsafe { transmute(&self.ret) }
  }
}
impl<P, A1, A2, S, R>
  CoerceUnsized<InvocCompletionReturn<P, A2, S, R>> for
    InvocCompletionReturn<P, A1, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle,
        A1: Unsize<A2> + Completion<CompletionSignal = S> + ?Sized,
        A2: Completion<CompletionSignal = S> + ?Sized,
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
}
