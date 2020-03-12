
use std::alloc::Layout;
use std::marker::{PhantomData, Unsize, };
use std::mem::{transmute, size_of, };
use std::num::NonZeroU64;
use std::ops::{CoerceUnsized, Deref, DerefMut, };
use std::pin::Pin;
use std::ptr::{self, Unique, };
use std::sync::{Arc, atomic, };
use std::sync::atomic::{AtomicUsize, Ordering, AtomicBool, };

use alloc_wg::boxed::Box;

use arrayvec::ArrayVec;

use log::{error, };

use num_traits::{ToPrimitive, };

use geobacter_core::kernel::{KernelInstance, OptionalFn, };
use gcore::ref_::*;

use hsa_rt::agent::Agent;
use hsa_rt::executable::FrozenExecutable;
use hsa_rt::queue::{DispatchPacket, IQueue, QueueKind, };
use hsa_rt::signal::SignalRef;

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

pub use self::args_pool::ArgsPool;
pub use self::deps::Deps;

pub mod args;
pub mod args_pool;
pub mod deps;

// TODO refactor common stuff into runtime-core

/// Closures are *explicitly not supported*, so we don't keep the function
/// around as a member or as a type param.
/// This struct used to have an `F` param, but it was realized that this
/// prevented selecting a kernel based on, eg, an enum and then using it
/// as a value (where `F` wasn't specific to the function anymore).
/// Closures have never been supported, so I decided to just drop `F`.
#[derive(Clone)]
pub struct Function {
  instance: KernelInstance,
  context_data: ModuleContextData,
  desc: KernelDesc,
  spec_params: core_codegen::SpecParamsDesc,
}

impl Function {
  pub fn new<F, A, R>(f: F) -> Self
    where F: Fn<A, Output = R> + Sized,
  {
    Function {
      instance: f.kernel_instance().unwrap(),
      context_data: ModuleContextData::get(&f),
      desc: KernelDesc::new(&f),
      spec_params: Default::default(),
    }
  }
  pub fn desc(&self) -> PKernelDesc<Codegenner> {
    core_codegen::KernelDesc {
      instance: self.instance.clone(),
      spec_params: self.spec_params.clone(),
      platform_desc: self.desc.clone(),
    }
  }
}

#[derive(Clone)]
pub struct FuncModule<A> {
  device: Arc<HsaAmdGpuAccel>,
  context_data: Arc<ModuleData>,
  module_data: Option<Arc<HsaModuleData>>,

  pub dynamic_group_size: u32,
  pub dynamic_private_size: u32,

  /// Keep these private; improper usage can result in the use of
  /// stale data/allocations
  begin_fence: FenceScope,
  end_fence: FenceScope,

  f: Function,
  /// To ensure we are only called with this argument type.
  _arg: PhantomData<*const A>,
}
impl<A> FuncModule<A> {
  pub fn new<F>(accel: &Arc<HsaAmdGpuAccel>, f: F) -> Self
    where F: for<'a> Fn(&'a A) + Sized,
  {
    let f = Function::new(f);
    let context_data = f.context_data
      .get_cache_data(accel.ctx());

    FuncModule {
      device: accel.clone(),
      context_data,
      module_data: None,
      dynamic_group_size: 0,
      dynamic_private_size: 0,
      begin_fence: FenceScope::System,
      end_fence: FenceScope::System,
      f,
      _arg: PhantomData,
    }
  }
}
impl<A> FuncModule<A> {
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
        .compile(&self.device, self.f.desc(),
                 &self.device.codegen())?;
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
      let desc = self.f.desc();
      spawn(move || {
        // ignore errors here; if an error does happen,
        // we'll compile again to get the actual error from codegen.
        let _ = context_data.compile(&device, desc, device.codegen());
      });
    }
  }

  /// Undefine all params. If this function was already compiled, it will be compiled
  /// again.
  pub fn clear_params(&mut self) {
    self.module_data.take();
    self.f.spec_params.clear();
  }
  /// Undefine a specialization entry. If the key (`f`) has no entry, this does nothing.
  ///
  /// If this function was already compiled, it will be compiled again.
  pub fn undefine_param<F, R>(&mut self, f: F)
    where F: Fn() -> R,
  {
    self.module_data.take();
    self.f.spec_params.undefine(f)
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
    self.f.spec_params.define(f, value)
  }
}

pub type CallError = crate::error::Error;

