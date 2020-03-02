//! Types similar in function to `&[T]` and `&mut [T]` but which
//! also hold a pointer local to a accelerator.
//!
//! These types automatically use the correct local pointer.
//!

use std::fmt;
use std::iter::IntoIterator;
use std::marker::{PhantomData, };
use std::mem::transmute;
use std::ops::{Deref, DerefMut, Index, IndexMut, };
use std::slice::{from_raw_parts, from_raw_parts_mut, SliceIndex,
                 Iter, IterMut, };

use crate::ptr::{SlicePtr, Ptr, NonNull, };
use crate::ref_::{AccelRefRaw, Mut, };

// TODO use <*const T x 2> to compute the slice offsets.

impl<T, I> Index<I> for AccelRefRaw<[T]>
  where T: Sized,
        I: SliceIndex<[T]>,
{
  type Output = AccelRefRaw<I::Output>;
  fn index(&self, index: I) -> &Self::Output {
    unsafe {
      transmute(Index::index(&**self, index))
    }
  }
}
impl<T, I> IndexMut<I> for AccelRefRaw<[T]>
  where T: Sized,
        I: SliceIndex<[T]>,
{
  fn index_mut(&mut self, index: I) -> &mut Self::Output {
    unsafe {
      transmute(IndexMut::index_mut(&mut **self, index))
    }
  }
}


/// Similar to `&[T]`, but uses the correct pointer on both the
/// host and device.
#[derive(Copy, Clone)]
pub struct SliceRef<'a, T> {
  pub(crate) _owner: PhantomData<&'a [T]>,
  pub(crate) ptr: SlicePtr<T>,
}

impl<'a, T> SliceRef<'a, T> {
  pub unsafe fn from_parts(ptr: Ptr<T>, len: usize) -> Self {
    host_assert_ne!(ptr.accel, ::std::ptr::null());
    SliceRef {
      _owner: PhantomData,
      ptr: SlicePtr {
        host: ptr.host,
        accel: ptr.accel,
        len,
      }
    }
  }
  pub fn as_ptr(&self) -> Ptr<T> { self.ptr.as_ptr() }
  pub fn as_local_ptr(&self) -> *const T { self.ptr.as_local_ptr() }

  pub fn into_slice_ptr(self) -> SlicePtr<T> { self.ptr }

  pub fn as_host_slice(&self) -> &'a [T] {
    unsafe {
      from_raw_parts(self.ptr.as_host_ptr() as *const _,
                     self.len())
    }
  }
  pub fn as_accel_slice(&self) -> &'a [T] {
    unsafe {
      from_raw_parts(self.ptr.as_accel_ptr() as *const _,
                     self.len())
    }
  }
  pub fn as_local_slice(&self) -> &'a [T] {
    unsafe {
      from_raw_parts(self.ptr.as_local_ptr() as *const _,
                     self.len())
    }
  }

  pub fn len(&self) -> usize { self.ptr.len() }

  pub fn slice<I>(self, index: I) -> Self
    where I: SliceIndex<[T], Output = [T]>,
  {
    SliceRef {
      _owner: PhantomData,
      ptr: self.ptr.slice(index),
    }
  }
}

impl<'a, T> Deref for SliceRef<'a, T> {
  type Target = [T];
  fn deref(&self) -> &[T] {
    self.as_local_slice()
  }
}
impl<'a, T> AsRef<[T]> for SliceRef<'a, T> {
  fn as_ref(&self) -> &[T] {
    self.as_local_slice()
  }
}

// Don't use derefs in the Index(Mut) impls! This is because (I think) the
// index and deref functions get inlined by the compiler, causing us to use
// the wrong ptr.
impl<'a, T> Index<usize> for SliceRef<'a, T> {
  type Output = T;
  fn index(&self, index: usize) -> &T {
    &self.as_ref()[index]
  }
}
impl<'a, T> fmt::Debug for SliceRef<'a, T>
  where T: fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    fmt::Debug::fmt(self.as_local_slice(), f)
  }
}
impl<'a, T> IntoIterator for SliceRef<'a, T> {
  type Item = &'a T;
  type IntoIter = Iter<'a, T>;
  fn into_iter(self) -> Iter<'a, T> {
    self.as_local_slice().into_iter()
  }
}


unsafe impl<'a, T> Send for SliceRef<'a, T>
  where T: Send,
{ }
unsafe impl<'a, T> Sync for SliceRef<'a, T>
  where T: Sync,
{ }

/// Similar to `&mut [T]`, but uses the correct pointer on both the
/// host and device.
pub struct SliceMut<'a, T> {
  pub(crate) _owner: PhantomData<&'a mut [T]>,
  pub(crate) ptr: SlicePtr<T>,
}

impl<'a, T> SliceMut<'a, T> {
  pub unsafe fn from_parts(ptr: Ptr<T>, len: usize) -> Self {
    host_assert_ne!(ptr.accel, ::std::ptr::null());
    SliceMut {
      _owner: PhantomData,
      ptr: SlicePtr {
        host: ptr.host,
        accel: ptr.accel,
        len,
      }
    }
  }
  pub fn as_ptr(&self) -> Ptr<T> { self.ptr.as_ptr() }
  pub fn as_mut_ptr(&mut self) -> Ptr<T> { self.ptr.as_ptr() }

  pub fn into_slice_ptr(self) -> SlicePtr<T> { self.ptr }
  pub fn as_slice_ptr(&self) -> SlicePtr<T> { self.ptr }

