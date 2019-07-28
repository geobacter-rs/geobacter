
use std::cmp;
use std::fmt;
use std::marker::{PhantomData, };
use std::mem::transmute;
use std::ops::*;

use crate::ptr::NonNull;
use ptr::{AccelPtr, PtrTy, PtrRefTy};
use platform::is_host;

/// A type which when dereferenced will automatically use the correct pointer.
#[derive(Clone, Copy)]
pub struct Ref<'a, T>
  where T: ?Sized,
{
  pub(crate) _owner: PhantomData<&'a T>,
  pub(crate) ptr: NonNull<T>,
}

impl<'a, T> Ref<'a, T>
  where T: ?Sized,
{
}

impl<'a, T> AsRef<T> for Ref<'a, T> {
  fn as_ref(&self) -> &T {
    unsafe { self.ptr.as_local_ref() }
  }
}
impl<'a, T> Deref for Ref<'a, T> {
  type Target = T;
  fn deref(&self) -> &T {
    unsafe { self.ptr.as_local_ref() }
  }
}
impl<'a, T> fmt::Debug for Ref<'a, T>
  where T: fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self.as_ref())
  }
}

/// A type which when dereferenced will automatically use the correct pointer.
pub struct Mut<'a, T>
  where T: ?Sized,
{
  pub(crate) _owner: PhantomData<&'a mut T>,
  pub(crate) ptr: NonNull<T>,
}

impl<'a, T> Mut<'a, T>
  where T: ?Sized,
{
  /// Create a temporary (in that self can be unborrowed, and used again)
  /// borrow
  pub fn into_ref<'b>(&'b self) -> Ref<'b, T>
    where 'a: 'b,
  {
    Ref {
      _owner: PhantomData,
      ptr: self.ptr.clone(),
    }
  }
}

impl<'a, T> AsRef<T> for Mut<'a, T> {
  fn as_ref(&self) -> &T {
    unsafe { self.ptr.as_local_ref() }
  }
}
impl<'a, T> AsMut<T> for Mut<'a, T> {
  fn as_mut(&mut self) -> &mut T {
    unsafe { self.ptr.as_local_mut() }
  }
}
impl<'a, T> Deref for Mut<'a, T> {
  type Target = T;
  fn deref(&self) -> &T {
    unsafe { self.ptr.as_local_ref() }
  }
}
impl<'a, T> DerefMut for Mut<'a, T> {
  fn deref_mut(&mut self) -> &mut T {
    unsafe { self.ptr.as_local_mut() }
  }
}
impl<'a, T> fmt::Debug for Mut<'a, T>
  where T: fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self.as_ref())
  }
}

pub trait RefTy<'a> { }
impl<'a, T> RefTy<'a> for &'a T
  where T: ?Sized + 'a,
{ }
impl<'a, T> RefTy<'a> for &'a mut T
  where T: ?Sized + 'a,
{ }

/// A newtype reference, representing references which are local to
/// an accelerator. Such references are often only dereferencable
/// on the specific accelerator it is associated with.
#[repr(transparent)]
pub struct AccelRefRaw<'a, T>(T, PhantomData<&'a ()>)
  where T: RefTy<'a> + 'a;
pub type AccelRef<'a, T> = AccelRefRaw<'a, &'a T>;
pub type AccelMut<'a, T>   = AccelRefRaw<'a, &'a mut T>;

impl<'a, T> AccelRefRaw<'a, T>
  where T: RefTy<'a>,
{
  pub unsafe fn from_ptr<P>(ptr: AccelPtr<P>) -> Option<Self>
    where P: PtrTy + PtrRefTy<'a, RefTy = T> + 'a,
  {
    ptr.as_ref_ty().map(|r| AccelRefRaw(r, PhantomData) )
  }
}

impl<'a, T> AccelRef<'a, &'a T>
  where T: ?Sized + 'a,
{ }

