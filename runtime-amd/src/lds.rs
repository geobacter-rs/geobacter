//! *Almost completely* safe LDS storage. Use it by calling the `lds!` macro inside your kernel. Examples:
//! ```rust
//! use geobacter_runtime_amd::prelude::*;
//!
//! #[derive(GeobacterDeps)]
//! struct YourGpuKernel {
//!  queue: DeviceMultiQueue,
//!  completion: GlobalSignal,
//! }
//! impl Completion for YourGpuKernel {
//!   type CompletionSignal = GlobalSignal;
//!   fn completion(&self) -> &GlobalSignal { &self.completion }
//! }
//! impl Kernel for YourGpuKernel {
//!   type Grid = Dim1D<RangeTo<u32>>;
//!   const WORKGROUP: Dim1D<RangeTo<u16>> = Dim1D {
//!     x: ..16,
//!   };
//!
//!   type Queue = DeviceMultiQueue;
//!   fn queue(&self) -> &DeviceMultiQueue { &self.queue }
//!
//!   fn kernel(&self, vp: KVectorParams<Self>) {
//!     // Note: the inner array's size here must match the workgroup grid.
//!     // Unfortunately, this isn't enforceable at compile time (in the current version of Rust,
//!     // that is); at runtime, out of bounds workitmes will be "killed" (masked off for the rest
//!     // of the kernel).
//!     lds! {
//!       let mut lds: Lds<[u32; 16]> = Lds::new();
//!     }
//!
//!     lds.with_mut(|mut lds| {
//!       let mut lds = lds.init(&vp, 0u32);
//!       // `lds` is unique to the current workitem; you can read or write to it
//!       // as you would any other mutable reference:
//!       let lds_value = *lds;
//!       // here, `lds_value == 0u32`
//!       *lds = 1u32;
//!     });
//!   }
//! }
//!
//! let ctx = Context::new().unwrap();
//! let dev = HsaAmdGpuAccel::first_device(&ctx).unwrap();
//! let args_pool = ArgsPool::new_arena(&dev, 1024).unwrap();
//!
//! let grid = Dim1D {
//!   x: ..32,
//! };
//!
//! let mut invoc = YourGpuKernel::module(&dev)
//!   .into_invoc(&args_pool);
//! unsafe {
//!   let args = YourGpuKernel {
//!     queue: dev.create_multi_queue(None).unwrap(),
//!     completion: GlobalSignal::new(1).unwrap(),
//!   };
//!   invoc.unchecked_call_async(&grid, args)
//!     .unwrap()
//!     .wait_for_zero(true)
//!     .unwrap();
//! }
//! ```
//! For use with 2d/3d grids:
//! ```rust
//! use geobacter_runtime_amd::prelude::*;
//!
//! #[derive(GeobacterDeps)]
//! struct YourGpuKernel {
//!  queue: DeviceMultiQueue,
//!  completion: GlobalSignal,
//! }
//! impl Completion for YourGpuKernel {
//!   type CompletionSignal = GlobalSignal;
//!   fn completion(&self) -> &GlobalSignal { &self.completion }
//! }
//! impl Kernel for YourGpuKernel {
//!   type Grid = Dim3D<RangeTo<u32>>;
//!   const WORKGROUP: Dim3D<RangeTo<u16>> = Dim3D {
//!     x: ..16,
//!     y: ..8,
//!     z: ..4,
//!   };
//!
//!   type Queue = DeviceMultiQueue;
//!   fn queue(&self) -> &DeviceMultiQueue { &self.queue }
//!
//!   fn kernel(&self, vp: KVectorParams<Self>) {
//!     lds! {
//!       let mut lds: Lds<[[[u32; 16]; 8]; 4]> = Lds::new();
//!     }
//!
//!     lds.with_mut(|mut lds| {
//!       let mut lds = lds.init(&vp, 0u32);
//!       let lds_value = *lds;
//!       // here, `lds_value == 0u32`
//!       *lds = 1u32;
//!     });
//!   }
//! }
//!
//! let ctx = Context::new().unwrap();
//! let dev = HsaAmdGpuAccel::first_device(&ctx).unwrap();
//! let args_pool = ArgsPool::new_arena(&dev, 1024).unwrap();
//!
//! let grid = Dim3D {
//!   x: ..32,
//!   y: ..32,
//!   z: ..32,
//! };
//!
//! let mut invoc = YourGpuKernel::module(&dev)
//!   .into_invoc(&args_pool);
//! unsafe {
//!   let args = YourGpuKernel {
//!     queue: dev.create_multi_queue(None).unwrap(),
//!     completion: GlobalSignal::new(1).unwrap(),
//!   };
//!   invoc.unchecked_call_async(&grid, args)
//!     .unwrap()
//!     .wait_for_zero(true)
//!     .unwrap();
//! }
//! ```
//! You can also use LDS in a workgroup shared manner:
//! ```rust
//! use geobacter_runtime_amd::prelude::*;
//!
//! #[derive(GeobacterDeps)]
//! struct YourGpuKernel {
//!  queue: DeviceMultiQueue,
//!  completion: GlobalSignal,
//! }
//! impl Completion for YourGpuKernel {
//!   type CompletionSignal = GlobalSignal;
//!   fn completion(&self) -> &GlobalSignal { &self.completion }
//! }
//! impl Kernel for YourGpuKernel {
//!   type Grid = Dim3D<RangeTo<u32>>;
//!   const WORKGROUP: Dim3D<RangeTo<u16>> = Dim3D {
//!     x: ..8,
//!     y: ..8,
//!     z: ..8,
//!   };
//!
//!   type Queue = DeviceMultiQueue;
//!   fn queue(&self) -> &DeviceMultiQueue { &self.queue }
//!
//!   fn kernel(&self, vp: KVectorParams<Self>) {
//!     lds! {
//!       let mut lds: Lds<[[[u32; 8]; 8]; 8]> = Lds::new();
//!     }
//!
//!     let idx = Dim3D {
//!       x: 0,
//!       y: 1,
//!       z: 2,
//!     };
//!     lds.with_shared(|mut lds| {
//!       let lds = lds.init(&vp, 1u32);
//!       let lds_value = lds[idx];
//!       // here, `lds_value == 1u32`
//!       // you can also:
//!       let same_lds_value = (&*lds)[0][1][2];
//!     });
//!   }
//! }
//!
//! let ctx = Context::new().unwrap();
//! let dev = HsaAmdGpuAccel::first_device(&ctx).unwrap();
//! let args_pool = ArgsPool::new_arena(&dev, 1024).unwrap();
//!
//! let grid = Dim3D {
//!   x: ..32,
//!   y: ..32,
//!   z: ..32,
//! };
//!
//! let mut invoc = YourGpuKernel::module(&dev)
//!   .into_invoc(&args_pool);
//! unsafe {
//!   let args = YourGpuKernel {
//!     queue: dev.create_multi_queue(None).unwrap(),
//!     completion: GlobalSignal::new(1).unwrap(),
//!   };
//!   invoc.unchecked_call_async(&grid, args)
//!     .unwrap()
//!     .wait_for_zero(true)
//!     .unwrap();
//! }
//! ```
//! Note that the usual Rust rules apply here: the shared LDS reference can not be written to.
//!
//! Mixing up the grid dimension will result in a compiler error:
//! ```compile_fail
//! use geobacter_runtime_amd::prelude::*;
//!
//! #[derive(GeobacterDeps)]
//! struct YourGpuKernel {
//!  queue: DeviceMultiQueue,
//!  completion: GlobalSignal,
//! }
//! impl Completion for YourGpuKernel {
//!   type CompletionSignal = GlobalSignal;
//!   fn completion(&self) -> &GlobalSignal { &self.completion }
//! }
//! impl Kernel for YourGpuKernel {
//!   type Grid = Dim3D<RangeTo<u32>>;
//!   const WORKGROUP: Dim3D<RangeTo<u16>> = Dim3D {
//!     x: ..8,
//!     y: ..8,
//!     z: ..8,
//!   };
//!
//!   type Queue = DeviceMultiQueue;
//!   fn queue(&self) -> &DeviceMultiQueue { &self.queue }
//!
//!   fn kernel(&self, vp: KVectorParams<Self>) {
//!     lds! {
//!       let mut lds: Lds<[[u32; 8]; 8]> = Lds::new();
//!     }
//!     lds.with_shared(|mut lds| {
//!       let _ = lds.init(&vp, 1u32);
//!     });
//!   }
//! }
//! ```
//!
//! There is still one way you can get UB without leaving safe code:
//! ```rust
//! use geobacter_runtime_amd::{prelude::*, lds::LdsBorrow, };
//!
//! #[derive(GeobacterDeps)]
//! struct YourGpuKernel {
//!  queue: DeviceMultiQueue,
//!  completion: GlobalSignal,
//! }
//! impl Completion for YourGpuKernel {
//!   type CompletionSignal = GlobalSignal;
//!   fn completion(&self) -> &GlobalSignal { &self.completion }
//! }
//! impl Kernel for YourGpuKernel {
//!   type Grid = Dim3D<RangeTo<u32>>;
//!   const WORKGROUP: Dim3D<RangeTo<u16>> = Dim3D {
//!     x: ..8,
//!     y: ..8,
//!     z: ..8,
//!   };
//!
//!   type Queue = DeviceMultiQueue;
//!   fn queue(&self) -> &DeviceMultiQueue { &self.queue }
//!
//!   fn kernel(&self, vp: KVectorParams<Self>) {
//!     fn inner_fn() -> LdsBorrow<[[u32; 8]; 8]> {
//!       lds! {
//!         let mut lds: Lds<[[u32; 8]; 8]> = Lds::new();
//!       }
//!       lds
//!     }
//!
//!     let first_borrow = inner_fn();
//!     let second_borrow = inner_fn();
//!
//!     // you can now initialize both at the same time, which is super-duper UB.
//!   }
//! }
//! ```
//! Unfortunately, you won't get any error doing this (without language/compiler
//! changes to make this sort of thing illegal at least); it's the nature of `static`s
//! in Rust. DO NOT DO THE ABOVE.
//!
//! Types with drops are supported and safe.
//!