/// A trait so downstream crates don't need ndarray to use generic dims.
pub trait LaunchDims: Copy {
  fn default_unit() -> Self;
  fn workgroup(self) -> Result<(u16, u16, u16), CallError>;
  fn grid(self) -> Result<(u32, u32, u32), CallError>;
}
// HSA/AMDGPUs only support up to 3d, so that's all we're going to support here.
macro_rules! impl_launch_dims {
  ($($ty:ty,)*) => ($(

impl LaunchDims for ($ty, ) {
  fn default_unit() -> Self { (1, ) }
  fn workgroup(self) -> Result<(u16, u16, u16), CallError> {
    Ok((self.0.to_u16().ok_or(CallError::Overflow)?, 1, 1, ))
  }
  fn grid(self) -> Result<(u32, u32, u32), CallError> {
    Ok((self.0.to_u32().ok_or(CallError::Overflow)?, 1, 1, ))
  }
}
impl LaunchDims for ($ty, $ty, ) {
  fn default_unit() -> Self { (1, 1, ) }
  fn workgroup(self) -> Result<(u16, u16, u16), CallError> {
    Ok((
      self.0.to_u16().ok_or(CallError::Overflow)?,
      self.1.to_u16().ok_or(CallError::Overflow)?,
      1,
    ))
  }
  fn grid(self) -> Result<(u32, u32, u32), CallError> {
    Ok((
      self.0.to_u32().ok_or(CallError::Overflow)?,
      self.1.to_u32().ok_or(CallError::Overflow)?,
      1,
    ))
  }
}
impl LaunchDims for ($ty, $ty, $ty, ) {
  fn default_unit() -> Self { (1, 1, 1, ) }
  fn workgroup(self) -> Result<(u16, u16, u16), CallError> {
    Ok((
      self.0.to_u16().ok_or(CallError::Overflow)?,
      self.1.to_u16().ok_or(CallError::Overflow)?,
      self.2.to_u16().ok_or(CallError::Overflow)?,
    ))
  }
  fn grid(self) -> Result<(u32, u32, u32), CallError> {
    Ok((
      self.0.to_u32().ok_or(CallError::Overflow)?,
      self.1.to_u32().ok_or(CallError::Overflow)?,
      self.2.to_u32().ok_or(CallError::Overflow)?,
    ))
  }
}

  )*);
}
impl_launch_dims!(u16, u32, u64, usize, );

#[derive(Clone)]
pub struct Invoc<A, Dim>
  where A: Sized + Unpin,
        Dim: LaunchDims,
{
  pub workgroup_dim: Dim,
  pub grid_dim: Dim,

  fmod: FuncModule<A>,
}

impl<A, Dim> Invoc<A, Dim>
  where A: Deps + Sized + Unpin,
        Dim: LaunchDims,
{
  pub fn new<F>(accel: &Arc<HsaAmdGpuAccel>, f: F)
    -> Result<Self, Error>
    where F: for<'a> Fn(&'a A) + Sized,
  {
    let fmod = FuncModule::new(accel, f);
    Ok(Invoc {
      workgroup_dim: Dim::default_unit(),
      grid_dim: Dim::default_unit(),
      fmod,
    })
  }
  pub fn new_dims<F>(accel: &Arc<HsaAmdGpuAccel>,
                     f: F, wg: Dim, grid: Dim)
    -> Result<Self, Error>
    where F: for<'a> Fn(&'a A) + Sized,
  {
    let fmod = FuncModule::new(accel, f);
    Ok(Invoc {
      workgroup_dim: wg,
      grid_dim: grid,
      fmod,
    })
  }
}

