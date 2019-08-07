
//! Helper types which store a pointer for the host and a pointer for the accelerator.
//!

use std::cmp;
use std::fmt;
use std::hash::*;
use std::marker::{PhantomData, };
use std::mem::transmute;
use std::ops::*;
use std::ptr::{NonNull as StdNonNull, slice_from_raw_parts,
               slice_from_raw_parts_mut, Unique };
use std::slice::{from_raw_parts, SliceIndex, };

use crate::platform::is_host;
use crate::ref_::{Ref, Mut, AccelRefRaw, };
use crate::slice::{SliceRef, SliceMut};

pub trait PtrTy
  where Self: fmt::Debug + fmt::Pointer,
{
  type ElementTy: ?Sized;
}
pub trait PtrRefTy<'a> {
  type RefTy: Sized;

  unsafe fn as_ref_ty(&'a self) -> Option<Self::RefTy>;
}
pub trait PtrRefTx<'a, T>
  where T: ?Sized,
{
  type TxRefTy: ?Sized;
}
impl<T> PtrTy for *const T
  where T: ?Sized,
{
  type ElementTy = T;
}
impl<'a, T> PtrRefTy<'a> for *const T
  where T: ?Sized + 'a,
{
  type RefTy = &'a T;
  unsafe fn as_ref_ty(&'a self) -> Option<Self::RefTy> {
    self.as_ref()
  }
}
impl<'a, T, U> PtrRefTx<'a, U> for *const T
  where T: ?Sized + 'a,
        U: ?Sized + 'a,
{
  type TxRefTy = &'a U;
}
impl<T> PtrTy for *mut T
  where T: ?Sized,
{
  type ElementTy = T;
}
impl<'a, T> PtrRefTy<'a> for *mut T
  where T: ?Sized + 'a,
{
  type RefTy = &'a mut T;
  unsafe fn as_ref_ty(&'a self) -> Option<Self::RefTy> {
    self.as_mut()
  }
}
impl<T> PtrTy for StdNonNull<T>
  where T: ?Sized,
{
  type ElementTy = T;
}
impl<T> PtrTy for Unique<T>
  where T: ?Sized,
{
  type ElementTy = T;
}

#[derive(Debug, Eq, PartialEq)]
// #[repr(simd)] XXX I dislike how Rust implemented simd-y types.
pub struct Ptr<T>
  where T: ?Sized,
{
  pub(crate) host: *const T,
  pub(crate) accel: *const T,
}
impl<T> Ptr<T>
  where T: ?Sized,
{
  pub fn from(host: *const T, accel: *const T) -> Self {
    Ptr {
      host,
      accel,
    }
  }
  pub fn from_mut(host: *mut T, accel: *mut T) -> Self {
    Ptr {
      host,
      accel,
    }
  }

  pub fn as_host_ptr(&self) -> *const T { self.host }
  pub fn as_accel_ptr(&self) -> *const T { self.accel }
  pub fn as_local_ptr(&self) -> *const T {
    if is_host() {
      self.host
    } else {
      self.accel
    }
  }
  pub fn as_ptr(&self) -> *const T {
    self.as_local_ptr()
  }

  pub unsafe fn as_host_ref<'a>(self) -> Option<&'a T> {
    self.host.as_ref()
  }
  pub unsafe fn as_accel_ref<'a>(self) -> Option<&'a T> {
    self.accel.as_ref()
  }
  pub unsafe fn as_local_ref<'a>(self) -> Option<&'a T> {
    self.as_local_ptr().as_ref()
  }

  pub unsafe fn as_host_mut<'a>(self) -> Option<&'a mut T> {
    (self.host as *mut T).as_mut()
  }
  pub unsafe fn as_accel_mut<'a>(self) -> Option<&'a mut T> {
    (self.accel as *mut T).as_mut()
  }
  pub unsafe fn as_local_mut<'a>(self) -> Option<&'a mut T> {
    (self.as_local_ptr() as *mut T).as_mut()
  }

  pub unsafe fn write(self, v: T)
    where T: Sized,
  {
    match self.as_local_mut() {
      Some(r) => {
        *r = v;
      },
      None => {
        ::std::hint::unreachable_unchecked();
      },
    }
  }

  pub unsafe fn offset(self, count: isize) -> Self
    where T: Sized,
  {
    // Hopefully LLVM sees the common expression...
    Ptr {
      host: self.host.offset(count),
      accel: self.accel.offset(count),
    }
  }
  pub unsafe fn add(self, count: usize) -> Self
    where T: Sized,
  {
    // Hopefully LLVM sees the common expression...
    Ptr {
      host: self.host.add(count),
      accel: self.accel.add(count),
    }
  }
}
impl<T> Copy for Ptr<T>
  where T: ?Sized,
{ }
impl<T> Clone for Ptr<T>
  where T: ?Sized,
{
  fn clone(&self) -> Self { *self }
}
impl<T> fmt::Pointer for Ptr<T>
  where T: ?Sized,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if f.alternate() {
      write!(f, r#"Ptr {{
  host:  {:p},
  accel: {:p},
}}"#,
             self.host, self.accel)
    } else {
      write!(f, r#"Ptr {{ host: {:p}, accel: {:p}, }}"#,
             self.host, self.accel)
    }
  }
}

