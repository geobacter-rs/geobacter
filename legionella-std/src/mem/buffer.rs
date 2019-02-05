
use std::marker::{PhantomData, };
use std::ops::{RangeBounds, Bound,  Deref, DerefMut, };

use vk_help::{__legionella_compute_descriptor_set_binding, };
use vk::buffer::{TypedBufferAccess, };
use vk::descriptor::descriptor_set::{PersistentDescriptorSetError,
                                     PersistentDescriptorSetBuf,
                                     PersistentDescriptorSetBuilder,
                                     PersistentDescriptorSetBuilderArray,};
use vk::descriptor::PipelineLayoutAbstract;

use crate::{is_host, };


/// Do not use this type directly, we provide macros (attributes, rather)
/// which expand to the correct usage.
///
/// A non-arrayed uniform of type `T`. `Fm` is a marker function which is
/// used to communicate descriptor set bindings with the compiler side.
/// It is never called.
///
/// Note: T must satisfy `Copy + 'static` in addition to `Sized`. They are
/// omitted because `const fn`s can't have any other bounds other than
/// `Sized` currently.
#[legionella(lang_item = "Uniform")]
pub struct Uniform<T>
  where T: Sized,
{
  /// Note: not a `&'static T` so that we don't *require* a `'static`
  /// bound.
  data: *const T,
  set_binding: &'static (u32, u32),
}

/// Do not use this type directly, we provide macros (attributes, rather)
/// which expand to the correct usage.
///
/// An arrayed uniform of type `T`. `Fm` is a marker function which is
/// used to communicate descriptor set bindings with the compiler side.
/// It is never called. It *MUST* be unique for each global static (as
/// in, no two global statics may have the same marker function).
///
/// This type must be used as a global or static. It's length is given at
/// construction and is immutable.
///
/// Note: T must satisfy `Copy + 'static` in addition to `Sized`. They are
/// omitted because `const fn`s can't have any other bounds other than
/// `Sized` currently.
#[legionella(lang_item = "UniformArray")]
pub struct UniformArray<T>
  where T: Sized,
{
  start: *const T,
  len: usize,
  set_binding: &'static (u32, u32),
}

pub unsafe trait DescriptorSetBinding {
  fn set_binding(&self) -> (usize, usize);
}

impl<T> Copy for Uniform<T>
  where T: Sized + Copy + 'static,
{ }
impl<T> Clone for Uniform<T>
  where T: Sized + Copy + 'static,
{
  fn clone(&self) -> Self { *self }
}
impl<T> Deref for Uniform<T>
  where T: Sized + Copy + 'static,
{
  type Target = T;
  fn deref(&self) -> &T {
    assert!(!is_host(), "do not deref Uniform<T> types on the host!");
    unsafe { self.data.as_ref().unwrap() }
  }
}

unsafe impl<T> DescriptorSetBinding for Uniform<T>
  where T: Sized + Copy + 'static,
{
  fn set_binding(&self) -> (usize, usize) {
    (self.set_binding.0 as usize,
     self.set_binding.1 as usize)
  }
}
unsafe impl<T> DescriptorSetBinding for UniformArray<T>
  where T: Sized + Copy + 'static,
{
  fn set_binding(&self) -> (usize, usize) {
    (self.set_binding.0 as usize,
     self.set_binding.1 as usize)
  }
}

impl<T> Uniform<T>
  where T: Sized,
{
  pub const fn new<Km>(data: &'static T, _marker: &Km) -> Self {
    Uniform {
      data,
      set_binding: unsafe {
        __legionella_compute_descriptor_set_binding::<Km>()
      },
    }
  }
}
impl<T> UniformArray<T>
  where T: Sized,
{
  pub const fn new<Km>(data: &'static [T], _marker: &Km) -> Self {
    UniformArray {
      start: data.as_ptr(),
      len: data.len(),
      set_binding: unsafe {
        __legionella_compute_descriptor_set_binding::<Km>()
      },
    }
  }
}

impl<T> Uniform<T>
  where T: Sized + Copy + 'static,
{
  pub fn add_binding<Pl, B, R>(&self, builder: PersistentDescriptorSetBuilder<Pl, R>, data: B)
    -> Result<PersistentDescriptorSetBuilder<Pl, (R, PersistentDescriptorSetBuf<B>)>,
              PersistentDescriptorSetError>
    where B: TypedBufferAccess<Content = T>,
          Pl: PipelineLayoutAbstract,
  {
    let (_, binding) = self.set_binding();
    builder.enter_array_binding(binding)?
      .add_buffer(data)?
      .leave_array()
  }
}

impl<T> UniformArray<T>
  where T: Sized + Copy + 'static,
{
  pub fn len(&self) -> usize { self.len }

  pub fn enter_array<Pl, R>(&self, builder: PersistentDescriptorSetBuilder<Pl, R>)
    -> Result<PersistentDescriptorSetBuilderArray<Pl, R>,
              PersistentDescriptorSetError>
    where Pl: PipelineLayoutAbstract,
  {
    let (_, binding) = self.set_binding();
    builder.enter_array_binding(binding)
  }
}

/// Do not use this type directly, we provide macros (attributes, rather)
/// which expand to the correct usage.
///
/// An arrayed uniform of type `T`. `Fm` is a marker function which is
/// used to communicate descriptor set bindings with the compiler side.
/// It is never called.
///
/// This type must be used as a global or mut static. This type is similar to
/// `Uniform`, if possibly slower, but allows shader or kernel mutation.
/// Because any part of the buffer data can be modified by any invocation,
/// (not to mention `static mut` requires `unsafe { }` anyway) this type
/// requires unsafe blocks to use.
///
/// Note: T must satisfy `Copy + 'static` in addition to `Sized`. They are
/// omitted because `const fn`s can't have any other bounds other than
/// `Sized` currently.
///#[legionella(lang_item = "Buffer")]
pub struct BufferBinding<T>
  where T: Sized,
{
  _data: PhantomData<*mut T>,
  set_binding: &'static (u32, u32),
}

impl<T> BufferBinding<T>
  where T: Sized,
{
  pub const fn new<Km>(_marker: &Km) -> Self {
    BufferBinding {
      _data: PhantomData,
      set_binding: unsafe {
        __legionella_compute_descriptor_set_binding::<Km>()
      },
    }
  }
}

impl<T> BufferBinding<T>
  where T: Sized + Copy + Sync + 'static,
{
  pub fn add_binding<Pl, B, R>(&self,
                               builder: PersistentDescriptorSetBuilder<Pl, R>,
                               data: B)
    -> Result<PersistentDescriptorSetBuilder<Pl, (R, PersistentDescriptorSetBuf<B>)>,
              PersistentDescriptorSetError>
    where B: TypedBufferAccess<Content = T>,
          Pl: PipelineLayoutAbstract,
  {
    let (_, binding) = self.set_binding();
    builder.enter_array_binding(binding)?
      .add_buffer(data)?
      .leave_array()
  }
}
unsafe impl<T> DescriptorSetBinding for BufferBinding<T>
  where T: Sized + Copy + Sync + 'static,
{
  fn set_binding(&self) -> (usize, usize) {
    (self.set_binding.0 as usize,
     self.set_binding.1 as usize)
  }
}