use std::cell::{Cell, UnsafeCell, };
use std::marker::*;
use std::mem::{MaybeUninit, };
use std::ops::*;
use std::ptr::*;

use super::*;
use crate::module::{GridDims, VectorParams, WorkgroupDims, grid::*, };

/// Do not use directly, use the `lds!` macro.
#[allow(unused_attributes)] // TODO
#[geobacter(amdgpu(runtime_item = "lds_array"))]
pub struct Lds<A>(UnsafeCell<MaybeUninit<A>>)
  where A: Sized;

/// See the module docs for usage.
#[macro_export]
macro_rules! lds {
  ($(let mut $name:ident: Lds<$ty:ty> = Lds::new();)*) => {$(
    let mut $name: $crate::lds::LdsBorrow<$ty> = {
      #[allow(non_upper_case_globals)]
      static $name: $crate::lds::Lds<$ty> = unsafe { $crate::lds::Lds::new() };
      unsafe { $name.borrow() }
    };
  )*};
}

impl<A> Lds<A>
  where A: Sized,
{
  /// Do not call on your own.
  #[doc(hidden)]
  pub const unsafe fn new() -> Self {
    Lds(UnsafeCell::new(MaybeUninit::uninit()))
  }

  /// Do not call on your own.
  #[doc(hidden)]
  pub unsafe fn borrow(&'static self) -> LdsBorrow<A> {
    LdsBorrow(NonNull::from(&self.0), false)
  }
}

