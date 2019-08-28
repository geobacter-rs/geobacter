
use std::alloc::Layout;
use std::convert::TryFrom;
use std::fmt;
use std::intrinsics::type_name;
use std::mem::{size_of_val, forget, transmute_copy, };
use std::marker::Unsize;
use std::ops::*;
use std::ptr::{NonNull as StdNonNull, Unique,
               slice_from_raw_parts_mut, drop_in_place,
               write, };
use std::slice::SliceIndex;
use std::sync::Arc;

use std::result::Result; // CLion.

use hsa_core::ptr::{SlicePtr, Ptr};
use hsa_core::slice::{SliceRef, SliceMut};

use hsa_rt::error::Error as HsaError;
use hsa_rt::ext::amd::{lock_memory, unlock_memory, MemoryPool, MemoryPoolPtr};

use log::{error, };

use crate::{HsaAmdGpuAccel, };
use crate::async_copy::{CopyDataObject, SourceDataObject, DestDataObject};

pub struct BoxSlice<T>
  where T: Sized,
{
  ptr: SlicePtr<T>,
}
impl<T> BoxSlice<T>
  where T: Sized,
{
  pub fn lock_to_accels(b: Box<[T]>, accels: &[&Arc<HsaAmdGpuAccel>])
    -> Result<Self, hsa_rt::error::Error>
  {
    let count = b.len();
    let host = Box::into_raw(b) as *mut T;
    let host = unsafe { StdNonNull::new_unchecked(host) };

    let mut agents = Vec::new();
    if accels.len() != 0 {
      let accels = accels.iter()
        .map(|a| a.agent().clone() );
      agents.extend(accels)
    }

    let accel = lock_memory(host, count, &agents)?;
    let ptr = Ptr::from_mut(host.as_ptr(),
                            accel.as_ptr());
    let ptr = SlicePtr::from_parts(ptr, count);
    Ok(BoxSlice { ptr, })
  }
  pub fn slice<I>(&self, index: I) -> SliceRef<T>
    where I: SliceIndex<[T], Output = [T]>,
  {
    self.as_slice().slice(index)
  }
  pub fn slice_mut<I>(&mut self, index: I) -> SliceMut<T>
    where I: SliceIndex<[T], Output = [T]>,
  {
    self.as_mut_slice().into_slice_mut(index)
  }
  pub fn as_slice(&self) -> SliceRef<T> {
    unsafe { self.ptr.as_slice() }
  }
  pub fn as_mut_slice(&mut self) -> SliceMut<T> {
    unsafe { self.ptr.as_mut_slice() }
  }

  pub fn len(&self) -> usize { self.ptr.len() }

  /// Clone the host box and then lock it.
  pub fn try_clone(&self, accels: &[&Arc<HsaAmdGpuAccel>])
    -> Result<Self, hsa_rt::error::Error>
    where T: Clone,
  {
    let b: Box<[T]> = unsafe {
      Box::from_raw(self.ptr.as_host_slice() as *mut _)
    };

    let new_b = b.clone();

    // don't drop our original box:
    ::std::mem::forget(b);

    Self::lock_to_accels(new_b, accels)
  }
  pub fn ptr_eq(&self, rhs: &Self) -> bool {
    self.ptr.as_local_ptr() == rhs.ptr.as_local_ptr()
  }
}


/// Locks the memory globally
impl<T> TryFrom<Box<[T]>> for BoxSlice<T>
  where T: Sized,
{
  type Error = hsa_rt::error::Error;
  fn try_from(v: Box<[T]>) -> Result<Self, Self::Error> {
    Self::lock_to_accels(v, &[])
  }
}

impl<T> Clone for BoxSlice<T>
  where T: Clone + Sized,
{
  fn clone(&self) -> Self {
    self.try_clone(&[])
      .expect("clone failed!")
  }
}
impl<T> fmt::Debug for BoxSlice<T>
  where T: fmt::Debug + Sized,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    fmt::Debug::fmt(&self.as_slice(), f)
  }
}
impl<T> fmt::Pointer for BoxSlice<T>
  where T: Sized,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "BoxSlice<{}>({:p})", type_name::<T>(),
           &self.ptr)
  }
}

/// XXX no check is done to make sure the GPU is finished with this
/// box
impl<T> Drop for BoxSlice<T>
  where T: Sized,
{
  fn drop(&mut self) {
    // unlock the memory:
    let ptr = self.ptr.as_host_ptr() as *mut T;
    let r = unsafe {
      unlock_memory(StdNonNull::new_unchecked(ptr),
                    self.len())
    };
    if let Err(err) = r {
      error!("failed to unlock host memory: {:?}", err);
    }

    // now run the host drop:
    unsafe { Box::from_raw(self.ptr.as_host_slice() as *mut [T]) };
  }
}

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
  pub unsafe fn new_uninit_slice(pool: MemoryPool, count: usize)
    -> Result<Self, HsaError>
  {
    let layout = Layout::new::<T>()
      .repeat_packed(count)
      .map_err(|_| HsaError::Overflow )?;

    let pool_aligment = pool.alloc_alignment()?
      .unwrap_or_default();
    if pool_aligment < layout.align() {
      return Err(HsaError::IncompatibleArguments);
    }

    let bytes = layout.size();
    let ptr: StdNonNull<T> = pool.alloc_in_pool(bytes)?
      .as_ptr()
      .cast();
    let ptr = slice_from_raw_parts_mut(ptr.as_ptr(), count);
    let ptr = Unique::new_unchecked(ptr);
    Ok(RawPoolBox(ptr, pool))
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
impl<T> CopyDataObject<T> for RawPoolBox<T>
  where T: ?Sized + Unpin,
{
  #[doc(hidden)]
  unsafe fn pool_copy_region(&self) -> Option<MemoryPoolPtr<[u8]>> {
    if self.size() == 0 { return None; }

    Some(self.as_pool_ptr().into_bytes())
  }
}
impl<T> SourceDataObject<T> for RawPoolBox<T>
  where T: ?Sized + Unpin,
{
  fn src_pool(&self) -> MemoryPool { self.pool().clone() }
}
impl<T> DestDataObject<T> for RawPoolBox<T>
  where T: ?Sized + Unpin,
{
  fn dst_pool(&self) -> MemoryPool { self.pool().clone() }
}

/// A box-esk type allocated from a CPU visible memory pool.
/// Locality is defined as memory accessible to processor which
/// is running the code.
///
/// This type is safe to dereference.
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

impl<T> CopyDataObject<T> for LocallyAccessiblePoolBox<T>
  where T: ?Sized + Unpin,
{
  #[doc(hidden)]
  unsafe fn pool_copy_region(&self) -> Option<MemoryPoolPtr<[u8]>> {
    self.0.pool_copy_region()
  }
}
impl<T> SourceDataObject<T> for LocallyAccessiblePoolBox<T>
  where T: ?Sized + Unpin,
{
  fn src_pool(&self) -> MemoryPool { self.pool().clone() }
}
impl<T> DestDataObject<T> for LocallyAccessiblePoolBox<T>
  where T: ?Sized + Unpin,
{
  fn dst_pool(&self) -> MemoryPool { self.pool().clone() }
}