  pub fn as_host_slice(&self) -> &'a [T] {
    unsafe {
      from_raw_parts(self.ptr.as_host_ptr() as *const _,
                     self.len())
    }
  }
  pub fn as_accel_slice(&self) -> &'a [T] {
    unsafe {
      from_raw_parts(self.ptr.as_accel_ptr() as *const _,
                     self.len())
    }
  }
  pub fn as_local_slice(&self) -> &'a [T] {
    unsafe {
      from_raw_parts(self.ptr.as_local_ptr() as *const _,
                     self.len())
    }
  }

  pub fn as_host_mut_slice(&self) -> &'a mut [T] {
    unsafe {
      from_raw_parts_mut(self.ptr.as_host_ptr() as *mut _,
                         self.len())
    }
  }
  pub fn as_accel_mut_slice(&self) -> &'a mut [T] {
    unsafe {
      from_raw_parts_mut(self.ptr.as_accel_ptr() as *mut _,
                         self.len())
    }
  }
  pub fn as_local_mut_slice(&self) -> &'a mut [T] {
    unsafe {
      from_raw_parts_mut(self.ptr.as_local_ptr() as *mut _,
                         self.len())
    }
  }

  /// Create a temporary (in that self can be unborrowed, and used again)
  /// borrow
  pub fn into_ref<'b>(&'b self) -> SliceRef<'b, T>
    where 'a: 'b,
  {
    SliceRef {
      _owner: PhantomData,
      ptr: self.ptr.clone(),
    }
  }

  pub fn len(&self) -> usize { self.ptr.len() }

  pub fn split_at_mut(&mut self, mid: usize)
    -> (SliceMut<'a, T>, SliceMut<'a, T>)
  {
    let len = self.len();
    let ptr = self.as_ptr();

    debug_assert!(mid <= len);

    unsafe {
      let l = SlicePtr::from_parts(ptr, mid);
      let r = self.ptr.index_ptr(mid);
      let r = SlicePtr::from_parts(r, len - mid);

      (l.as_mut_slice(), r.as_mut_slice())
    }
  }
  pub fn into_split_at_mut(self, mid: usize)
    -> (SliceMut<'a, T>, SliceMut<'a, T>)
  {
    let len = self.len();
    let ptr = self.as_ptr();

    debug_assert!(mid <= len);

    unsafe {
      let l = SlicePtr::from_parts(ptr, mid);
      let r = self.ptr.index_ptr(mid);
      let r = SlicePtr::from_parts(r, len - mid);

      (l.as_mut_slice(), r.as_mut_slice())
    }
  }

  pub fn mut_<I>(self, index: I) -> Mut<'a, T>
    where I: SliceIndex<[T], Output = T>,
  {
    let ptr = self.ptr.index_ptr(index);
    Mut {
      _owner: PhantomData,
      ptr: unsafe { NonNull::new_unchecked(ptr) },
    }
  }

  pub fn into_slice_ref<I>(self, index: I) -> SliceRef<'a, T>
    where I: SliceIndex<[T], Output = [T]>,
  {
    SliceRef {
      _owner: PhantomData,
      ptr: self.ptr.slice(index),
    }
  }

  pub fn slice_ref<I>(&self, index: I) -> SliceRef<T>
    where I: SliceIndex<[T], Output = [T]>,
  {
    SliceRef {
      _owner: PhantomData,
      ptr: self.ptr.slice(index),
    }
  }

  pub fn slice<I>(&mut self, index: I) -> SliceMut<T>
    where I: SliceIndex<[T], Output = [T]>,
  {
    self.slice_mut(index)
  }
  pub fn slice_mut<I>(&mut self, index: I) -> SliceMut<T>
    where I: SliceIndex<[T], Output = [T]>,
  {
    SliceMut {
      _owner: PhantomData,
      ptr: self.ptr.slice(index),
    }
  }
  pub fn into_slice_mut<I>(self, index: I) -> SliceMut<'a, T>
    where I: SliceIndex<[T], Output = [T]>,
  {
    SliceMut {
      _owner: PhantomData,
      ptr: self.ptr.slice(index),
    }
  }
}
impl<'a, T> fmt::Debug for SliceMut<'a, T>
  where T: fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    fmt::Debug::fmt(self.as_local_slice(), f)
  }
}
impl<'a, T> Deref for SliceMut<'a, T> {
  type Target = [T];
  fn deref(&self) -> &[T] {
    self.as_local_slice()
  }
}
impl<'a, T> AsRef<[T]> for SliceMut<'a, T> {
  fn as_ref(&self) -> &[T] {
    self.as_local_slice()
  }
}
impl<'a, T> DerefMut for SliceMut<'a, T> {
  fn deref_mut(&mut self) -> &mut [T] {
    self.as_local_mut_slice()
  }
}
impl<'a, T> AsMut<[T]> for SliceMut<'a, T> {
  fn as_mut(&mut self) -> &mut [T] {
    self.as_local_mut_slice()
  }
}
impl<'a, T> Index<usize> for SliceMut<'a, T> {
  type Output = T;
  fn index(&self, index: usize) -> &T {
    &self.as_ref()[index]
  }
}
impl<'a, T> IndexMut<usize> for SliceMut<'a, T> {
  fn index_mut(&mut self, index: usize) -> &mut T {
    &mut self.as_mut()[index]
  }
}
impl<'a, T> IntoIterator for SliceMut<'a, T> {
  type Item = &'a mut T;
  type IntoIter = IterMut<'a, T>;
  fn into_iter(self) -> IterMut<'a, T> {
    self.as_local_mut_slice().into_iter()
  }
}