/// We directly ensure workitems don't clobber each other
unsafe impl<A> Sync for Lds<A>
  where A: Sized,
{ }

fn enforce_amdgpu() {
  use std::geobacter::platform::platform;
  if !platform().is_amdgcn() {
    panic!("Use LDSArray only on AMDGPU");
  }
}

/// Used to prevent multiple in-scope mutable borrows of the LDS data.
/// Note: `A` does not need to live as long as `'a`, it only needs to live as long
/// as the mutable reference returned from the initialization functions.
/// Note: this type can only be actually used on the GPU (ie it can be constructed on the host, but
/// it will panic if you attempt to call any method on it).
// No Copy or Clone on this structure.
pub struct LdsBorrow<A>(NonNull<UnsafeCell<MaybeUninit<A>>>, bool);

impl<A> LdsBorrow<A> {
  /// Our types here are only correct when the initialization functions are not switched mid
  /// kernel.
  #[inline(always)]
  fn mark_used(&mut self) {
    if self.1 {
      // ensure that any subsequent new borrow will not conflict with any to-be-completed
      // drops in other workitems.
      std::geobacter::amdgpu::sync::workgroup_barrier();
    }
    self.1 = true;
  }
  #[inline(always)]
  pub fn with_singleton<'a, F, R>(&'a mut self, f: F) -> R
    where F: FnOnce(Singleton<'a, A>) -> R,
  {
    self.mark_used();
    let v = Singleton(self);
    f(v)
  }
  #[inline(always)]
  pub fn with_shared<'a, F, R>(&'a mut self, f: F) -> R
    where F: FnOnce(Shared<'a, A>) -> R,
  {
    self.mark_used();
    let v = Shared(self);
    f(v)
  }
  #[inline(always)]
  pub fn with_mut<'a, F, R>(&'a mut self, f: F) -> R
    where F: FnOnce(Unique<'a, A>) -> R,
  {
    self.mark_used();
    let v = Unique(self);
    f(v)
  }
}

// These are separate structures because we must ensure that only one type is used per
// kernel/scope.
macro_rules! lds_usage_type {
  ($($ty:ident, )*) => {$(
    #[repr(transparent)]
    pub struct $ty<'a, A>(&'a mut LdsBorrow<A>);
  )*}
}
lds_usage_type! {
  Singleton, Shared, Unique,
}

impl<'a, A> Singleton<'a, A> {
  pub fn init<G>(&mut self, vp: &VectorParams<G>, v: A)
    -> Ref<A>
    where A: Sync,
          G: GridDims,
          <G::Workgroup as WorkgroupDims>::Idx: num_traits::Zero,
  {
    self.init_with(vp, move || v)
  }
  pub fn init_with<F, G>(&mut self, vp: &VectorParams<G>, f: F)
    -> Ref<A>
    where A: Sync,
          G: GridDims,
          <G::Workgroup as WorkgroupDims>::Idx: num_traits::Zero,
          F: FnOnce() -> A,
  {
    enforce_amdgpu();

    // Ensure all workitems have dropped any previous LDS reference:
    std::geobacter::amdgpu::sync::workgroup_barrier();

    unsafe {
      self.unchecked_init_with(vp, f)
    }
  }
  pub unsafe fn unchecked_init_with<F, G>(&mut self, vp: &VectorParams<G>, f: F)
    -> Ref<A>
    where A: Sync,
          G: GridDims,
          <G::Workgroup as WorkgroupDims>::Idx: num_traits::Zero,
          F: FnOnce() -> A,
  {
    enforce_amdgpu();

    let wi0 = vp.is_wi0();
    let out = Ref({
      let uninit_ptr = &mut *(self.0).0.as_ref().get();

      if wi0 {
        // Safety: this section is only run on the first workitem, so this mut is thus unique.
        write(&mut *uninit_ptr.as_mut_ptr(), f());
      }

      NonNull::from(&*uninit_ptr.as_ptr())
    }, PhantomData, wi0);

    out
  }
}

