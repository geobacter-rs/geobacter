
#![allow(deprecated)]

use std::alloc::Layout;
use std::cmp::{Ordering, };
use std::fmt;
use std::iter::*;
use std::mem::{size_of_val, forget, transmute_copy, };
use std::marker::Unsize;
use std::ops::*;
use std::ptr::{NonNull as StdNonNull, Unique,
               slice_from_raw_parts_mut, drop_in_place,
               write, };

use alloc_wg::alloc::{AllocRef, AllocInit};

use hsa_rt::error::Error as HsaError;
use hsa_rt::ext::amd::{MemoryPool, MemoryPoolAlloc, MemoryPoolPtr};

use crate::{HsaAmdGpuAccel, Error};
use crate::mem::{BoxPoolPtr, };

/// A box-esk type which represents an allocation in any memory pool.
///
/// # Safety
///
/// This box can include *non-visible* memory (meaning dereferencing could
/// result in a segfault), so this type is still pretty unsafe to use.
/// This type will deallocate the contained region upon drop, but it
/// will *not* run drop, as we don't know if this memory is locally
/// visible.
pub struct RawPoolBox<T>(Unique<T>, MemoryPool)
  where T: ?Sized;

impl<T> RawPoolBox<T>
  where T: ?Sized,
{
  pub unsafe fn new_uninit(pool: MemoryPool)
    -> Result<Self, HsaError>
    where T: Sized,
  {
    let layout = Layout::new::<T>();

    let pool_aligment = pool.alloc_alignment()?
      .unwrap_or_default();
    if pool_aligment < layout.align() {
      return Err(HsaError::IncompatibleArguments);
    }

    let bytes = layout.size();
    let ptr: StdNonNull<T> = pool.alloc_in_pool(bytes)?
      .as_ptr()
      .cast();
    let ptr = Unique::new_unchecked(ptr.as_ptr());
    Ok(RawPoolBox(ptr, pool))
  }
  pub fn as_ptr(&self) -> *mut T {
    self.0.as_ptr()
  }
  pub fn as_pool_ptr(&self) -> MemoryPoolPtr<T> {
    unsafe {
      let ptr = StdNonNull::new_unchecked(self.as_ptr());
      MemoryPoolPtr::from_ptr(self.1.clone(), ptr)
    }
  }

  pub fn pool(&self) -> &MemoryPool { &self.1 }

  pub unsafe fn as_ref(&self) -> &T {
    self.0.as_ref()
  }
  pub unsafe fn as_mut(&mut self) -> &mut T {
    self.0.as_mut()
  }

  fn size(&self) -> usize {
    unsafe {
      size_of_val(self.as_ref())
    }
  }
}
impl<T> RawPoolBox<[T]>
  where T: Sized,
{
  pub unsafe fn new_uninit_slice(mut pool: MemoryPoolAlloc, count: usize)
    -> Result<Self, Error>
  {
    let layout = Layout::new::<T>()
      .repeat_packed(count)
      .map_err(|_| HsaError::Overflow )?;

    let ptr = pool.alloc(layout, AllocInit::Uninitialized)
      .map_err(|_| Error::Alloc(layout) )?;
    let ptr = slice_from_raw_parts_mut(ptr.ptr.as_ptr() as *mut _, count);

    let ptr = Unique::new_unchecked(ptr);
    Ok(RawPoolBox(ptr, pool.pool()))
  }

  /// Safe due to the length being embedded inside a fat pointer.
  pub fn len(&self) -> usize {
    unsafe { self.as_ref().len() }
  }
}
impl<T> fmt::Debug for RawPoolBox<T>
  where T: ?Sized,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_tuple("RawPoolBox")
      .field(&self.0)
      .field(&self.1)
      .finish()
  }
}
impl<T> fmt::Pointer for RawPoolBox<T>
  where T: ?Sized,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Pointer::fmt(&self.0, f)
  }
}
impl<T> Drop for RawPoolBox<T>
  where T: ?Sized,
{
  fn drop(&mut self) {
    let ptr = self.as_ptr();
    if let Some(ptr) = StdNonNull::new(ptr) {
      if let Err(err) = unsafe { self.1.dealloc_from_pool(ptr) } {
        log::error!("ignored deallocation error: {:?}", err);
      }
    }
  }
}
impl<T, U> CoerceUnsized<RawPoolBox<U>> for RawPoolBox<T>
  where T: Unsize<U> + ?Sized,
        U: ?Sized,
{ }
impl<T> BoxPoolPtr for RawPoolBox<T>
  where T: ?Sized,
{
  #[doc(hidden)]
  unsafe fn pool_ptr(&self) -> Option<MemoryPoolPtr<[u8]>> {
    if self.size() == 0 { return None; }

    Some(self.as_pool_ptr().into_bytes())
  }
}

/// Deprecated, use `LapBox` instead.
#[deprecated]
pub struct LocallyAccessiblePoolBox<T>(pub(crate) RawPoolBox<T>)
  where T: ?Sized;