impl<'a, T> Copy for AccelRefRaw<'a, T>
  where T: RefTy<'a> + Copy,
{ }
impl<'a, T> Clone for AccelRefRaw<'a, T>
  where T: RefTy<'a> + Copy,
{
  fn clone(&self) -> Self { *self }
}
impl<'a, T> fmt::Debug for AccelRefRaw<'a, T>
  where T: RefTy<'a> + fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Debug::fmt(&self.0, f)
  }
}
impl<'a, T> Eq for AccelRefRaw<'a, T>
  where T: RefTy<'a> + Eq,
{ }
impl<'a, 'b, T, U> PartialEq<AccelRefRaw<'b, U>> for AccelRefRaw<'a, T>
  where T: RefTy<'a> + PartialEq<U>,
        U: RefTy<'b>
{
  fn eq(&self, rhs: &AccelRefRaw<'b, U>) -> bool { self.0 == rhs.0 }
}
impl<'a, T> Ord for AccelRefRaw<'a, T>
  where T: RefTy<'a> + Ord,
{
  fn cmp(&self, rhs: &Self) -> cmp::Ordering {
    self.0.cmp(&rhs.0)
  }
}
impl<'a, 'b, T, U> PartialOrd<AccelRefRaw<'b, U>> for AccelRefRaw<'a, T>
  where T: RefTy<'a> + PartialOrd<U>,
        U: RefTy<'b>
{
  fn partial_cmp(&self, rhs: &AccelRefRaw<'b, U>) -> Option<cmp::Ordering> {
    self.0.partial_cmp(&rhs.0)
  }
}
impl<'a, T> fmt::Pointer for AccelRefRaw<'a, T>
  where T: RefTy<'a> + fmt::Pointer,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Pointer::fmt(&self.0, f)
  }
}
/// XXX this still marginally unsafe: we don't check that the pointer
/// is actually accessible by the running processor, ie the pointer could
/// be accessible from a different accelerator.
/// TODO add intrinsic to get the running accelerator id.
impl<'a, T> Deref for AccelRefRaw<'a, &'a T>
  where &'a T: RefTy<'a>,
{
  type Target = T;
  fn deref(&self) -> &T {
    assert!(!is_host());
    self.0
  }
}
impl<'a, T> Deref for AccelRefRaw<'a, &'a mut T>
  where &'a mut T: RefTy<'a>,
{
  type Target = T;
  fn deref(&self) -> &T {
    assert!(!is_host());
    self.0
  }
}
impl<'a, T> DerefMut for AccelRefRaw<'a, &'a mut T>
  where &'a mut T: RefTy<'a>,
{
  fn deref_mut(&mut self) -> &mut T {
    assert!(!is_host());
    self.0
  }
}
impl<'a, T, U> CoerceUnsized<AccelRefRaw<'a, U>> for AccelRefRaw<'a, T>
  where
    T: CoerceUnsized<U> + RefTy<'a>,
    U: RefTy<'a>,
{ }

/// A newtype which asserts on if the host attemps to read from the
/// wrapped inner data. This is intended to be used behind a reference;
/// ie via `&AccelRefRaw2<T>` or `&mut AccelRefRaw2<T>`
///
/// This type should never be constructed. It should always be "created"
/// by transmuting a `&T` into `&AccelRefRaw2<T>`.
///
/// Such references are often only dereferencable
/// on the specific accelerator it is associated with.
/// XXX this still marginally unsafe: we don't check that the pointer
/// is actually accessible by the running processor, ie the pointer could
/// be accessible from a different accelerator, which if `T` is Sized can
/// still be dereferenced
/// TODO add intrinsic to get the running accelerator id.
#[repr(transparent)]
pub struct AccelRefRaw2<T>(T)
  where T: ?Sized;