impl<'a, A> Unique<'a, A> {
  #[inline(always)]
  pub fn init<'b, E, G>(&'b mut self, vp: &VectorParams<G>, v: E) -> Mut<'b, E>
    where A: LdsArray<E, Grid = G::Workgroup>,
          G: GridDims,
          E: 'b,
  {
    self.init_with(vp, move || v)
  }
  #[inline(always)]
  pub fn init_default<'b, E, G>(&'b mut self, vp: &VectorParams<G>)
    -> Mut<'b, E>
    where A: LdsArray<E, Grid = G::Workgroup>,
          E: Default + 'b,
          G: GridDims,
  {
    self.init_with(vp, E::default)
  }
  /// Initialize an LDS slot, and return a mutable reference to the slot.
  pub fn init_with<'b, E, F, G>(&'b mut self, vp: &VectorParams<G>, f: F)
    -> Mut<E>
    where A: LdsArray<E, Grid = G::Workgroup>,
          E: 'b,
          F: FnOnce() -> E,
          G: GridDims,
  {
    enforce_amdgpu();
    Mut(unsafe {
      // safety: hardware ensures idx is unique per workitem. The runtime ensures
      // `self` is placed in LDS, thus `self` is unique per workgroup.
      let uninit_ptr = &mut *(self.0).0.as_ref().get();
      let uninit_mut = &mut *uninit_ptr.as_mut_ptr();
      let uninit = A::workitem_index_mut(uninit_mut, &vp.wi());

      write(uninit, f()); // initialize our idx

      uninit
    })
  }
}
impl<'a, A> Shared<'a, A> {
  #[inline(always)]
  pub fn init<'b, E, G>(&'b mut self, vp: &VectorParams<G>, v: E)
    -> Slice<'b, A, E, G>
    where A: LdsArray<E, Grid = G::Workgroup> + Sync + 'b,
          E: Sync + 'b,
          G: GridDims,
  {
    self.init_with(vp, move || v )
  }
  #[inline(always)]
  pub fn init_default<'b, E, G>(&'b mut self, vp: &VectorParams<G>)
    -> Slice<'b, A, E, G>
    where A: LdsArray<E, Grid = G::Workgroup> + Sync + 'b,
          E: Sync + Default + 'b,
          G: GridDims,
  {
    self.init_with(vp, E::default)
  }
  pub fn init_with<'b, E, F, G>(&'b mut self, vp: &VectorParams<G>, f: F)
    -> Slice<'b, A, E, G>
    where A: LdsArray<E, Grid = G::Workgroup> + Sync + 'b,
          E: Sync + 'b,
          F: FnOnce() -> E,
          G: GridDims,
  {
    enforce_amdgpu();
    // We don't have to run a barrier first before initializing our LDS slot because either:
    // * this is the first time this LDS space is initialized, in which case no references to
    //   this LDS space can safely exist.
    // * it isn't the first time, in which case `LdsSlice` waits on a workgroup barrier before
    //   dropping.
    unsafe {
      let uninit_ptr = &mut *(self.0).0.as_ref().get();
      let uninit_mut = &mut *uninit_ptr.as_mut_ptr();

      let wi = vp.wi();

      // safety: hardware ensures idx is unique per workitem. The runtime ensures
      // `self` is placed in LDS, thus `self` is unique per workgroup.
      write(A::workitem_index_mut(uninit_mut, &wi), f()); // initialize our slot

      Slice {
        owned: A::workitem_index(uninit_mut, &wi).into(),
        s: NonNull::from(uninit_mut),
        initial_barrier: Cell::new(false),
        _g: PhantomData,
        _mb: PhantomData,
      }
    }
  }
}

pub struct Slice<'a, A, E, G>
  where A: LdsArray<E, Grid = G::Workgroup> + Sync + ?Sized + 'a,
        G: GridDims,
{
  /// The entire workgroup slice.
  s: NonNull<A>,
  /// The slot we're responsible for dropping.
  owned: NonNull<E>,
  /// We have to run a barrier before a single wavefront tries to use LDS data.
  /// Last because alignment.
  initial_barrier: Cell<bool>,
  _g: PhantomData<G>,
  _mb: PhantomData<&'a mut A>,
}