impl<T> LocallyAccessiblePoolBox<T>
  where T: ?Sized,
{
  pub unsafe fn from_raw_box_unchecked(rb: RawPoolBox<T>) -> Self {
    LocallyAccessiblePoolBox(rb)
  }

  pub fn as_ptr(&self) -> *mut T {
    self.0.as_ptr()
  }
  pub fn as_pool_ptr(&self) -> MemoryPoolPtr<T> {
    self.0.as_pool_ptr()
  }

  pub fn pool(&self) -> &MemoryPool { self.0.pool() }

  /// Overwrites the set of agents which will have this region mapped
  /// into their address space.
  /// No device is allowed to access this that is not in `accels` (the
  /// local processor is always allowed access)
  pub fn set_accessible(&mut self, accels: &[&HsaAmdGpuAccel])
    -> Result<(), HsaError>
  {
    let pool_ptr = self.as_pool_ptr();
    if accels.len() == 0 {
      pool_ptr.grant_agents_access(&[])
    } else if accels.len() == 1 {
      pool_ptr.grant_agent_access(accels[0].agent())
    } else {
      let agents: Vec<_> = accels.iter()
        .map(|a| a.agent().clone() )
        .collect();
      pool_ptr.grant_agents_access(&agents)
    }
  }
}

impl<T> LocallyAccessiblePoolBox<T>
  where T: Sized,
{
  pub unsafe fn overwrite(&mut self, v: T) {
    write(self.as_ptr(), v);
  }
}
impl<T> LocallyAccessiblePoolBox<[T]>
  where T: Sized,
{
  /// Don't need to run drop for `T` cause `Copy`.
  pub fn into_raw_box(self) -> RawPoolBox<[T]> {
    let b = unsafe {
      transmute_copy(&self.0)
    };
    forget(self);

    b
  }

  pub fn from_iter<I>(accel: &HsaAmdGpuAccel, iter: I)
    -> Result<Self, Error>
    where I: ExactSizeIterator<Item = T>,
  {
    let count = iter.len();
    let rb = unsafe {
      RawPoolBox::new_uninit_slice(accel.host_pool().clone(), count)?
    };
    let mut this = unsafe {
      Self::from_raw_box_unchecked(rb)
    };

    for (dst, v) in this.iter_mut().zip(iter) {
      unsafe { write(dst, v); }
    }

    Ok(this)
  }
}
impl<T> LocallyAccessiblePoolBox<T>
  where T: Sized + Copy,
{
  /// Don't need to run drop for `T` cause `Copy`.
  pub fn into_raw_box(self) -> RawPoolBox<T> {
    let b = unsafe {
      transmute_copy(&self.0)
    };
    forget(self);

    b
  }
}
impl<T, U> PartialEq<LocallyAccessiblePoolBox<U>> for LocallyAccessiblePoolBox<T>
  where T: PartialEq<U> + ?Sized,
        U: ?Sized,
{
  fn eq(&self, rhs: &LocallyAccessiblePoolBox<U>) -> bool {
    (&**self).eq(&**rhs)
  }
}
impl<T> Eq for LocallyAccessiblePoolBox<T>
  where T: Eq,
{ }
impl<T, U> PartialOrd<LocallyAccessiblePoolBox<U>> for LocallyAccessiblePoolBox<T>
  where T: PartialOrd<U> + ?Sized,
        U: ?Sized,
{
  fn partial_cmp(&self, rhs: &LocallyAccessiblePoolBox<U>) -> Option<Ordering> {
    (&**self).partial_cmp(&**rhs)
  }
}
impl<T> Ord for LocallyAccessiblePoolBox<T>
  where T: Ord,
{
  fn cmp(&self, rhs: &LocallyAccessiblePoolBox<T>) -> Ordering {
    (&**self).cmp(&**rhs)
  }
}

impl<T> Deref for LocallyAccessiblePoolBox<T>
  where T: ?Sized,
{
  type Target = T;
  fn deref(&self) -> &T {
    unsafe { self.0.as_ref() }
  }
}
impl<T> DerefMut for LocallyAccessiblePoolBox<T>
  where T: ?Sized,
{
  fn deref_mut(&mut self) -> &mut T {
    unsafe { self.0.as_mut() }
  }
}

impl<T> fmt::Debug for LocallyAccessiblePoolBox<T>
  where T: ?Sized,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_tuple("LocallyAccessiblePoolBox")
      .field(&(self.0).0)
      .field(&(self.0).1)
      .finish()
  }
}
impl<T> fmt::Pointer for LocallyAccessiblePoolBox<T>
  where T: ?Sized,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Pointer::fmt(&self.0, f)
  }
}
impl<T> Drop for LocallyAccessiblePoolBox<T>
  where T: ?Sized,
{
  fn drop(&mut self) {
    // run drop:
    unsafe { drop_in_place(self.as_ptr()) }
  }
}
impl<T, U> CoerceUnsized<LocallyAccessiblePoolBox<U>> for LocallyAccessiblePoolBox<T>
  where T: Unsize<U> + ?Sized,
        U: ?Sized,
{ }

impl<T> BoxPoolPtr for LocallyAccessiblePoolBox<T>
  where T: ?Sized,
{
  #[doc(hidden)]
  unsafe fn pool_ptr(&self) -> Option<MemoryPoolPtr<[u8]>> {
    self.0.pool_ptr()
  }
}
