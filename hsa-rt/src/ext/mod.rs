
use std::ffi::c_void;
use std::mem::{size_of, transmute, };
use std::ops::{Deref, DerefMut, };
use std::slice::{from_raw_parts, from_raw_parts_mut, };

use ffi;
use agent::Agent;
use error::Error;

use nd;

pub mod amd;

pub trait AsPtrValue {
  type Target;
  fn byte_len(&self) -> usize;
  fn as_ptr_value(&self) -> *const Self::Target;

  fn as_u8_slice(&self) -> &[u8] {
    let len = self.byte_len();
    let ptr = self.as_ptr_value();
    unsafe {
      from_raw_parts(ptr as *const u8, len)
    }
  }
}
impl<T> AsPtrValue for Vec<T> {
  type Target = T;
  fn byte_len(&self) -> usize {
    size_of::<T>() * self.len()
  }
  fn as_ptr_value(&self) -> *const T {
    self.as_ptr()
  }
}
impl<T> AsPtrValue for Box<T> {
  type Target = T;
  fn byte_len(&self) -> usize {
    size_of::<T>()
  }
  fn as_ptr_value(&self) -> *const T {
    unsafe {
      ::std::mem::transmute_copy(self)
    }
  }
}
impl<T, D> AsPtrValue for nd::ArrayBase<T, D>
  where T: nd::DataOwned,
        D: nd::Dimension,
{
  type Target = T::Elem;
  fn byte_len(&self) -> usize {
    self.as_slice_memory_order()
      .expect("owned nd::ArrayBase isn't contiguous")
      .len() * size_of::<T::Elem>()
  }
  fn as_ptr_value(&self) -> *const T::Elem { self.as_ptr() }
}

enum HostPtr<T>
  where T: AsPtrValue,
{
  Obj(T),
  Ptr(*const T::Target),
}
impl<T> HostPtr<T>
  where T: AsPtrValue,
{
  fn host_ptr(&self) -> *const T::Target {
    match self {
      &HostPtr::Obj(ref obj) => obj.as_ptr_value(),
      &HostPtr::Ptr(ptr) => ptr,
    }
  }
  fn host_obj_ref(&self) -> &T {
    match self {
      &HostPtr::Obj(ref obj) => obj,
      _ => unreachable!(),
    }
  }
  fn host_obj_mut(&mut self) -> &mut T {
    match self {
      &mut HostPtr::Obj(ref mut obj) => obj,
      _ => unreachable!(),
    }
  }
  fn take_obj(&mut self) -> T {
    let mut new_val = HostPtr::Ptr(self.host_ptr());
    ::std::mem::swap(&mut new_val, self);
    match new_val {
      HostPtr::Obj(obj) => obj,
      _ => panic!("don't have object anymore"),
    }
  }
}

pub struct HostLockedAgentPtr<T>
  where T: AsPtrValue,
{
  // always `HostPtr::Obj`, except during `Drop`.
  host: HostPtr<T>,
  agent_ptr: *const T::Target,
}

impl<T> HostLockedAgentPtr<T>
  where T: AsPtrValue,
{
  fn host_ptr(&self) -> *const T::Target {
    self.host.host_ptr()
  }
  pub fn agent_ptr(&self) -> *const T::Target { self.agent_ptr }
  fn agent_mut_ptr(&self) -> *mut T::Target {
    self.agent_ptr as *mut T::Target
  }

  pub fn unlock(mut self) -> T { self.host.take_obj() }
}
impl<T> HostLockedAgentPtr<Vec<T>> {
  pub fn as_agent_ref(&self) -> &[T] {
    unsafe {
      from_raw_parts(self.agent_ptr(),
                     self.len())
    }
  }
  pub fn as_agent_mut(&mut self) -> &mut [T] {
    unsafe {
      from_raw_parts_mut(self.agent_mut_ptr(),
                         self.len())
    }
  }
}
impl<T, D> HostLockedAgentPtr<nd::ArrayBase<T, D>>
  where T: nd::DataOwned,
        D: nd::Dimension,
{
  pub fn agent_view(&self) -> nd::ArrayView<T::Elem, D> {
    unsafe {
      nd::ArrayView::from_shape_ptr(self.raw_dim(),
                                    self.agent_ptr())
    }
  }
  pub fn agent_view_mut(&mut self) -> nd::ArrayViewMut<T::Elem, D> {
    unsafe {
      nd::ArrayViewMut::from_shape_ptr(self.raw_dim(),
                                       self.agent_mut_ptr())
    }
  }
}
impl<T> Drop for HostLockedAgentPtr<T>
  where T: AsPtrValue,
{
  fn drop(&mut self) {
    let ptr = self.host_ptr();
    unsafe {
      ffi::hsa_amd_memory_unlock(ptr as *mut c_void);
      // ignore result.
    }
  }
}
impl<T> Deref for HostLockedAgentPtr<T>
  where T: AsPtrValue,
{
  type Target = T;
  fn deref(&self) -> &T { self.host.host_obj_ref() }
}
impl<T> DerefMut for HostLockedAgentPtr<T>
  where T: AsPtrValue,
{
  fn deref_mut(&mut self) -> &mut T { self.host.host_obj_mut() }
}

/// Locks `self` in host memory, and gives access to the specified agents.
/// If `agents` as no elements, access will be given to everyone.
pub trait HostLockedAgentMemory: Sized + AsPtrValue {
  fn lock_memory<'a>(self, agents: impl Iterator<Item = &'a Agent>)
    -> Result<HostLockedAgentPtr<Self>, Error>
  {
    let mut agents: Vec<_> = agents
      .map(|agent| agent.0 )
      .collect();

    let mut agent_ptr = 0 as *mut Self::Target;
    {
      let bytes = self.as_u8_slice();
      check_err!(ffi::hsa_amd_memory_lock(bytes.as_ptr() as *mut c_void,
                                          bytes.len(),
                                          agents.as_mut_ptr(),
                                          agents.len() as _,
                                          transmute(&mut agent_ptr)))?;
      assert_ne!(agent_ptr as usize, 0);
    }

    Ok(HostLockedAgentPtr {
      host: HostPtr::Obj(self),
      agent_ptr,
    })
  }
}
impl<T> HostLockedAgentMemory for Vec<T> { }
impl<T> HostLockedAgentMemory for Box<T> { }
impl<T, D> HostLockedAgentMemory for nd::ArrayBase<T, D>
  where T: nd::DataOwned,
        D: nd::Dimension,
{ }