impl<'a, A, E, G> Slice<'a, A, E, G>
  where A: LdsArray<E, Grid = G::Workgroup> + Sync + ?Sized + 'a,
        G: GridDims,
{
  /// Disable the initial barrier. This is unsafe. Useful if you have another LDS object you're
  /// using.
  pub unsafe fn force_no_barrier(&self) {
    self.initial_barrier.set(true);
  }
  pub fn enforce_initial_barrier(&self) {
    // this barrier is executed before any references are safely given out.
    // thus we don't need acqrel fences.
    if !self.initial_barrier.get() {
      self.initial_barrier.set(true);
      std::geobacter::amdgpu::sync::workgroup_barrier();
    }
  }
  pub fn owned(&self) -> &E {
    // we don't need a barrier here, as we own this slot.

    // invariant: no other mutable references can exist. True because we don't safely allow it.
    unsafe { self.owned.as_ref() }
  }
  /// Does not run a workgroup acqrel barrier before giving you the mutable reference.
  pub unsafe fn unchecked_owned_mut(&mut self) -> &mut E {
    self.owned.as_mut()
  }
  /// Gets the LDS data without ensuring the barrier was executed.
  pub unsafe fn unchecked_ref(&self) -> &A {
    self.s.as_ref()
  }

  pub fn as_ptr(&self) -> *const A {
    self.enforce_initial_barrier();
    self.s.as_ptr()
  }
  pub unsafe fn unchecked_as_ptr(&self) -> *const A {
    self.s.as_ptr()
  }

  /// Does not execute a barrier before dropping, nor afterwards
  pub unsafe fn unsynced_drop(mut self) {
    drop_in_place(self.owned.as_mut());

    std::mem::forget(self);
  }
}

impl<'a, A, E, G> Deref for Slice<'a, A, E, G>
  where A: LdsArray<E, Grid = G::Workgroup> + Sync + ?Sized + 'a,
        G: GridDims,
{
  type Target = A;
  fn deref(&self) -> &A {
    self.enforce_initial_barrier();
    unsafe { self.s.as_ref() }
  }
}
impl<'a, A, E, G> Index<<G::Workgroup as WorkgroupDims>::Idx> for Slice<'a, A, E, G>
  where A: LdsArray<E, Grid = G::Workgroup> + Sync + ?Sized + 'a,
        G: GridDims,
{
  type Output = E;
  fn index(&self, index: <G::Workgroup as WorkgroupDims>::Idx) -> &E {
    self.enforce_initial_barrier();
    A::workitem_index(unsafe { self.s.as_ref() }, &index)
  }
}
impl<'a, A, E, G> fmt::Debug for Slice<'a, A, E, G>
  where A: LdsArray<E, Grid = G::Workgroup> + fmt::Debug + Sync + ?Sized + 'a,
        G: GridDims,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    self.enforce_initial_barrier();
    fmt::Debug::fmt(unsafe { self.s.as_ref() }, f)
  }
}
unsafe impl<'a, #[may_dangle] A, #[may_dangle] E, G> Drop for Slice<'a, A, E, G>
  where A: LdsArray<E, Grid = G::Workgroup> + Sync + ?Sized + 'a,
        G: GridDims,
{
  fn drop(&mut self) {
    // This barrier is required regardless of `impl Drop for E`; we must be sure all other
    // workitems have finished possibly using our LDS slot. Either we run the barrier now, or we
    // wait until right before the next loop iteration when our LDS slot is (re-)initialized (but
    // only for types with no drop code). Since `&mut`+`&` is UB anyway, we do it now.
    std::geobacter::amdgpu::sync::workgroup_barrier();

    unsafe { drop_in_place(self.owned.as_mut()) }

    // we do not need a post-drop barrier; no references can exist now, and only we can
    // initialize our slot (after which, we require another barrier).
  }
}
impl<'a, A1, A2, E, G> CoerceUnsized<Slice<'a, A2, E, G>> for Slice<'a, A1, E, G>
  where A1: LdsArray<E, Grid = G::Workgroup> + Sync + Unsize<A2> + 'a,
        A2: LdsArray<E, Grid = G::Workgroup> + Sync + ?Sized + 'a,
        G: GridDims,
{ }

/// Drops the workitem's LDS slot when this goes out of scope.
// Not safe to construct.
pub struct Mut<'a, A>(&'a mut A)
  where A: 'a;

impl<'a, A> Deref for Mut<'a, A>
  where A: 'a,
{
  type Target = A;
  #[inline(always)]
  fn deref(&self) -> &A { self.0 }
}
impl<'a, A> DerefMut for Mut<'a, A>
  where A: 'a,
{
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut A { self.0 }
}
unsafe impl<'a, #[may_dangle] A> Drop for Mut<'a, A> {
  fn drop(&mut self) {
    unsafe { drop_in_place(self.0) }
    // Note: this doesn't need a post-barrier:
  }
}

/// Drops the "owned reference" on the first workitem only. This is intended to be used workgroup
/// wide.
// Not safe to construct.
pub struct Ref<'a, A>(NonNull<A>, PhantomData<&'a A>, bool)
  where A: 'a;

impl<'a, A> Deref for Ref<'a, A>
  where A: 'a,
{
  type Target = A;
  #[inline(always)]
  fn deref(&self) -> &A { unsafe { self.0.as_ref() } }
}
unsafe impl<'a, #[may_dangle] A> Drop for Ref<'a, A> {
  fn drop(&mut self) {
    // This barrier is required regardless of `impl Drop for E`; we must be sure all other
    // workitems have finished possibly using our LDS slot. Either we run the barrier now, or we
    // wait until right before the next loop iteration when our LDS slot is (re-)initialized (but
    // only for types with no drop code). Since `&mut`+`&` is UB anyway, we do it now.
    if !cfg!(test) || std::geobacter::platform::platform().is_amdgcn() {
      // skip if on the host AND we're testing.
      std::geobacter::amdgpu::sync::workgroup_barrier();
    }

    if self.2 {
      // Only the first workitem runs drop
      unsafe { drop_in_place(self.0.as_mut()) }
    }
  }
}

