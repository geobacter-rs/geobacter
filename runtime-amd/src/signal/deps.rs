
#![allow(deprecated)]

use std::cell::Cell;
use std::marker::{PhantomData, PhantomPinned, };
use std::num::{NonZeroI8, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroI128, NonZeroIsize,
               NonZeroU8, NonZeroU16, NonZeroU32, NonZeroU64, NonZeroU128, NonZeroUsize,
               Wrapping, };
use std::ops::*;
use std::ptr::NonNull;
use std::rc::{Rc, };
use std::sync::{Arc, atomic::*, };

use hsa_rt::signal::SignalRef;
use hsa_rt::queue::RingQueue;

use gcore::ptr::*;
use gcore::platform::is_host;

use smallvec::SmallVec;

use crate::{Error, HsaAmdGpuAccel};
use crate::module::{CallError, DeviceMultiQueue, DeviceSingleQueue, Completion, };
use crate::signal::{SignalHandle, DeviceConsumable, DeviceSignal, GlobalSignal,
                    HostConsumable, SignalFactory, };
use crate::boxed::{RawPoolBox, LocallyAccessiblePoolBox, };
use crate::alloc::{LapBox, LapVec};

/// This is unsafe because you must ensure the proper dep signals are registered!
/// You should probably just use the `GeobacterDeps` derive macro to implement this.
pub unsafe trait Deps {
  #[doc(hidden)]
  fn barrier_impl<F, S, Q>(&self, dev: &Arc<HsaAmdGpuAccel>,
                           queue: &Q,
                           completion: &mut S,
                           mut iter_deps: F)
    -> Result<(), Error>
    where F: for<'a> FnMut(&'a Self,
      &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), Error>)
      -> Result<(), Error>,
          S: SignalFactory,
          Q: RingQueue,
  {
    let packets = {
      let mut packets = 0i64;
      let mut count = 0;

      iter_deps(self, &mut |_| {
        count += 1;
        if count == 5 {
          packets += 1;
          count = 0;
        }
        Ok(())
      })?;

      if count != 0 {
        packets += 1;
      }
      packets
    };

    completion.reset(dev, packets)?;
    if packets == 0 {
      return Ok(());
    }

    let mut signals: SmallVec<[SignalRef; 5]> = SmallVec::new();
    {
      let mut f = |sig: &dyn DeviceConsumable| -> Result<(), Error> {
        let sig = unsafe {
          ::std::mem::transmute_copy(sig.signal_ref())
        };
        signals.push(sig);
        if signals.len() == 5 {
          {
            let mut deps = signals.iter();
            queue.try_enqueue_barrier_and(&mut deps,
                                          Some(completion.signal_ref()))?;
            gamd_std::host_debug_assert_eq!(deps.len(), 0);
          }
          signals.clear();
        }

        Ok(())
      };
      iter_deps(self, &mut f)?;
    }
    if signals.len() > 0 {
      let mut deps = signals.iter();
      queue.try_enqueue_barrier_and(&mut deps,
                                    Some(completion.signal_ref()))?;
      gamd_std::host_debug_assert_eq!(deps.len(), 0);
    }

    Ok(())
  }

  fn barrier<S, Q>(&self, dev: &Arc<HsaAmdGpuAccel>, queue: &Q)
    -> Result<S, Error>
    where S: SignalFactory,
          Q: RingQueue,
  {
    let mut c = S::new(dev, 0)?;
    self.barrier_impl(dev,queue, &mut c,
                      Self::iter_deps)?;

    Ok(c)
  }
  fn global_barrier<Q>(&self, dev: &Arc<HsaAmdGpuAccel>, queue: &Q)
    -> Result<GlobalSignal, Error>
    where Q: RingQueue,
  {
    self.barrier(dev, queue)
  }

  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>;
}

unsafe impl Deps for DeviceSignal {
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    f(self)
  }
}
unsafe impl Deps for GlobalSignal {
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    f(self)
  }
}
unsafe impl<T> Deps for Option<T>
  where T: Deps,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    if let &Some(ref v) = self {
      v.iter_deps(f)?;
    }
    Ok(())
  }
}