impl<T> AccelRefRaw2<T>
  where T: ?Sized,
{
  pub unsafe fn ref_from_ptr<'a, P>(ptr: AccelPtr<P>) -> Option<&'a Self>
    where P: PtrTy<ElementTy = T> + PtrRefTy<'a, RefTy = &'a T> + 'a,
  {
    ptr.as_ref_ty().map(|r| transmute(r) )
  }
  pub unsafe fn mut_from_ptr<'a, P>(ptr: AccelPtr<P>) -> Option<&'a mut Self>
    where P: PtrTy<ElementTy = T> + PtrRefTy<'a, RefTy = &'a mut T> + 'a,
  {
    ptr.as_ref_ty().map(|r| transmute(r) )
  }

  pub fn as_ref(&self) -> &T {
    assert!(!is_host());
    &self.0
  }
  pub fn as_mut(&mut self) -> &mut T {
    assert!(!is_host());
    &mut self.0
  }

  pub unsafe fn unchecked_as_ref(&self) -> &T { &self.0 }
  pub unsafe fn unchecked_as_mut(&mut self) -> &mut T { &mut self.0 }
}
impl<T> AccelRefRaw2<[T]>
  where T: Sized,
{
  /// This is always safe to call, host or device, due to the way
  /// DSTs/slices work. They are fat pointers (ie `&self` is twice
  /// the size of a pointer), so reading the length just reads the
  /// second pointer sized integer (a `usize`) from `&self`.
  pub fn len(&self) -> usize {
    unsafe {
      self.unchecked_as_ref().len()
    }
  }

  // TODO implement the reset of the [T] subslice methods (the ones which
  // do not read any element in the slice).
}

// XXX Can't run any code to trip an assertion!
// This will probably never be fixable.
/*
impl<T> Copy for AccelRefRaw2<T>
  where T: Copy,
{ }
*/
impl<T> Clone for AccelRefRaw2<T>
  where T: Clone,
{
  fn clone(&self) -> Self {
    AccelRefRaw2(self.as_ref().clone())
  }
}
impl<T> Eq for AccelRefRaw2<T>
  where T: Eq,
{ }
impl<T, U> PartialEq<AccelRefRaw2<U>> for AccelRefRaw2<T>
  where T: PartialEq<U>,
{
  fn eq(&self, rhs: &AccelRefRaw2<U>) -> bool {
    self.as_ref().eq(rhs.as_ref())
  }
}
impl<T> Ord for AccelRefRaw2<T>
  where T: Ord,
{
  fn cmp(&self, rhs: &Self) -> cmp::Ordering {
    self.as_ref().cmp(rhs.as_ref())
  }
}
impl<T, U> PartialOrd<AccelRefRaw2<U>> for AccelRefRaw2<T>
  where T: PartialOrd<U>,
{
  fn partial_cmp(&self, rhs: &AccelRefRaw2<U>) -> Option<cmp::Ordering> {
    self.as_ref().partial_cmp(rhs.as_ref())
  }
}

impl<T> fmt::Debug for AccelRefRaw2<T>
  where T: ?Sized + fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if is_host() {
      fmt::Pointer::fmt(&self, f)
    } else {
      fmt::Debug::fmt(&self.0, f)
    }
  }
}
/// XXX this still marginally unsafe: we don't check that the pointer
/// is actually accessible by the running processor, ie the pointer could
/// be accessible from a different accelerator.
/// TODO add intrinsic to get the running accelerator id.
impl<T> Deref for AccelRefRaw2<T>
  where T: ?Sized,
{
  type Target = T;
  fn deref(&self) -> &T {
    self.as_ref()
  }
}
impl<T> DerefMut for AccelRefRaw2<T>
  where T: ?Sized,
{
  fn deref_mut(&mut self) -> &mut T {
    self.as_mut()
  }
}
impl<T, U> CoerceUnsized<AccelRefRaw2<U>> for AccelRefRaw2<T>
  where
    T: CoerceUnsized<U> + ?Sized,
    U: ?Sized,
{ }