impl<A, Dim> Invoc<A, Dim>
  where A: Deps + Sized + Unpin,
        Dim: LaunchDims,
{
  pub fn from(fmod: FuncModule<A>,
              wg: Dim, grid: Dim) -> Self
  {
    Invoc {
      workgroup_dim: wg,
      grid_dim: grid,
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

  /// `completion` must already have the correct value set, eg set to `1`.
  /// This function does no argument checking to ensure, eg, you're not passing
  /// anything by mutable reference or something with drop code by value.
  pub unsafe fn unchecked_call_async<P, Q, T, K, CS>(&mut self,
                                                     args: A,
                                                     queue: Q,
                                                     completion: CS,
                                                     args_pool: P)
    -> Result<InvocCompletion<P, A, Q, CS>, CallError>
    where P: Deref<Target = ArgsPool> + Clone,
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
  pub unsafe fn try_unchecked_call_async<P, Q, T, K, CS>(&mut self,
                                                         args: &mut Option<A>,
                                                         queue: &mut Option<Q>,
                                                         completion: &mut Option<CS>,
                                                         args_pool: P)
    -> Result<InvocCompletion<P, A, Q, CS>, CallError>
    where P: Deref<Target = ArgsPool> + Clone,
          Q: Deref<Target = T>,
          T: IQueue<K>,
          K: QueueKind,
          CS: SignalHandle,
  {
    let kernel_object = {
      let kernel = self.fmod
        .compile_internal()?;

      assert!(kernel.desc.kernarg_segment_size as usize <= size_of::<(&A, )>());

      kernel
        .kernel_object
        .get()
    };

    let mut kernargs = args_pool.alloc::<(A, &A, )>()
      .ok_or(CallError::KernelArgsPoolOom)?;
    let kargs = kernargs.cast();
    let kargs_ref = (&mut kernargs.as_mut().1) as *mut &A;

    let Invoc {
      workgroup_dim,
      grid_dim,
      ref mut fmod,
    } = self;

    let q = queue.as_ref().expect("provide a queue pls");
    let c = completion.as_ref()
      .map(|c| c.signal_ref() );

    let dispatch = DispatchPacket {
      workgroup_size: workgroup_dim.workgroup()?,
      grid_size: grid_dim.grid()?,
      group_segment_size: fmod.group_size().unwrap(),
      private_segment_size: fmod.private_size().unwrap(),
      scaquire_scope: fmod.begin_fence.clone(),
      screlease_scope: fmod.end_fence.clone(),
      ordered: c.is_none(),
      kernel_object,
      kernel_args: &*kargs_ref,
      completion_signal: c,
    };

    // enqueue the dep barriers. this is done after kernarg allocation
    // so that this step isn't repeated if we're called again as a result
    // of kernarg alloc failure.
    {
      let mut signals: ArrayVec<[SignalRef; 5]> = ArrayVec::new();
      {
        let mut f = |sig: &dyn DeviceConsumable| -> Result<(), CallError> {
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
          .iter_deps(&mut f)?;
      }
      if signals.len() > 0 {
        let mut deps = signals.iter();
        q.try_enqueue_barrier_and(&mut deps, None)?;
        assert_eq!(deps.len(), 0);
      }
    }

    ptr::write(kargs.as_ptr(), args.take().unwrap());
    ptr::write(kargs_ref, kargs.as_ref());
    // Ensure the writes to the kernel args are all the way to memory:
    atomic::fence(Ordering::SeqCst);

    let kargs = Box::from_raw_in(kargs.as_ptr(),
                                 ArgsPoolAlloc(args_pool));

    q.try_enqueue_kernel_dispatch(dispatch)?;

    let inner = InvocCompletion {
      _queue: queue.take().unwrap(),
      signal: completion.take().unwrap(),
      waited: AtomicBool::new(false),
      args: Box::into_pin(kargs),
    };
    Ok(inner)
  }
}
impl<A, WGDim> Deref for Invoc<A, WGDim>
  where A: Sized + Unpin,
        WGDim: LaunchDims,
{
  type Target = FuncModule<A>;
  fn deref(&self) -> &FuncModule<A> { &self.fmod }
}
impl<A, WGDim> DerefMut for Invoc<A, WGDim>
  where A: Sized + Unpin,
        WGDim: LaunchDims,
{
  fn deref_mut(&mut self) -> &mut FuncModule<A> { &mut self.fmod }
}

unsafe impl<P, A, Q, S> deps::Deps for InvocCompletion<P, A, Q, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: DeviceConsumable,
        A: ?Sized,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    f(&self.signal)
  }
}
impl<P, A, Q, S> Deref for InvocCompletion<P, A, Q, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle,
        A: ?Sized,
{
  type Target = A;
  fn deref(&self) -> &Self::Target {
    &self.args
  }
}
#[must_use]
pub struct InvocCompletion<P, A, Q, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle,
        A: ?Sized,
{
  _queue: Q,
  signal: S,
  /// a flag which we will set when we get used as a dep in
  /// another invoc. Only used when our completion signal
  /// is a device signal.
  waited: AtomicBool,
  /// Keep this last so we can be unsized
  args: Pin<Box<A, args_pool::ArgsPoolAlloc<P>>>,
}
impl<P, A, Q, S> InvocCompletion<P, A, Q, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle,
        A: ?Sized,
{
  pub unsafe fn wait_ref<F, R>(&self, f: F) -> SignaledDeref<R, &S>
    where F: FnOnce(Pin<&A>) -> R,
          S: HostConsumable,
  {
    let args = self.args.as_ref();
    let signal = &self.signal;
    let r = f(args);

    SignaledDeref::new(r, signal)
  }

  pub fn ret<R>(self, ret: R) -> InvocCompletionReturn<P, A, Q, S, R>
    where A: Sized,
  {
    InvocCompletionReturn {
      ret,
      invoc: self,
    }
  }
}
impl<P, A, Q, S> InvocCompletion<P, A, Q, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: HostConsumable,
        A: ?Sized,
{ }
// impl the signal traits so that invoc completions can be reused for multiple
// kernel dispatches.
impl<P, A, Q, S> SignalHandle for InvocCompletion<P, A, Q, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle,
        A: ?Sized,
{
  fn signal_ref(&self) -> &SignalRef {
    self.signal
      .signal_ref()
  }

  unsafe fn mark_consumed(&self) {
    self.waited
      .store(true, Ordering::Release)
  }

  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    self.signal
      .as_host_consumable()
  }
}
impl<P, A, Q, S> DeviceConsumable for InvocCompletion<P, A, Q, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: DeviceConsumable,
        A: ?Sized,
{
  fn usable_on_device(&self, id: AcceleratorId) -> bool {
    self.signal
      .usable_on_device(id)
  }
}
impl<P, A, Q, S> HostConsumable for InvocCompletion<P, A, Q, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: HostConsumable,
        A: ?Sized,
{
  unsafe fn wait_for_zero_relaxed(&self, spin: bool) -> Result<(), Value> {
    self.signal.wait_for_zero_relaxed(spin)?;
    self.mark_consumed();
    Ok(())
  }
  fn wait_for_zero(&self, spin: bool) -> Result<(), Value> {
    self.signal.wait_for_zero(spin)?;
    unsafe { self.mark_consumed() };
    Ok(())
  }
}
impl<P, A, Q, S> Drop for InvocCompletion<P, A, Q, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle,
        A: ?Sized,
{
  fn drop(&mut self) {
    let mut waited = self.waited.load(Ordering::Acquire);
    if !waited {
      if let Some(host) = self.signal.as_host_consumable() {
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
impl<P, A1, A2, Q, S> CoerceUnsized<InvocCompletion<P, A2, Q, S>> for InvocCompletion<P, A1, Q, S>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle,
        A1: Unsize<A2> + ?Sized,
        A2: ?Sized,
{ }

pub struct InvocCompletionReturn<P, A, Q, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle,
        A: ?Sized,
{
  ret: R,
  invoc: InvocCompletion<P, A, Q, S>,
}
impl<P, A, Q, S, R> SignalHandle for InvocCompletionReturn<P, A, Q, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
        S: SignalHandle,
        A: ?Sized,
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
impl<P, A, Q, S, R> DeviceConsumable for InvocCompletionReturn<P, A, Q, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
        S: DeviceConsumable,
        A: ?Sized,
{
  fn usable_on_device(&self, id: AcceleratorId) -> bool {
    self.invoc.usable_on_device(id)
  }
}
impl<P, A, Q, S, R> HostConsumable for InvocCompletionReturn<P, A, Q, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
        S: HostConsumable,
        A: ?Sized,
{
  unsafe fn wait_for_zero_relaxed(&self, spin: bool) -> Result<(), Value> {
    self.invoc.wait_for_zero_relaxed(spin)
  }
  fn wait_for_zero(&self, spin: bool) -> Result<(), Value> {
    self.invoc.wait_for_zero(spin)
  }
}
unsafe impl<P, A, Q, S, R> deps::Deps for InvocCompletionReturn<P, A, Q, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
        S: DeviceConsumable,
        A: ?Sized,
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
impl<P, A, Q, S, R> Deref for InvocCompletionReturn<P, A, Q, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
        S: DeviceConsumable,
        A: ?Sized,
{
  type Target = AccelRefRaw<R>;
  fn deref(&self) -> &Self::Target {
    unsafe { transmute(&self.ret) }
  }
}
impl<P, A1, A2, Q, S, R>
  CoerceUnsized<InvocCompletionReturn<P, A2, Q, S, R>> for
    InvocCompletionReturn<P, A1, Q, S, R>
  where P: Deref<Target = ArgsPool> + Clone,
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