unsafe impl<'b, T> Deps for &'b T
  where T: Deps + ?Sized,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    (&**self).iter_deps(f)
  }
}
unsafe impl<'b, T> Deps for &'b mut T
  where T: Deps + ?Sized,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    (&**self).iter_deps(f)
  }
}

macro_rules! impl_prim {
  ($($prim:ty,)*) => {$(

unsafe impl Deps for $prim {
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}

  )*};
}
impl_prim!(i8, u8, i16, u16, i32, u32, i64, u64, i128, u128, usize, isize,
           f32, f64, bool, (), );
impl_prim!(AtomicU8, AtomicI8, AtomicU16, AtomicI16, AtomicU32, AtomicI32,
           AtomicU64, AtomicI64, AtomicUsize, AtomicIsize, );
impl_prim!(NonZeroI8, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroI128, NonZeroIsize,
           NonZeroU8, NonZeroU16, NonZeroU32, NonZeroU64, NonZeroU128, NonZeroUsize, );

unsafe impl<T> Deps for Wrapping<T>
  where T: Deps,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    self.0.iter_deps(f)
  }
}

macro_rules! impl_simd {
  ($($prim:ident,)*) => {$(

#[cfg(feature = "packed_simd")]
unsafe impl Deps for packed_simd::$prim {
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}

  )*};
}
impl_simd! {
  f32x2, f32x4, f32x8, f32x16,
  f64x2, f64x4, f64x8,

  i8x2, i8x4, i8x8, i8x16, i8x32, i8x64,
  u8x2, u8x4, u8x8, u8x16, u8x32, u8x64,
  i16x2, i16x4, i16x8, i16x16, i16x32,
  u16x2, u16x4, u16x8, u16x16, u16x32,
  i32x2, i32x4, i32x8, i32x16,
  u32x2, u32x4, u32x8, u32x16,
  i64x2, i64x4, i64x8,
  u64x2, u64x4, u64x8,
  i128x1, i128x2, i128x4,
  u128x1, u128x2, u128x4,

  isizex2, isizex4, isizex8,
  usizex2, usizex4, usizex8,

  m8x2, m8x4, m8x8, m8x16, m8x32, m8x64,
  m16x2, m16x4, m16x8, m16x16, m16x32,
  m32x2, m32x4, m32x8, m32x16,
  m64x2, m64x4, m64x8,
  m128x1, m128x2, m128x4,
}

macro_rules! impl_tuple {
  ($(($($gen:ident, )*),)*) => {$(

unsafe impl<$($gen,)*> Deps for ($($gen,)*)
  where $($gen: Deps),*
{
  #[allow(non_snake_case)]
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    let &($(ref $gen,)*) = self;
    $($gen.iter_deps(f)?;)*
    Ok(())
  }
}

  )*};
}
impl_tuple! {
  (A, ),
  (A, B, ),
  (A, B, C, ),
  (A, B, C, D, ),
  (A, B, C, D, E, ),
  (A, B, C, D, E, F, ),
  (A, B, C, D, E, F, G, ),
  (A, B, C, D, E, F, G, H, ),
  (A, B, C, D, E, F, G, H, I, ),
  (A, B, C, D, E, F, G, H, I, J, ),
  (A, B, C, D, E, F, G, H, I, J, K, ),
}

unsafe impl<T> Deps for *const T
  where T: ?Sized,
{
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}
unsafe impl<T> Deps for *mut T
  where T: ?Sized,
{
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}
unsafe impl<T> Deps for NonNull<T>
  where T: ?Sized,
{
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}

unsafe impl<T> Deps for AccelPtr<T>
  where T: PtrTy,
{
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}

