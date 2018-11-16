use std::alloc::{Layout, };
use std::ffi::c_void;
use std::marker::{PhantomData, };
use std::mem::{size_of, transmute, align_of_val, size_of_val,
               ManuallyDrop, };
use std::ops::{Deref, DerefMut, Index, IndexMut, };
use std::ptr::{NonNull, };
use std::slice::{from_raw_parts, from_raw_parts_mut, SliceIndex, };
use std::sync::{Arc, atomic::AtomicUsize, };

use hsa_rt::ffi;
use hsa_rt::agent::Agent;
use hsa_rt::error::Error;
use hsa_rt::utils::set_data_ptr;

use {is_host, };

use nd;

// TODO use <*const T x 2> to compute the slice offsets.
// Don't use derefs in the Index(Mut) impls! This is because (I think) the
// index and deref functions get inline by the compiler, causing us to use
// the wrong ptr.

/// Similar to `&[T]`, but uses the correct pointer on both the
/// host and device.
#[derive(Copy, Clone)]
pub struct SliceRef<'a, T> {
  pub(super) _owner: PhantomData<&'a [T]>,
  pub(super) host: NonNull<T>,
  pub(super) agent: NonNull<T>,
  pub(super) len: usize,
}

impl<'a, T> SliceRef<'a, T> {
  fn ptr(&self) -> &NonNull<T> {
    if is_host() {
      &self.host
    } else {
      &self.agent
    }
  }

  pub fn slice<I>(self, index: I) -> Self
    where I: SliceIndex<[T], Output = [T]>,
  {
    // compute the offset w/ the host ref, then use that to offset
    // the agent ptr.
    let host_slice = unsafe {
      from_raw_parts(self.host.as_ptr() as *const T,
                     self.len)
    };
    let slice = index.index(host_slice);

    let ptr = host_slice.as_ptr() as usize;
    let slice_ptr = slice.as_ptr() as usize;

    let offset = slice_ptr - ptr;

    SliceRef {
      _owner: PhantomData,
      host: unsafe { NonNull::new_unchecked(slice.as_ptr() as *mut _) },
      agent: unsafe {
        let p = self.agent
          .as_ptr()
          .add(offset);
        NonNull::new_unchecked(p)
      },
      len: slice.len(),
    }
  }
}

impl<'a, T> Deref for SliceRef<'a, T> {
  type Target = [T];
  fn deref(&self) -> &[T] {
    unsafe {
      from_raw_parts(self.ptr().as_ptr() as *const T,
                     self.len)
    }
  }
}
impl<'a, T> AsRef<[T]> for SliceRef<'a, T> {
  fn as_ref(&self) -> &[T] {
    unsafe {
      from_raw_parts(self.ptr().as_ptr() as *const T,
                     self.len)
    }
  }
}
impl<'a, T> Index<usize> for SliceRef<'a, T> {
  type Output = T;
  fn index(&self, index: usize) -> &T {
    &self.as_ref()[index]
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
  pub(super) _owner: PhantomData<&'a mut [T]>,
  pub(super) host: NonNull<T>,
  pub(super) agent: NonNull<T>,
  pub(super) len: usize,
}

impl<'a, T> SliceMut<'a, T> {
  fn ptr(&self) -> &NonNull<T> {
    if is_host() {
      &self.host
    } else {
      &self.agent
    }
  }

  pub fn slice_ref<I>(&self, index: I) -> SliceRef<T>
    where I: SliceIndex<[T], Output = [T]>,
  {
    // compute the offset w/ the host ref, then use that to offset
    // the agent ptr.

    let host_slice = unsafe {
      from_raw_parts(self.host.as_ptr() as *const T,
                     self.len)
    };
    let slice = index.index(host_slice);

    let ptr = host_slice.as_ptr() as usize;
    let slice_ptr = slice.as_ptr() as usize;

    let offset = slice_ptr - ptr;

    SliceRef {
      _owner: PhantomData,
      host: unsafe { NonNull::new_unchecked(slice.as_ptr() as *mut _) },
      agent: unsafe {
        let p = self.agent
          .as_ptr()
          .add(offset);
        NonNull::new_unchecked(p)
      },
      len: slice.len(),
    }
  }

  pub fn slice<I>(self, index: I) -> Self
    where I: SliceIndex<[T], Output = [T]>,
  {
    // compute the offset w/ the host ref, then use that to offset
    // the agent ptr.

    let host_slice = unsafe {
      from_raw_parts(self.host.as_ptr() as *const T,
                     self.len)
    };
    let slice = index.index(host_slice);

    let ptr = host_slice.as_ptr() as usize;
    let slice_ptr = slice.as_ptr() as usize;

    let offset = slice_ptr - ptr;

    SliceMut {
      _owner: PhantomData,
      host: unsafe { NonNull::new_unchecked(slice.as_ptr() as *mut _) },
      agent: unsafe {
        let p = self.agent
          .as_ptr()
          .add(offset);
        NonNull::new_unchecked(p)
      },
      len: slice.len(),
    }
  }
}

impl<'a, T> Deref for SliceMut<'a, T> {
  type Target = [T];
  fn deref(&self) -> &[T] {
    unsafe {
      from_raw_parts(self.ptr().as_ptr() as *const T,
                     self.len)
    }
  }
}
impl<'a, T> AsRef<[T]> for SliceMut<'a, T> {
  fn as_ref(&self) -> &[T] {
    unsafe {
      from_raw_parts(self.ptr().as_ptr() as *const T,
                     self.len)
    }
  }
}
impl<'a, T> DerefMut for SliceMut<'a, T> {
  fn deref_mut(&mut self) -> &mut [T] {
    unsafe {
      from_raw_parts_mut(self.ptr().as_ptr(),
                         self.len)
    }
  }
}
impl<'a, T> AsMut<[T]> for SliceMut<'a, T> {
  fn as_mut(&mut self) -> &mut [T] {
    unsafe {
      from_raw_parts_mut(self.ptr().as_ptr(),
                         self.len)
    }
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