pub trait LdsArray<E> {
  type Grid: WorkgroupDims;

  #[doc(hidden)]
  fn workitem_index_mut(&mut self, idx: &<Self::Grid as WorkgroupDims>::Idx) -> &mut E;
  #[doc(hidden)]
  fn workitem_index(&self, idx: &<Self::Grid as WorkgroupDims>::Idx) -> &E;
}
impl<E, const X: usize> LdsArray<E> for [E; X] {
  type Grid = Dim1D<RangeTo<u16>>;

  #[doc(hidden)] #[inline(always)]
  fn workitem_index_mut(&mut self, idx: &<Self::Grid as WorkgroupDims>::Idx) -> &mut E {
    &mut self[idx.x as usize]
  }
  #[doc(hidden)] #[inline(always)]
  fn workitem_index(&self, idx: &<Self::Grid as WorkgroupDims>::Idx) -> &E {
    &self[idx.x as usize]
  }
}
impl<E, const X: usize, const Y: usize> LdsArray<E> for [[E; X]; Y] {
  type Grid = Dim2D<RangeTo<u16>>;

  #[doc(hidden)] #[inline(always)]
  fn workitem_index_mut(&mut self, idx: &<Self::Grid as WorkgroupDims>::Idx) -> &mut E {
    &mut self[idx.y as usize][idx.x as usize]
  }
  #[doc(hidden)] #[inline(always)]
  fn workitem_index(&self, idx: &<Self::Grid as WorkgroupDims>::Idx) -> &E {
    &self[idx.y as usize][idx.x as usize]
  }
}
impl<E, const X: usize, const Y: usize, const Z: usize> LdsArray<E> for [[[E; X]; Y]; Z] {
  type Grid = Dim3D<RangeTo<u16>>;
  #[doc(hidden)] #[inline(always)]
  fn workitem_index_mut(&mut self, idx: &<Self::Grid as WorkgroupDims>::Idx) -> &mut E {
    &mut self[idx.z as usize][idx.y as usize][idx.x as usize]
  }
  #[doc(hidden)] #[inline(always)]
  fn workitem_index(&self, idx: &<Self::Grid as WorkgroupDims>::Idx) -> &E {
    &self[idx.z as usize][idx.y as usize][idx.x as usize]
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::utils::test::*;

  #[test]
  fn one_d() {
    #[derive(GeobacterDeps)]
    struct LDSKernel {
      dst: *mut [u32],
      completion: GlobalSignal,
    }
    unsafe impl Send for LDSKernel { }
    unsafe impl Sync for LDSKernel { }
    impl Completion for LDSKernel {
      type CompletionSignal = GlobalSignal;
      fn completion(&self) -> &GlobalSignal { &self.completion }
    }
    impl Kernel for LDSKernel {
      type Grid = Dim1D<RangeTo<u32>>;
      const WORKGROUP: Dim1D<RangeTo<u16>> = Dim1D {
        x: ..16,
      };

      type Queue = DeviceMultiQueue;
      fn queue(&self) -> &DeviceMultiQueue { queue() }

      fn kernel(&self, vp: KVectorParams<Self>) {
        use std::ptr;
        use std::geobacter::amdgpu::{dispatch_packet, sync::atomic::*, };

        lds! {
          let mut lds: Lds<[u32; 16]> = Lds::new();
        }

        let g_lid = dispatch_packet().global_linear_id();
        lds.with_mut(|mut lds| {
          let mut lds = lds.init(&vp, 0u32);

          unsafe {
            // force LLVM to not optimize this away:
            ptr::write_volatile(&mut *lds, g_lid as _);
          }

          work_group_rel_acq_barrier(Scope::WorkGroup);

          unsafe {
            (&mut *self.dst)[g_lid] = ptr::read_volatile(&*lds);
          }
        });
      }
    }

    let dev = device();

    let grid = Dim1D {
      x: ..32,
    };

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    m.resize(grid.linear_len().unwrap() as _, u32::max_value());

    let module = LDSKernel::module(&dev);
    let mut invoc = module.into_invoc(args_pool());
    unsafe {
      let args = LDSKernel {
        dst: m.as_mut_slice(),
        completion: GlobalSignal::new(1).unwrap(),
      };
      invoc.unchecked_call_async(&grid, args)
        .unwrap()
        .wait_for_zero(true)
        .unwrap();
    }

    for (i, &v) in m.iter().enumerate() {
      assert_eq!(i, v as usize);
    }
  }
  #[test]
  fn three_d() {
    #[derive(GeobacterDeps)]
    struct LDSKernel {
      dst: *mut [u32],
      completion: GlobalSignal,
    }
    unsafe impl Send for LDSKernel { }
    unsafe impl Sync for LDSKernel { }
    impl Completion for LDSKernel {
      type CompletionSignal = GlobalSignal;
      fn completion(&self) -> &GlobalSignal { &self.completion }
    }
    impl Kernel for LDSKernel {
      type Grid = Dim3D<RangeTo<u32>>;
      const WORKGROUP: Dim3D<RangeTo<u16>> = Dim3D {
        x: ..8,
        y: ..8,
        z: ..8,
      };

      type Queue = DeviceMultiQueue;
      fn queue(&self) -> &DeviceMultiQueue { queue() }

      fn kernel(&self, vp: KVectorParams<Self>) {
        use std::ptr;
        use std::geobacter::amdgpu::{dispatch_packet, sync::atomic::*, };

        lds! {
          let mut lds: Lds<[[[u32; 8]; 8]; 8]> = Lds::new();
        }

        let g_lid = dispatch_packet().global_linear_id();
        lds.with_mut(|mut lds| {
          let mut lds = lds.init_default(&vp);

          unsafe {
            // force LLVM to not optimize this away:
            ptr::write_volatile(&mut *lds, g_lid as _);
          }

          work_group_rel_acq_barrier(Scope::WorkGroup);

          unsafe {
            (&mut *self.dst)[g_lid] = ptr::read_volatile(&*lds);
          }
        });
      }
    }

    let dev = device();

    let grid = Dim3D {
      x: ..32,
      y: ..32,
      z: ..32,
    };

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    m.resize(grid.linear_len().unwrap() as _, u32::max_value());

    let module = LDSKernel::module(&dev);
    let mut invoc = module.into_invoc(args_pool());
    unsafe {
      let args = LDSKernel {
        dst: m.as_mut_slice(),
        completion: GlobalSignal::new(1).unwrap(),
      };
      invoc.unchecked_call_async(&grid, args)
        .unwrap()
        .wait_for_zero(true)
        .unwrap();
    }

    for (i, &v) in m.iter().enumerate() {
      assert_eq!(i, v as usize);
    }
  }
  #[test]
  fn axis_order() {
    #[derive(GeobacterDeps)]
    struct LDSKernel {
      dst: *mut [u32],
      completion: GlobalSignal,
    }
    unsafe impl Send for LDSKernel { }
    unsafe impl Sync for LDSKernel { }
    impl Completion for LDSKernel {
      type CompletionSignal = GlobalSignal;
      fn completion(&self) -> &GlobalSignal { &self.completion }
    }
    impl Kernel for LDSKernel {
      type Grid = Dim2D<RangeTo<u32>>;
      const WORKGROUP: Dim2D<RangeTo<u16>> = Dim2D {
        x: ..1,
        y: ..2,
      };

      type Queue = DeviceMultiQueue;
      fn queue(&self) -> &DeviceMultiQueue { queue() }

      fn kernel(&self, vp: KVectorParams<Self>) {
        use std::geobacter::amdgpu::{dispatch_packet, sync::atomic::*, };

        lds! {
          let mut lds: Lds<[[u32; 1]; 2]> = Lds::new();
        }

        let g_lid = dispatch_packet().global_linear_id();
        lds.with_shared(|mut lds| {
          let lds = lds.init(&vp, 1);

          work_group_rel_acq_barrier(Scope::WorkGroup);

          unsafe {
            (&mut *self.dst)[g_lid] = lds[vp.wi()];
          }
        });
      }
    }

    let dev = device();

    let grid = Dim2D {
      x: ..3,
      y: ..3,
    };

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    m.resize(grid.linear_len().unwrap() as _, u32::max_value());

    let module = LDSKernel::module(&dev);
    let mut invoc = module.into_invoc(args_pool());
    unsafe {
      let args = LDSKernel {
        dst: m.as_mut_slice(),
        completion: GlobalSignal::new(1).unwrap(),
      };
      invoc.unchecked_call_async(&grid, args)
        .unwrap()
        .wait_for_zero(true)
        .unwrap();
    }

    for &v in m.iter() {
      assert_eq!(v as usize, 1);
    }
  }
  #[test]
  fn drop() {
    #[derive(GeobacterDeps)]
    struct LDSKernel {
      dst: *mut [u8],
      completion: GlobalSignal,
    }
    unsafe impl Send for LDSKernel { }
    unsafe impl Sync for LDSKernel { }
    impl Completion for LDSKernel {
      type CompletionSignal = GlobalSignal;
      fn completion(&self) -> &GlobalSignal { &self.completion }
    }
    impl Kernel for LDSKernel {
      type Grid = Dim3D<RangeTo<u32>>;
      const WORKGROUP: Dim3D<RangeTo<u16>> = Dim3D {
        x: ..8,
        y: ..8,
        z: ..8,
      };

      type Queue = DeviceMultiQueue;
      fn queue(&self) -> &DeviceMultiQueue { queue() }

      fn kernel(&self, vp: KVectorParams<Self>) {
        use std::ptr;
        use std::geobacter::amdgpu::{dispatch_packet, sync::atomic::*, };

        lds! {
          let mut lds: Lds<[[[FlagOnDrop; 8]; 8]; 8]> = Lds::new();
        }

        let mut dropped = 0u8;
        // can't use lifetimes here because Rust will force it to 'static.
        struct FlagOnDrop(*mut u8);
        impl Drop for FlagOnDrop {
          fn drop(&mut self) {
            unsafe { *self.0 = 2; }
          }
        }

        let g_lid = dispatch_packet().global_linear_id();
        lds.with_mut(|mut lds| {
          let mut lds = lds.init(&vp, FlagOnDrop(&mut dropped));
          unsafe {
            // force LLVM to not optimize this away:
            ptr::write_volatile((&mut *lds).0, 1u8);
          }
          work_group_rel_acq_barrier(Scope::WorkGroup);
          // drop `lds`
        });
        unsafe {
          (&mut *self.dst)[g_lid] = ptr::read_volatile(&dropped);
        }
      }
    }

    let dev = device();

    let grid = Dim3D {
      x: ..32,
      y: ..32,
      z: ..32,
    };

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    m.resize(grid.linear_len().unwrap() as _, u8::max_value());

    let module = LDSKernel::module(&dev);
    let mut invoc = module.into_invoc(args_pool());
    unsafe {
      let args = LDSKernel {
        dst: m.as_mut_slice(),
        completion: GlobalSignal::new(1).unwrap(),
      };
      invoc.unchecked_call_async(&grid, args)
        .unwrap()
        .wait_for_zero(true)
        .unwrap();
    }

    for &v in m.iter() {
      assert_eq!(v, 2);
    }
  }
  #[test]
  fn singleton() {
    use num_traits::Zero;

    #[derive(GeobacterDeps)]
    struct LDSKernel {
      dst: *mut [(Dim2D<u16>, bool)],
      completion: GlobalSignal,
    }
    unsafe impl Send for LDSKernel { }
    unsafe impl Sync for LDSKernel { }
    impl Completion for LDSKernel {
      type CompletionSignal = GlobalSignal;
      fn completion(&self) -> &GlobalSignal { &self.completion }
    }
    impl Kernel for LDSKernel {
      type Grid = Dim2D<RangeTo<u32>>;
      const WORKGROUP: Dim2D<RangeTo<u16>> = Dim2D {
        x: ..16,
        y: ..16,
      };

      type Queue = DeviceMultiQueue;
      fn queue(&self) -> &DeviceMultiQueue { queue() }

      fn kernel(&self, vp: KVectorParams<Self>) {
        use std::geobacter::amdgpu::{sync::atomic::*, };

        lds! {
          let mut lds: Lds<Dim2D<u16>> = Lds::new();
        }

        let wi = vp.wi();

        lds.with_singleton(|mut lds| {
          let lds = lds.init_with(&vp, move || {
            // only one workitem will call this closure
            wi
          });

          work_group_rel_acq_barrier(Scope::WorkGroup);

          let eq = *lds == wi;

          unsafe {
            (&mut *self.dst)[vp.gl_id() as usize] = (wi, eq);
          }
        });
      }
    }

    let dev = device();

    let grid = Dim2D {
      x: ..32,
      y: ..32,
    };

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    m.resize(grid.linear_len().unwrap() as _, Default::default());

    let module = LDSKernel::module(&dev);
    let mut invoc = module.into_invoc(args_pool());
    unsafe {
      let args = LDSKernel {
        dst: m.as_mut_slice(),
        completion: GlobalSignal::new(1).unwrap(),
      };
      invoc.unchecked_call_async(&grid, args)
        .unwrap()
        .wait_for_zero(true)
        .unwrap();
    }

    for (i, &(wi, v)) in m.iter().enumerate() {
      let expected = Dim2D {
        x: i % (LDSKernel::WORKGROUP.x.end as usize),
        y: (i / grid.x.end as usize) % LDSKernel::WORKGROUP.y.end as usize,
      }.is_zero();
      assert_eq!(v, expected, "i == {}, v = {}, wi = {:?}", i, v, wi);
    }
  }

  #[test] #[should_panic]
  fn lds_mut_host_panic() {
    static T: Lds<[u8; 1]> = unsafe { Lds::new() };

    let mut t = unsafe { T.borrow() };

    let vp = MaybeUninit::uninit();
    let vp: VectorParams<Dim1D<Range<u32>>> =
      unsafe { vp.assume_init() }; // actually unused

    t.with_mut(|mut t| {
      t.init(&vp, 0);
    });
  }
  #[test] #[should_panic]
  fn lds_shared_host_panic() {
    static T: Lds<[u8; 1]> = unsafe { Lds::new() };

    let mut t = unsafe { T.borrow() };

    let vp = MaybeUninit::uninit();
    let vp: VectorParams<Dim1D<Range<u32>>> =
      unsafe { vp.assume_init() }; // actually unused

    t.with_shared(|mut t| {
      t.init(&vp, 0);
    });
  }
  #[test] #[should_panic]
  fn lds_singleton_host_panic() {
    static T: Lds<[u8; 1]> = unsafe { Lds::new() };

    let mut t = unsafe { T.borrow() };

    let vp = MaybeUninit::uninit();
    let vp: VectorParams<Dim1D<Range<u32>>> =
      unsafe { vp.assume_init() }; // actually unused

    t.with_singleton(|mut t| {
      t.init(&vp, [0; 1]);
    });
  }
  /// LdsArray::borrow should not panic on non-amdgpu platforms.
  #[test]
  fn lds_array_borrow_no_panic() {
    static T: Lds<[u8; 1]> = unsafe { Lds::new() };
    unsafe { T.borrow() };
  }
}