unsafe impl<T> Deps for [T]
  where T: Deps,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    for v in self.iter() {
      v.iter_deps(f)?;
    }
    Ok(())
  }
}
unsafe impl<T> Deps for Rc<T>
  where T: Deps + ?Sized,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    (&**self).iter_deps(f)
  }
}
unsafe impl<T> Deps for Arc<T>
  where T: Deps + ?Sized,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    (&**self).iter_deps(f)
  }
}
unsafe impl<T> Deps for Box<T>
  where T: Deps + ?Sized,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    (&**self).iter_deps(f)
  }
}
unsafe impl<T> Deps for PhantomData<T>
  where T: ?Sized,
{
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}
unsafe impl Deps for PhantomPinned {
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}
unsafe impl<T, const C: usize> Deps for [T; C]
  where T: Deps,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    for v in self.iter() {
      v.iter_deps(f)?;
    }
    Ok(())
  }
}
/// XXX ??
unsafe impl<T> Deps for RawPoolBox<T>
  where T: ?Sized,
{
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}
unsafe impl<T> Deps for LocallyAccessiblePoolBox<T>
  where T: ?Sized + Deps,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    (&**self).iter_deps(f)
  }
}
unsafe impl<T> Deps for LapBox<T>
  where T: ?Sized + Deps,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    (&**self).iter_deps(f)
  }
}
unsafe impl<T> Deps for LapVec<T>
  where T: Deps,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    (&**self).iter_deps(f)
  }
}
unsafe impl Deps for DeviceMultiQueue {
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), Error>)
    -> Result<(), Error>
  {
    Ok(())
  }
}
unsafe impl Deps for DeviceSingleQueue {
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), Error>)
    -> Result<(), Error>
  {
    Ok(())
  }
}
unsafe impl<T> Deps for Range<T>
  where T: Deps,
{
  #[inline(always)]
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    self.start.iter_deps(f)?;
    self.end.iter_deps(f)?;
    Ok(())
  }
}
unsafe impl<T> Deps for RangeFrom<T>
  where T: Deps,
{
  #[inline(always)]
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    self.start.iter_deps(f)?;
    Ok(())
  }
}
unsafe impl Deps for RangeFull {
  #[inline(always)]
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}
unsafe impl<T> Deps for RangeInclusive<T>
  where T: Deps,
{
  #[inline(always)]
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    self.start().iter_deps(f)?;
    self.end().iter_deps(f)?;
    Ok(())
  }
}
unsafe impl<T> Deps for RangeTo<T>
  where T: Deps,
{
  #[inline(always)]
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    self.end.iter_deps(f)?;
    Ok(())
  }
}
unsafe impl<T> Deps for RangeToInclusive<T>
  where T: Deps,
{
  #[inline(always)]
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    self.end.iter_deps(f)?;
    Ok(())
  }
}

/// Turns the inner completion into a dep.
pub struct CompletionDep<T>(T, Cell<bool>)
  where T: Completion;

impl<T> CompletionDep<T>
  where T: Completion,
{
  pub fn new(v: T) -> Self {
    CompletionDep(v, Cell::new(false))
  }

  fn host_wait(&self) {
    if is_host() && !self.1.get() {
      if let Some(host) = self.0.completion().as_host_consumable() {
        if let Err(code) = host.wait_for_zero(false) {
          panic!("got negative signal from signal: {}", code);
        }
      } else {
        let v = self.0.completion()
          .signal_ref()
          .load_scacquire();
        assert_eq!(v, 0, "can't directly wait on non-host-consumable signal");
      }

      self.1.set(true);
    }
  }
}

unsafe impl<T, S> Deps for CompletionDep<T>
  where T: Completion<CompletionSignal = S>,
        S: DeviceConsumable + Sized,
{
  #[inline(always)]
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    f(unsafe {
      std::mem::transmute(self.0.completion() as &dyn DeviceConsumable)
    })
  }
}

impl<T> Deref for CompletionDep<T>
  where T: Completion,
        T::CompletionSignal: DeviceConsumable,
{
  type Target = T;
  fn deref(&self) -> &T {
    self.host_wait();

    &self.0
  }
}
impl<T> DerefMut for CompletionDep<T>
  where T: Completion,
        T::CompletionSignal: DeviceConsumable,
{
  fn deref_mut(&mut self) -> &mut T {
    self.host_wait();

    &mut self.0
  }
}
impl<T> SignalHandle for CompletionDep<T>
  where T: Completion,
{
  fn signal_ref(&self) -> &SignalRef {
    self.0.completion().signal_ref()
  }

  fn as_host_consumable(&self) -> Option<&dyn HostConsumable> {
    self.0.completion().as_host_consumable()
  }
}