/// A newtype pointer, representing pointers which are local to
/// an accelerator. Such pointers are often only dereferencable
/// on the specific accelerator it is associated with.
#[repr(transparent)]
pub struct AccelPtr<T>(T)
  where T: PtrTy;
pub type AccelConstPtr<T> = AccelPtr<*const T>;
pub type AccelMutPtr<T>   = AccelPtr<*mut T>;
pub type AccelNonNull<T>  = AccelPtr<StdNonNull<T>>;
pub type AccelUnique<T>   = AccelPtr<Unique<T>>;

impl<T> AccelConstPtr<T>
  where T: ?Sized,
{
  pub fn is_null(self) -> bool { self.0.is_null() }
  pub unsafe fn as_ref<'a>(self) -> Option<&'a T> {
    self.0.as_ref()
  }

  pub unsafe fn into_slice(self, count: usize) -> AccelConstPtr<[T]>
    where T: Sized,
  {
    AccelPtr(slice_from_raw_parts(self.0, count))
  }

  pub unsafe fn offset(self, count: isize) -> Self
    where T: Sized,
  {
    AccelPtr(self.0.offset(count))
  }
  pub unsafe fn add(self, count: usize) -> Self
    where T: Sized,
  {
    AccelPtr(self.0.add(count))
  }
}
impl<T> AccelMutPtr<T>
  where T: ?Sized,
{
  pub fn is_null(self) -> bool { self.0.is_null() }
  pub unsafe fn as_ref<'a>(self) -> Option<&'a T> {
    self.0.as_ref()
  }
  pub unsafe fn as_mut<'a>(self) -> Option<&'a mut T> {
    self.0.as_mut()
  }
  pub unsafe fn into_slice(self, count: usize) -> AccelMutPtr<[T]>
    where T: Sized,
  {
    AccelPtr(slice_from_raw_parts_mut(self.0, count))
  }
  pub unsafe fn offset(self, count: isize) -> Self
    where T: Sized,
  {
    AccelPtr(self.0.offset(count))
  }
  pub unsafe fn add(self, count: usize) -> Self
    where T: Sized,
  {
    AccelPtr(self.0.add(count))
  }
}
impl<T> AccelNonNull<T>
  where T: ?Sized,
{
  pub fn new(ptr: *mut T) -> Option<Self> {
    StdNonNull::new(ptr).map(AccelPtr)
  }
  pub const unsafe fn new_unchecked(ptr: *mut T) -> Self {
    AccelPtr(StdNonNull::new_unchecked(ptr))
  }
  pub fn is_null(self) -> bool { false }
  pub const fn as_ptr(self) -> *mut T { self.0.as_ptr() }
  pub unsafe fn as_ref<'a>(self) -> &'a AccelRefRaw<T> {
    transmute(self.0.as_ref())
  }
  pub unsafe fn as_mut<'a>(mut self) -> &'a mut AccelRefRaw<T> {
    transmute(self.0.as_mut())
  }
  pub const fn cast<U>(self) -> AccelNonNull<U>
    where U: PtrTy,
  {
    AccelPtr(self.0.cast())
  }
}
impl<T> AccelUnique<T>
  where T: Sized,
{
  pub const fn empty() -> Self {
    AccelPtr(Unique::empty())
  }
}
impl<T> AccelUnique<T>
  where T: ?Sized,
{
  pub fn new(ptr: *mut T) -> Option<Self> {
    Unique::new(ptr).map(AccelPtr)
  }
  pub const unsafe fn new_unchecked(ptr: *mut T) -> Self {
    AccelPtr(Unique::new_unchecked(ptr))
  }
  pub fn is_null(self) -> bool { false }
  pub const fn as_ptr(self) -> *mut T { self.0.as_ptr() }
  pub unsafe fn as_ref(&self) -> &AccelRefRaw<T> {
    transmute(self.0.as_ref())
  }
  pub unsafe fn as_mut(&mut self) -> &mut AccelRefRaw<T> {
    transmute(self.0.as_mut())
  }
}

