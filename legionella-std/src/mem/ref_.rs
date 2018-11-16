use {is_host, };

use std::alloc::{Layout, };
use std::ffi::c_void;
use std::fmt;
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

use nd;

/// A type which when dereferenced will automatically use the correct pointer.
#[derive(Clone, Copy)]
pub struct Ref<'a, T>
  where T: ?Sized,
{
  pub(super) _owner: PhantomData<&'a T>,
  pub(super) host: NonNull<T>,
  pub(super) agent: NonNull<T>,
}

impl<'a, T> Ref<'a, T>
  where T: ?Sized,
{
  fn as_ptr(&self) -> &NonNull<T> {
    if is_host() {
      &self.host
    } else {
      &self.agent
    }
  }
}

impl<'a, T> AsRef<T> for Ref<'a, T> {
  fn as_ref(&self) -> &T {
    unsafe { self.as_ptr().as_ref() }
  }
}
impl<'a, T> Deref for Ref<'a, T> {
  type Target = T;
  fn deref(&self) -> &T {
    unsafe { self.as_ptr().as_ref() }
  }
}
impl<'a, T> fmt::Debug for Ref<'a, T>
  where T: fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self.as_ref())
  }
}