impl<T> From<Unique<T>> for AccelUnique<T>
  where T: ?Sized,
{
  fn from(v: Unique<T>) -> Self {
    AccelPtr(v)
  }
}

impl<T> AccelPtr<T>
  where T: PtrTy,
{
  pub unsafe fn as_ref_ty<'a>(self) -> Option<T::RefTy>
    where T: PtrRefTy<'a> + 'a,
  {
    // force the lifetime:
    let this: &'a Self = transmute(&self);
    this.0.as_ref_ty()
  }
}

impl<T> Copy for AccelPtr<T>
  where T: PtrTy + Copy,
{ }
impl<T> Clone for AccelPtr<T>
  where T: PtrTy + Copy,
{
  fn clone(&self) -> Self { *self }
}
impl<T> fmt::Debug for AccelPtr<T>
  where T: PtrTy,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Debug::fmt(&self.0, f)
  }
}
impl<T> Eq for AccelPtr<T>
  where T: PtrTy + Eq,
{ }
impl<T> PartialEq for AccelPtr<T>
  where T: PtrTy + PartialEq,
{
  fn eq(&self, rhs: &AccelPtr<T>) -> bool { self.0 == rhs.0 }
}
impl<T> Ord for AccelPtr<T>
  where T: PtrTy + Ord,
{
  fn cmp(&self, rhs: &Self) -> cmp::Ordering {
    self.0.cmp(&rhs.0)
  }
}
impl<T> PartialOrd for AccelPtr<T>
  where T: PtrTy + PartialOrd,
{
  fn partial_cmp(&self, rhs: &Self) -> Option<cmp::Ordering> {
    self.0.partial_cmp(&rhs.0)
  }
}
impl<T> fmt::Pointer for AccelPtr<T>
  where T: PtrTy,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Pointer::fmt(&self.0, f)
  }
}
impl<T> Hash for AccelPtr<T>
  where T: PtrTy + Hash,
{
  fn hash<H>(&self, state: &mut H)
    where H: Hasher,
  {
    self.0.hash(state)
  }
}
impl<T, U> CoerceUnsized<AccelPtr<U>> for AccelPtr<T>
  where
    T: CoerceUnsized<U> + PtrTy,
    U: PtrTy,
{ }

/// We allow unsized types because trait objects will have different
/// data AND vtable pointers.
/// Use SlicePtr below for `[T]` unsized types.
#[derive(Debug, Eq, PartialEq)]
pub struct NonNull<T>
  where T: ?Sized,
{
  pub(crate) host: StdNonNull<T>,
  pub(crate) accel: StdNonNull<T>,
}
impl<T> NonNull<T> {
  pub fn from_parts(host: *mut T, accel: *mut T) -> Option<Self> {
    Some(NonNull {
      host: StdNonNull::new(host as *mut _)?,
      accel: StdNonNull::new(accel as *mut _)?,
    })
  }
  pub fn new(ptr: Ptr<T>) -> Option<Self> {
    Self::from_parts(ptr.host as *mut _,
                     ptr.accel as *mut _)
  }
  pub unsafe fn new_unchecked(ptr: Ptr<T>) -> Self {
    NonNull {
      host: StdNonNull::new_unchecked(ptr.host as *mut _),
      accel: StdNonNull::new_unchecked(ptr.accel as *mut _),
    }
  }

  pub fn as_host_ptr(&self) -> &StdNonNull<T> { &self.host }
  pub fn as_accel_ptr(&self) -> &StdNonNull<T> { &self.accel }
  pub fn as_local_ptr(&self) -> &StdNonNull<T> {
    if is_host() {
      &self.host
    } else {
      &self.accel
    }
  }
  pub fn as_ptr(&self) -> Ptr<T> {
    Ptr {
      host: self.host.as_ptr() as *const _,
      accel: self.accel.as_ptr() as *const _,
    }
  }

  pub unsafe fn as_host_ref(&self) -> &T {
    self.host.as_ref()
  }
  pub unsafe fn as_accel_ref(&self) -> &T {
    self.accel.as_ref()
  }
  pub unsafe fn as_local_ref(&self) -> &T {
    self.as_local_ptr().as_ref()
  }
  pub unsafe fn as_ref(&self) -> Ref<T> {
    Ref {
      _owner: PhantomData,
      ptr: self.clone(),
    }
  }

  pub unsafe fn as_host_mut(&mut self) -> &mut T {
    self.host.as_mut()
  }
  pub unsafe fn as_accel_mut(&mut self) -> &mut T {
    self.accel.as_mut()
  }
  pub unsafe fn as_local_mut(&mut self) -> &mut T {
    if is_host() {
      self.host.as_mut()
    } else {
      self.accel.as_mut()
    }
  }
  pub unsafe fn as_mut(&mut self) -> Mut<T> {
    Mut {
      _owner: PhantomData,
      ptr: self.clone(),
    }
  }

  pub const fn cast<U>(self) -> NonNull<U> {
    NonNull {
      host: self.host.cast(),
      accel: self.accel.cast(),
    }
  }
}
impl<T> Copy for NonNull<T>
  where T: ?Sized,
{ }
impl<T> Clone for NonNull<T>
  where T: ?Sized,
{
  fn clone(&self) -> Self { *self }
}
impl<T> fmt::Pointer for NonNull<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if f.alternate() {
      write!(f, r#"NonNull {{
  host:  {:p},
  accel: {:p},
}}"#,
             self.host, self.accel)
    } else {
      write!(f, r#"NonNull {{ host: {:p}, accel: {:p}, }}"#,
             self.host, self.accel)
    }
  }
}

/// This is slightly more efficient than storing two `*const [T]`s, as
/// in that case the length will be duplicated.
#[derive(Debug, Eq, PartialEq)]
pub struct SlicePtr<T> {
  pub(super) host: *const T,
  pub(super) accel: *const T,
  pub(super) len: usize,
}
impl<T> SlicePtr<T> {
  pub fn from_parts(ptr: Ptr<T>, len: usize) -> Self {
    SlicePtr {
      host: ptr.host,
      accel: ptr.accel,
      len,
    }
  }

  pub fn as_ptr(&self) -> Ptr<T> {
    Ptr {
      host: self.host,
      accel: self.accel,
    }
  }

  pub fn as_host_ptr(&self) -> *const T {
    self.host
  }
  pub fn as_accel_ptr(&self) -> *const T {
    self.accel
  }
  pub fn as_local_ptr(&self) -> *const T {
    if is_host() {
      self.host
    } else {
      self.accel
    }
  }

  pub unsafe fn as_host_slice(&self) -> *const [T] {
    slice_from_raw_parts(self.as_host_ptr(), self.len())
  }
  pub unsafe fn as_accel_slice(&self) -> *const [T] {
    slice_from_raw_parts(self.as_accel_ptr(), self.len())
  }
  pub unsafe fn as_local_slice(&self) -> *const [T] {
    slice_from_raw_parts(self.as_local_ptr(), self.len())
  }

  pub unsafe fn as_slice(&self) -> SliceRef<T> {
    SliceRef {
      _owner: PhantomData,
      ptr: self.clone(),
    }
  }
  pub unsafe fn as_mut_slice<'a>(self) -> SliceMut<'a, T> {
    SliceMut {
      _owner: PhantomData,
      ptr: self.clone(),
    }
  }

  pub fn len(self) -> usize {
    self.len
  }

  pub fn index_ptr<I>(self, index: I) -> Ptr<T>
    where I: SliceIndex<[T], Output = T>,
  {
    // compute the offset w/ the host ref, then use that to offset
    // the accel ptr.
    // XXX hopefully LLVM can vectorize this
    let host_slice = unsafe {
      from_raw_parts(self.host, self.len)
    };
    let r = SliceIndex::index(index, host_slice);

    let ptr = host_slice.as_ptr() as usize;
    let slice_ptr = r as *const T as usize;

    let offset = slice_ptr - ptr;
    Ptr {
      host: r as *const T,
      accel: unsafe { self.accel.add(offset) },
    }
  }

  pub fn slice<I>(self, index: I) -> Self
    where I: SliceIndex<[T], Output = [T]>,
  {
    // compute the offset w/ the host ref, then use that to offset
    // the accel ptr.
    let host_slice = unsafe {
      from_raw_parts(self.host, self.len)
    };
    let slice = SliceIndex::index(index, host_slice);

    let ptr = host_slice.as_ptr() as usize;
    let slice_ptr = slice.as_ptr() as usize;

    let offset = slice_ptr - ptr;
    SlicePtr {
      host: slice.as_ptr(),
      accel: unsafe { self.accel.add(offset) },
      len: slice.len(),
    }
  }
}
impl<T> Copy for SlicePtr<T> { }
impl<T> Clone for SlicePtr<T> {
  fn clone(&self) -> Self { *self }
}
impl<T> fmt::Pointer for SlicePtr<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if f.alternate() {
      write!(f, r#"SlicePtr {{
  host:  {:p},
  accel: {:p},
  len:   {},
}}"#,
             self.host, self.accel, self.len)
    } else {
      write!(f, r#"SlicePtr {{ host: {:p}, accel: {:p}, len: {}, }}"#,
             self.host, self.accel, self.len)
    }
  }
}
