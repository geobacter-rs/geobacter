
//! A type that is held in whole in a register.
//! This is basically the primitive types.
//!
//! There is a lot more generic code possible, if it weren't for
//! rustc bugs.
//! Can't use `[T; N]` as the inner type on structs marked with
//! `#[repr(simd)]` (this is absurd, imo).
//! Can't transmute `[T; 2]` to `InnerVecN2<T>`, even with
//! `where T: Sized,`.
//! Can't use `type FromT = [T; Self::N];`, even with
//! `where Self: Nn,`.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Mul, Div, Sub, Add, MulAssign, DivAssign,
               SubAssign, AddAssign, Deref, DerefMut,
               Index, IndexMut, };

use serde::{Serialize, Serializer,
            Deserialize, Deserializer};

use num_traits::{self, Float, AsPrimitive, };

extern "platform-intrinsic" {
  pub fn simd_eq<T, U>(x: T, y: T) -> U;
  pub fn simd_ne<T, U>(x: T, y: T) -> U;
  pub fn simd_lt<T, U>(x: T, y: T) -> U;
  pub fn simd_le<T, U>(x: T, y: T) -> U;
  pub fn simd_gt<T, U>(x: T, y: T) -> U;
  pub fn simd_ge<T, U>(x: T, y: T) -> U;

  pub fn simd_shuffle2<T, U>(x: T, y: T, idx: [u32; 2]) -> U;
  pub fn simd_shuffle4<T, U>(x: T, y: T, idx: [u32; 4]) -> U;
  pub fn simd_shuffle8<T, U>(x: T, y: T, idx: [u32; 8]) -> U;
  pub fn simd_shuffle16<T, U>(x: T, y: T, idx: [u32; 16]) -> U;
  pub fn simd_shuffle32<T, U>(x: T, y: T, idx: [u32; 32]) -> U;
  pub fn simd_shuffle64<T, U>(x: T, y: T, idx: [u32; 64]) -> U;
  pub fn simd_shuffle128<T, U>(x: T, y: T, idx: [u32; 128]) -> U;

  pub fn simd_insert<T, U>(x: T, idx: u32, val: U) -> T;
  pub fn simd_extract<T, U>(x: T, idx: u32) -> U;

  pub fn simd_cast<T, U>(x: T) -> U;

  pub fn simd_add<T>(x: T, y: T) -> T;
  pub fn simd_sub<T>(x: T, y: T) -> T;
  pub fn simd_mul<T>(x: T, y: T) -> T;
  pub fn simd_div<T>(x: T, y: T) -> T;
  pub fn simd_rem<T>(x: T, y: T) -> T;
  pub fn simd_shl<T>(x: T, y: T) -> T;
  pub fn simd_shr<T>(x: T, y: T) -> T;
  pub fn simd_and<T>(x: T, y: T) -> T;
  pub fn simd_or<T>(x: T, y: T) -> T;
  pub fn simd_xor<T>(x: T, y: T) -> T;

  pub fn simd_reduce_add_unordered<T, U>(x: T) -> U;
  pub fn simd_reduce_mul_unordered<T, U>(x: T) -> U;
  pub fn simd_reduce_add_ordered<T, U>(x: T, acc: U) -> U;
  pub fn simd_reduce_mul_ordered<T, U>(x: T, acc: U) -> U;
  pub fn simd_reduce_min<T, U>(x: T) -> U;
  pub fn simd_reduce_max<T, U>(x: T) -> U;
  pub fn simd_reduce_min_nanless<T, U>(x: T) -> U;
  pub fn simd_reduce_max_nanless<T, U>(x: T) -> U;
  pub fn simd_reduce_and<T, U>(x: T) -> U;
  pub fn simd_reduce_or<T, U>(x: T) -> U;
  pub fn simd_reduce_xor<T, U>(x: T) -> U;
  pub fn simd_reduce_all<T>(x: T) -> bool;
  pub fn simd_reduce_any<T>(x: T) -> bool;

  pub fn simd_select<M, T>(m: M, a: T, b: T) -> T;

  pub fn simd_fmin<T>(a: T, b: T) -> T;
  pub fn simd_fmax<T>(a: T, b: T) -> T;

  pub fn simd_fsqrt<T>(a: T) -> T;
  pub fn simd_fabs<T>(a: T) -> T;
  pub fn simd_fma<T>(a: T, b: T, c: T) -> T;
}

pub trait Nn {
  const N: usize;
}
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N1<T = ()>(PhantomData<T>) where T: ?Sized;
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N2<T = ()>(PhantomData<T>) where T: ?Sized;
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N3<T = ()>(PhantomData<T>) where T: ?Sized;
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N4<T = ()>(PhantomData<T>) where T: ?Sized;
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N8<T = ()>(PhantomData<T>) where T: ?Sized;
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N16<T = ()>(PhantomData<T>) where T: ?Sized;
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N24<T = ()>(PhantomData<T>) where T: ?Sized;
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N32<T = ()>(PhantomData<T>) where T: ?Sized;
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N64<T = ()>(PhantomData<T>) where T: ?Sized;

impl<T> Nn for N1<T>
  where T: ?Sized,
{
  const N: usize = 1;
}
impl<T> Nn for N2<T>
  where T: ?Sized,
{
  const N: usize = 2;
}
impl<T> Nn for N3<T>
  where T: ?Sized,
{
  const N: usize = 3;
}
impl<T> Nn for N4<T>
  where T: ?Sized,
{
  const N: usize = 4;
}
impl<T> Nn for N8<T>
  where T: ?Sized,
{
  const N: usize = 8;
}
impl<T> Nn for N16<T>
  where T: ?Sized,
{
  const N: usize = 16;
}
impl<T> Nn for N24<T>
  where T: ?Sized,
{
  const N: usize = 24;
}
impl<T> Nn for N32<T>
  where T: ?Sized,
{
  const N: usize = 32;
}
impl<T> Nn for N64<T>
  where T: ?Sized,
{
  const N: usize = 64;
}

#[derive(Clone, Copy, Debug)]
#[repr(simd)]
pub struct InnerVecN1<T>(pub T, )
  where T: Copy + Sized;
#[derive(Clone, Copy, Debug)]
#[repr(simd)]
pub struct InnerVecN2<T>(pub T, pub T, )
  where T: Copy + Sized;
#[derive(Clone, Copy, Debug)]
#[repr(simd)]
pub struct InnerVecN3<T>(pub T, pub T, pub T, )
  where T: Copy + Sized;
#[derive(Clone, Copy, Debug)]
#[repr(simd)]
pub struct InnerVecN4<T>(pub T, pub T, pub T, pub T, )
  where T: Copy + Sized;
#[derive(Clone, Copy, Debug)]
#[repr(simd)]
pub struct InnerVecN8<T>(pub T, pub T, pub T, pub T,
                         pub T, pub T, pub T, pub T, )
  where T: Copy + Sized;
#[derive(Clone, Copy, Debug)]
#[repr(simd)]
pub struct InnerVecN16<T>(pub T, pub T, pub T, pub T,
                          pub T, pub T, pub T, pub T,
                          pub T, pub T, pub T, pub T,
                          pub T, pub T, pub T, pub T, )
  where T: Copy + Sized;

impl<T> From<[T; 1]> for InnerVecN1<T>
  where T: Copy + Sized,
{
  fn from(v: [T; 1]) -> Self {
    unsafe {
      ::std::mem::transmute_copy(&v)
    }
  }
}
impl<T> From<[T; 2]> for InnerVecN2<T>
  where T: Copy + Sized,
{
  fn from(v: [T; 2]) -> Self {
    unsafe {
      ::std::mem::transmute_copy(&v)
    }
  }
}
impl<T> From<[T; 3]> for InnerVecN3<T>
  where T: Copy + Sized,
{
  fn from(v: [T; 3]) -> Self {
    unsafe {
      ::std::mem::transmute_copy(&v)
    }
  }
}
impl<T> From<[T; 4]> for InnerVecN4<T>
  where T: Copy + Sized,
{
  fn from(v: [T; 4]) -> Self {
    unsafe {
      ::std::mem::transmute_copy(&v)
    }
  }
}
impl<T> From<[T; 8]> for InnerVecN8<T>
  where T: Copy + Sized,
{
  fn from(v: [T; 8]) -> Self {
    unsafe {
      ::std::mem::transmute_copy(&v)
    }
  }
}
impl<T> From<[T; 16]> for InnerVecN16<T>
  where T: Copy + Sized,
{
  fn from(v: [T; 16]) -> Self {
    unsafe {
      ::std::mem::transmute_copy(&v)
    }
  }
}
/// This is broken currently, due to the parameterization on `N*`.
pub trait VecNCast<T, U>
  where T: ScalarT + Into<U>,
        U: ScalarT,
{
  type From: VecN<T>;
  type Into: VecN<U>;

  #[doc(hidden)]
  fn cast_impl(f: &<Self::From as VecN<T>>::FromT,
               out: &mut <Self::Into as VecN<U>>::FromT);

  fn cast(v: Vec<T, Self::From>) -> Vec<U, Self::Into> {
    unsafe { self::simd_cast(v) }
  }
}
// could be made generic over `Nn::N`, if not for rustc bugs.
impl<T, U> VecNCast<T, U> for N1<(T, U)>
  where N1<T>: VecN<T>,
        N1<U>: VecN<U>,
        <N1<T> as VecN<T>>::FromT: Index<usize, Output = T>,
        <N1<U> as VecN<U>>::FromT: IndexMut<usize, Output = U>,
        T: ScalarT + Copy + Into<U>,
        U: ScalarT,
{
  type From = N1<T>;
  type Into = N1<U>;

  fn cast_impl(f: &<Self::From as VecN<T>>::FromT,
               out: &mut <Self::Into as VecN<U>>::FromT) {
    let mut i = 0usize;
    out[{ i += 1; i } - 1] = f[i - 1].into();
  }
}
impl<T, U> VecNCast<T, U> for N2<(T, U)>
  where N2<T>: VecN<T>,
        N2<U>: VecN<U>,
        <N2<T> as VecN<T>>::FromT: Index<usize, Output = T>,
        <N2<U> as VecN<U>>::FromT: IndexMut<usize, Output = U>,
        T: ScalarT + Copy + Into<U>,
        U: ScalarT,
{
  type From = N2<T>;
  type Into = N2<U>;

  fn cast_impl(f: &<Self::From as VecN<T>>::FromT,
               out: &mut <Self::Into as VecN<U>>::FromT) {
    let mut i = 0usize;
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
  }
}
impl<T, U> VecNCast<T, U> for N3<(T, U)>
  where N3<T>: VecN<T>,
        N3<U>: VecN<U>,
        <N3<T> as VecN<T>>::FromT: Index<usize, Output = T>,
        <N3<U> as VecN<U>>::FromT: IndexMut<usize, Output = U>,
        T: ScalarT + Copy + Into<U>,
        U: ScalarT,
{
  type From = N3<T>;
  type Into = N3<U>;

  fn cast_impl(f: &<Self::From as VecN<T>>::FromT,
               out: &mut <Self::Into as VecN<U>>::FromT) {
    let mut i = 0usize;
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
  }
}
impl<T, U> VecNCast<T, U> for N4<(T, U)>
  where N4<T>: VecN<T>,
        N4<U>: VecN<U>,
        <N4<T> as VecN<T>>::FromT: Index<usize, Output = T>,
        <N4<U> as VecN<U>>::FromT: IndexMut<usize, Output = U>,
        T: ScalarT + Copy + Into<U>,
        U: ScalarT,
{
  type From = N4<T>;
  type Into = N4<U>;

  fn cast_impl(f: &<Self::From as VecN<T>>::FromT,
               out: &mut <Self::Into as VecN<U>>::FromT) {
    let mut i = 0usize;
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
  }
}
impl<T, U> VecNCast<T, U> for N8<(T, U)>
  where N8<T>: VecN<T>,
        N8<U>: VecN<U>,
        <N8<T> as VecN<T>>::FromT: Index<usize, Output = T>,
        <N8<U> as VecN<U>>::FromT: IndexMut<usize, Output = U>,
        T: ScalarT + Copy + Into<U>,
        U: ScalarT,
{
  type From = N8<T>;
  type Into = N8<U>;

  fn cast_impl(f: &<Self::From as VecN<T>>::FromT,
               out: &mut <Self::Into as VecN<U>>::FromT) {
    let mut i = 0usize;
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();

    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
  }
}
impl<T, U> VecNCast<T, U> for N16<(T, U)>
  where N16<T>: VecN<T>,
        N16<U>: VecN<U>,
        <N16<T> as VecN<T>>::FromT: Index<usize, Output = T>,
        <N16<U> as VecN<U>>::FromT: IndexMut<usize, Output = U>,
        T: ScalarT + Copy + Into<U>,
        U: ScalarT,
{
  type From = N16<T>;
  type Into = N16<U>;

  fn cast_impl(f: &<Self::From as VecN<T>>::FromT,
               out: &mut <Self::Into as VecN<U>>::FromT) {
    let mut i = 0usize;
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();

    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();

    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();

    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
    out[{ i += 1; i } - 1] = f[i - 1].into();
  }
}

pub trait VecN<T>
  where T: ScalarT,
{
  type FromT: Copy;
  type InnerT: Copy + From<Self::FromT>;

  fn splat_impl(v: T) -> Self::FromT;
  fn splat<U>(v: U) -> Self::InnerT
    where U: Into<T>,
  {
    Self::splat_impl(v.into()).into()
  }
}
impl<T> VecN<T> for N1<T>
  where T: ScalarT + Copy + Sized,
{
  type FromT = [T; 1];
  type InnerT = InnerVecN1<T>;

  fn splat_impl(v: T) -> Self::FromT {
    [v; 1]
  }
}
impl<T> VecN<T> for N2<T>
  where T: ScalarT + Copy + Sized,
{
  type FromT = [T; 2];
  type InnerT = InnerVecN2<T>;

  fn splat_impl(v: T) -> Self::FromT {
    [v; 2]
  }
}
impl<T> VecN<T> for N3<T>
  where T: ScalarT + Copy + Sized,
{
  type FromT = [T; 3];
  type InnerT = InnerVecN3<T>;

  fn splat_impl(v: T) -> Self::FromT {
    [v; 3]
  }
}
impl<T> VecN<T> for N4<T>
  where T: ScalarT + Copy + Sized,
{
  type FromT = [T; 4];
  type InnerT = InnerVecN4<T>;

  fn splat_impl(v: T) -> Self::FromT {
    [v; 4]
  }
}
impl<T> VecN<T> for N8<T>
  where T: ScalarT + Copy + Sized,
{
  type FromT = [T; 8];
  type InnerT = InnerVecN8<T>;

  fn splat_impl(v: T) -> Self::FromT {
    [v; 8]
  }
}
impl<T> VecN<T> for N16<T>
  where T: ScalarT + Copy + Sized,
{
  type FromT = [T; 16];
  type InnerT = InnerVecN16<T>;

  fn splat_impl(v: T) -> Self::FromT {
    [v; 16]
  }
}

#[hsa_lang_type = "vec"]
#[repr(transparent)]
pub struct Vec<T, N>(pub <N as VecN<T>>::InnerT)
  where N: VecN<T>,
        T: ScalarT;

impl<T, N> Vec<T, N>
  where N: VecN<T>,
        T: ScalarT,
{
  pub fn new(a: <N as VecN<T>>::FromT) -> Self {
    Vec::new_v(a.into())
  }
}

pub type Vec1<T> = Vec<T, N1<T>>;
pub type Vec2<T> = Vec<T, N2<T>>;
pub type Vec3<T> = Vec<T, N3<T>>;
pub type Vec4<T> = Vec<T, N4<T>>;
pub type Vec8<T> = Vec<T, N8<T>>;
pub type Vec16<T> = Vec<T, N16<T>>;

// these will probably not stay.
impl<T, N> Deref for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  type Target = <N as VecN<T>>::FromT;
  fn deref(&self) -> &Self::Target {
    unsafe {
      ::std::mem::transmute(&self.0)
    }
  }
}
impl<T, N> DerefMut for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  fn deref_mut(&mut self) -> &mut <N as VecN<T>>::FromT {
    unsafe {
      ::std::mem::transmute(&mut self.0)
    }
  }
}

impl<T, N> Copy for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{ }
impl<T, N> Clone for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Clone,
{
  fn clone(&self) -> Self {
    Vec(self.0.clone())
  }
}

impl<T, N> Mul for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  type Output = Self;
  fn mul(self, rhs: Self) -> Self {
    Vec(unsafe { self::simd_mul(self.0, rhs.0) })
  }
}
impl<T, N> Div for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  type Output = Self;
  fn div(self, rhs: Self) -> Self {
    Vec(unsafe { self::simd_div(self.0, rhs.0) })
  }
}
impl<T, N> Add for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  type Output = Self;
  fn add(self, rhs: Self) -> Self {
    Vec(unsafe { self::simd_add(self.0, rhs.0) })
  }
}
impl<T, N> Sub for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  type Output = Self;
  fn sub(self, rhs: Self) -> Self {
    Vec(unsafe { self::simd_sub(self.0, rhs.0) })
  }
}
impl<T, N> MulAssign for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  fn mul_assign(&mut self, rhs: Self) {
    self.0 = unsafe { self::simd_mul(self.0, rhs.0) };
  }
}
impl<T, N> DivAssign for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  fn div_assign(&mut self, rhs: Self) {
    self.0 = unsafe { self::simd_div(self.0, rhs.0) };
  }
}
impl<T, N> AddAssign for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  fn add_assign(&mut self, rhs: Self){
    self.0 = unsafe { self::simd_add(self.0, rhs.0) };
  }
}
impl<T, N> SubAssign for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  fn sub_assign(&mut self, rhs: Self) {
    self.0 = unsafe { self::simd_sub(self.0, rhs.0) };
  }
}

impl<T, N> Mul<T> for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  type Output = Self;
  fn mul(self, rhs: T) -> Self {
    let rhs: Vec<T, N> = Vec(<N as VecN<T>>::splat(rhs));
    Vec(unsafe { self::simd_mul(self.0, rhs.0) })
  }
}
impl<T, N> Div<T> for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  type Output = Self;
  fn div(self, rhs: T) -> Self {
    let rhs: Vec<T, N> = Vec(<N as VecN<T>>::splat(rhs));
    Vec(unsafe { self::simd_div(self.0, rhs.0) })
  }
}
impl<T, N> Add<T> for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  type Output = Self;
  fn add(self, rhs: T) -> Self {
    let rhs: Vec<T, N> = Vec(<N as VecN<T>>::splat(rhs));
    Vec(unsafe { self::simd_add(self.0, rhs.0) })
  }
}
impl<T, N> Sub<T> for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  type Output = Self;
  fn sub(self, rhs: T) -> Self {
    let rhs: Vec<T, N> = Vec(<N as VecN<T>>::splat(rhs));
    Vec(unsafe { self::simd_sub(self.0, rhs.0) })
  }
}
impl<T, N> MulAssign<T> for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  fn mul_assign(&mut self, rhs: T) {
    let rhs: Vec<T, N> = Vec(<N as VecN<T>>::splat(rhs));
    self.0 = unsafe { self::simd_mul(self.0, rhs.0) };
  }
}
impl<T, N> DivAssign<T> for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  fn div_assign(&mut self, rhs: T) {
    let rhs: Vec<T, N> = Vec(<N as VecN<T>>::splat(rhs));
    self.0 = unsafe { self::simd_div(self.0, rhs.0) };
  }
}
impl<T, N> AddAssign<T> for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  fn add_assign(&mut self, rhs: T) {
    let rhs: Vec<T, N> = Vec(<N as VecN<T>>::splat(rhs));
    self.0 = unsafe { self::simd_add(self.0, rhs.0) };
  }
}
impl<T, N> SubAssign<T> for Vec<T, N>
  where N: VecN<T>,
        T: ScalarT + Copy,
{
  fn sub_assign(&mut self, rhs: T) {
    let rhs: Vec<T, N> = Vec(<N as VecN<T>>::splat(rhs));
    self.0 = unsafe { self::simd_sub(self.0, rhs.0) };
  }
}

pub trait VecFloat<T, N>
  where N: VecN<T> + Nn,
        T: ScalarT + Copy + num_traits::Float + PartialOrd + 'static,
        <N as VecN<T>>::InnerT: Copy,
        f64: num_traits::AsPrimitive<T>,
{
  #[doc(hidden)]
  fn inner(&self) -> &<N as VecN<T>>::InnerT;
  #[doc(hidden)]
  fn inner_mut(&mut self) -> &mut <N as VecN<T>>::InnerT;

  fn length_sqrd(&self) -> T {
    let inner = *self.inner();
    if <N as Nn>::N == 1 {
      let inner = unsafe { self::simd_mul(inner, inner) };
      unsafe { self::simd_extract(inner, 0) }
    } else {
      let mid = unsafe { self::simd_mul(inner, inner) };
      unsafe { self::simd_reduce_add_unordered(mid) }
    }
  }
  fn length(&self) -> T {
    let inner = *self.inner();
    if <N as Nn>::N == 1 {
      let inner = unsafe { self::simd_fabs(inner) };
      unsafe { self::simd_extract(inner, 0) }
    } else {
      let mid = unsafe { self::simd_mul(inner, inner) };
      let v: T = unsafe { self::simd_reduce_add_unordered(mid) };
      v.sqrt()
    }
  }

  /// Accurate.
  fn normalize(&mut self) -> T {
    let len_sqrd = if <N as Nn>::N == 1 {
      self.length()
    } else {
      self.length_sqrd()
    };
    let l = (1.0 - f64::epsilon()).as_();
    let h = (1.0 + f64::epsilon()).as_();
    if l <= len_sqrd && h >= len_sqrd {
      // we're already approximately normalized.
      len_sqrd
    } else {
      let len = if <N as Nn>::N == 1 {
        len_sqrd
      } else {
        len_sqrd.sqrt()
      };
      let vlen = <N as VecN<T>>::splat(len);
      let inner = *self.inner();
      *self.inner_mut() = unsafe { self::simd_div(inner, vlen) };
      len
    }
  }
  /// Fast, possibly loses some precision if already normalized.
  fn fast_normalize(&mut self) -> T {
    let len = self.length();
    let vlen = <N as VecN<T>>::splat(len.recip());
    let inner = *self.inner();
    *self.inner_mut() = unsafe { self::simd_mul(inner, vlen) };
    len
  }
}
impl<T, N> VecFloat<T, N> for Vec<T, N>
  where N: VecN<T> + Nn,
        T: ScalarT + Copy + num_traits::Float + PartialOrd + 'static,
        <N as VecN<T>>::InnerT: Copy,
        f64: num_traits::AsPrimitive<T>,
{
  #[doc(hidden)]
  fn inner(&self) -> &<N as VecN<T>>::InnerT { &self.0 }
  #[doc(hidden)]
  fn inner_mut(&mut self) -> &mut <N as VecN<T>>::InnerT { &mut self.0 }
}

pub trait MatN<T>: Nn {
  type InnerT: Copy;

  fn splat_impl(v: T) -> Self::InnerT;
  fn splat<U>(v: U) -> Self::InnerT
    where U: Into<T>,
  {
    Self::splat_impl(v.into())
  }
}
impl<T> MatN<T> for N2<T>
  where T: Copy,
{
  type InnerT = [[T; 2]; 2];

  fn splat_impl(v: T) -> Self::InnerT {
    let v = [v; 2];
    [v; 2]
  }
}
impl<T> MatN<T> for N3<T>
  where T: Copy,
{
  type InnerT = [[T; 3]; 3];

  fn splat_impl(v: T) -> Self::InnerT {
    let v = [v; 3];
    [v; 3]
  }
}
impl<T> MatN<T> for N4<T>
  where T: Copy,
{
  type InnerT = [[T; 4]; 4];

  fn splat_impl(v: T) -> Self::InnerT {
    let v = [v; 4];
    [v; 4]
  }
}

#[hsa_lang_type = "mat"]
pub struct Mat<T, N>(pub(crate) <N as MatN<T>>::InnerT)
  where N: MatN<T>,
        T: ScalarT;

impl<T, N> Copy for Mat<T, N>
  where N: MatN<T>,
        T: ScalarT + Copy,
{ }
impl<T, N> Clone for Mat<T, N>
  where N: MatN<T>,
        T: ScalarT + Copy,
{
  fn clone(&self) -> Self {
    // TODO: don't rely on Copy for this.
    unsafe {
      ::std::mem::transmute_copy(self)
    }
  }
}

pub trait RealBitWidth: Nn {
  type RealStorageTy: Copy + Serialize + for<'a> Deserialize<'a> + PartialOrd + PartialEq;
}
impl RealBitWidth for N16<()> {
  type RealStorageTy = u16;
}
impl RealBitWidth for N24<()> {
  type RealStorageTy = [u8; 3];
}
impl RealBitWidth for N32<()> {
  type RealStorageTy = f32;
}
impl RealBitWidth for N64<()> {
  type RealStorageTy = f64;
}

#[hsa_lang_type = "real"]
#[repr(transparent)]
pub struct Real<Bits>(pub(crate) Bits::RealStorageTy)
  where Bits: RealBitWidth;

/*macro_rules! impl_static {
  ($wrapper:ident, [$($name:ident),*]) => (
    $(fn $name() -> Self {
      $wrapper(<Bits::RealStorageTy as num_traits::Float>::$name())
    })*
  )
}
impl<Bits> num_traits::Float for Real<Bits>
  where Bits: RealBitWidth,
        Bits::RealStorageTy: num_traits::Float,
{
  impl_static!(Real, [nan, infinity, neg_infinity, ]);
}*/

pub trait RealT { }
impl<Bits> RealT for Real<Bits>
  where Bits: RealBitWidth,
{ }

pub trait IntBitWidth: Nn {
  type UIntStorageTy: Copy + Serialize + for<'a> Deserialize<'a> + Hash + PartialOrd + PartialEq + Ord + Eq;
  type IntStorageTy: Copy + Serialize + for<'a> Deserialize<'a> + Hash + PartialOrd + PartialEq + Ord + Eq;
}
impl IntBitWidth for N8<()> {
  type UIntStorageTy = u8;
  type IntStorageTy  = i8;
}
impl IntBitWidth for N16<()> {
  type UIntStorageTy = u16;
  type IntStorageTy  = i16;
}
impl IntBitWidth for N32<()> {
  type UIntStorageTy = u32;
  type IntStorageTy  = i32;
}
impl IntBitWidth for N64<()> {
  type UIntStorageTy = u64;
  type IntStorageTy  = i64;
}

#[hsa_lang_type = "uint"]
#[derive(Hash)]
#[repr(transparent)]
pub struct UInt<Bits>(pub(crate) Bits::UIntStorageTy)
  where Bits: IntBitWidth;
#[hsa_lang_type = "int"]
#[derive(Hash)]
#[repr(transparent)]
pub struct Int<Bits>(pub(crate) Bits::IntStorageTy)
  where Bits: IntBitWidth;

pub trait ScalarT { }
impl ScalarT for Real<N16<()>> { }
//impl ScalarT for Real<N24<()>> { }
impl ScalarT for Real<N32<()>> { }
impl ScalarT for Real<N64<()>> { }
impl ScalarT for UInt<N8<()>>  { }
impl ScalarT for UInt<N16<()>> { }
impl ScalarT for UInt<N32<()>> { }
impl ScalarT for UInt<N64<()>> { }
impl ScalarT for Int<N8<()>>   { }
impl ScalarT for Int<N16<()>>  { }
impl ScalarT for Int<N32<()>>  { }
impl ScalarT for Int<N64<()>>  { }

macro_rules! impl_traits_for_scalar {
  ($ty:ident, $bt:ident, $storage_assoc:ident) => {

impl<B> Serialize for $ty<B>
  where B: $bt,
        <B as $bt>::$storage_assoc: Serialize,
{
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer
  {
    self.0.serialize(serializer)
  }
}

impl<'a, B> Deserialize<'a> for $ty<B>
  where B: $bt,
        <B as $bt>::$storage_assoc: for<'b> Deserialize<'b>,
{
  fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'a>>::Error>
    where D: Deserializer<'a>,
  {
    let inner: <B as $bt>::$storage_assoc =
      <<B as $bt>::$storage_assoc as Deserialize>::deserialize(deserializer)?;

    Ok($ty::new_v(inner))
  }
}

impl<B> Copy for $ty<B>
  where B: $bt,
        <B as $bt>::$storage_assoc: Copy,
{ }
impl<B> Clone for $ty<B>
  where B: $bt,
        <B as $bt>::$storage_assoc: Clone,
{
  fn clone(&self) -> Self {
    $ty(self.0.clone())
  }
}

impl<B> Default for $ty<B>
  where B: $bt,
        <B as $bt>::$storage_assoc: Default,
{
  fn default() -> Self {
    Self::new_v(Default::default())
  }
}
impl<B> Ord for $ty<B>
  where B: $bt,
        <B as $bt>::$storage_assoc: Ord,
{
  fn cmp(&self, rhs: &Self) -> ::std::cmp::Ordering {
    self.0.cmp(&rhs.0)
  }
}
impl<B> Eq for $ty<B>
  where B: $bt,
        <B as $bt>::$storage_assoc: Eq,
{ }
impl<LB, RS> PartialOrd<RS> for $ty<LB>
  where LB: $bt,
        RS: Copy + Into<Self>,
        <LB as $bt>::$storage_assoc: PartialOrd,
{
  fn partial_cmp(&self, &rhs: &RS) -> Option<::std::cmp::Ordering> {
    let $ty(rhs): Self = rhs.into();
    self.0.partial_cmp(&rhs)
  }
}
impl<LB, RS> PartialEq<RS> for $ty<LB>
  where LB: $bt,
        RS: Copy + Into<Self>,
        <LB as $bt>::$storage_assoc: PartialEq,
{
  fn eq(&self, &rhs: &RS) -> bool {
    let $ty(rhs): Self = rhs.into();
    self.0.eq(&rhs)
  }
}

impl<LB, RS> Add<RS> for $ty<LB>
  where LB: $bt,
        RS: Into<Self>,
        <LB as $bt>::$storage_assoc: Add<Output = <LB as $bt>::$storage_assoc>,
{
  type Output = Self;
  fn add(self, rhs: RS) -> Self {
    let $ty(rhs): Self = rhs.into();
    $ty(self.0.add(rhs))
  }
}
impl<LB, RS> AddAssign<RS> for $ty<LB>
  where LB: $bt,
        RS: Into<Self>,
        <LB as $bt>::$storage_assoc: AddAssign,
{
  fn add_assign(&mut self, rhs: RS) {
    let $ty(rhs): Self = rhs.into();
    self.0.add_assign(rhs)
  }
}
impl<LB, RS> Sub<RS> for $ty<LB>
  where LB: $bt,
        RS: Into<Self>,
        <LB as $bt>::$storage_assoc: Sub<Output = <LB as $bt>::$storage_assoc>,
{
  type Output = Self;
  fn sub(self, rhs: RS) -> Self {
    let $ty(rhs): Self = rhs.into();
    $ty(self.0.sub(rhs))
  }
}
impl<LB, RS> SubAssign<RS> for $ty<LB>
  where LB: $bt,
        RS: Into<Self>,
        <LB as $bt>::$storage_assoc: SubAssign,
{
  fn sub_assign(&mut self, rhs: RS) {
    let $ty(rhs): Self = rhs.into();
    self.0.sub_assign(rhs)
  }
}
impl<LB, RS> Mul<RS> for $ty<LB>
  where LB: $bt,
        RS: Into<Self>,
        <LB as $bt>::$storage_assoc: Mul<Output = <LB as $bt>::$storage_assoc>,
{
  type Output = Self;
  fn mul(self, rhs: RS) -> Self {
    let $ty(rhs): Self = rhs.into();
    $ty(self.0.mul(rhs))
  }
}
impl<LB, RS> MulAssign<RS> for $ty<LB>
  where LB: $bt,
        RS: Into<Self>,
        <LB as $bt>::$storage_assoc: MulAssign,
{
  fn mul_assign(&mut self, rhs: RS) {
    let $ty(rhs): Self = rhs.into();
    self.0.mul_assign(rhs)
  }
}
impl<LB, RS> Div<RS> for $ty<LB>
  where LB: $bt,
        RS: Into<Self>,
        <LB as $bt>::$storage_assoc: Div<Output = <LB as $bt>::$storage_assoc>,
{
  type Output = Self;
  fn div(self, rhs: RS) -> Self {
    let $ty(rhs): Self = rhs.into();
    $ty(self.0.div(rhs))
  }
}
impl<LB, RS> DivAssign<RS> for $ty<LB>
  where LB: $bt,
        RS: Into<Self>,
        <LB as $bt>::$storage_assoc: DivAssign,
{
  fn div_assign(&mut self, rhs: RS) {
    let $ty(rhs): Self = rhs.into();
    self.0.div_assign(rhs)
  }
}

impl<B> fmt::Debug for $ty<B>
  where B: $bt,
        <B as $bt>::$storage_assoc: fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    self.0.fmt(f)
  }
}
impl<B> fmt::Display for $ty<B>
  where B: $bt,
        <B as $bt>::$storage_assoc: fmt::Display,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    self.0.fmt(f)
  }
}

impl<B> $ty<B>
  where B: $bt,
{
  pub fn new_v(v: <B as $bt>::$storage_assoc) -> Self {
    $ty(v)
  }
}

  } // macro
}

impl_traits_for_scalar!(Real, RealBitWidth, RealStorageTy);
impl_traits_for_scalar!(UInt, IntBitWidth, UIntStorageTy);
impl_traits_for_scalar!(Int, IntBitWidth, IntStorageTy);

macro_rules! impl_traits_for {
  ($aty:ident, $bt:ident) => {

impl<T, N> Serialize for $aty<T, N>
  where N: $bt<T>,
        T: ScalarT,
        <N as $bt<T>>::InnerT: Serialize,
{
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer
  {
    self.0.serialize(serializer)
  }
}

impl<'a, T, N> Deserialize<'a> for $aty<T, N>
  where N: $bt<T>,
        T: ScalarT,
        <N as $bt<T>>::InnerT: for<'b> Deserialize<'b>,
{
  fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'a>>::Error>
    where D: Deserializer<'a>,
  {
    let inner: <N as $bt<T>>::InnerT =
      <<N as $bt<T>>::InnerT as Deserialize>::deserialize(deserializer)?;

    Ok($aty::new_v(inner))
  }
}

impl<T, N> Hash for $aty<T, N>
  where N: $bt<T>,
        T: ScalarT,
        <N as $bt<T>>::InnerT: Hash,
{
  fn hash<H>(&self, state: &mut H)
    where H: Hasher,
  {
    self.0.hash(state)
  }
}
impl<T, N> Default for $aty<T, N>
  where N: $bt<T>,
        T: ScalarT + Default,
{
  fn default() -> Self {
    Self::splat(T::default())
  }
}

impl<T, N> $aty<T, N>
  where N: $bt<T>,
        T: ScalarT,
{
  pub fn new_v(v: <N as $bt<T>>::InnerT) -> Self {
    $aty(v)
  }
  pub fn splat<U>(v: U) -> Self
    where U: Into<T>,
  {
    $aty(<N as $bt<T>>::splat(v))
  }

  /// In practice a rustc bug prevents this functions use.
  /// Doesn't fail to compile here though.
  pub fn cast<U>(self) -> $aty<U, N>
    where N: $bt<U>,
          U: ScalarT,
  {
    $aty::new_v(unsafe { self::simd_cast(self.0) })
  }
}

// MACRO END
  };
}

impl_traits_for!(Vec, VecN);
impl_traits_for!(Mat, MatN);

macro_rules! impl_from_for {
  ($ty:ty, $inner:ty) => {

impl From<$inner> for $ty {
  fn from(v: $inner) -> Self {
    unsafe { ::std::mem::transmute(v) }
  }
}
impl Into<$inner> for $ty {
  fn into(self) -> $inner {
    unsafe { ::std::mem::transmute(self) }
  }
}

impl ScalarT for $inner { }

  }
}

impl_from_for!(Real<N32<()>>, f32);
impl_from_for!(Real<N64<()>>, f64);
impl_from_for!(UInt<N8<()>>,  u8);
impl_from_for!(UInt<N16<()>>, u16);
impl_from_for!(UInt<N32<()>>, u32);
impl_from_for!(UInt<N64<()>>, u64);
impl_from_for!( Int<N8<()>>,  i8);
impl_from_for!( Int<N16<()>>, i16);
impl_from_for!( Int<N32<()>>, i32);
impl_from_for!( Int<N64<()>>, i64);

#[cfg(target_pointer_width = "64")]
impl_from_for!(UInt<N64<()>>, usize);
#[cfg(target_pointer_width = "32")]
impl_from_for!(UInt<N32<()>>, usize);
#[cfg(target_pointer_width = "64")]
impl_from_for!( Int<N64<()>>, isize);
#[cfg(target_pointer_width = "32")]
impl_from_for!( Int<N32<()>>, isize);

pub type R32 = Real<N32<()>>;
pub type R64 = Real<N64<()>>;
pub type U8  = UInt<N8<()>>;
pub type U16 = UInt<N16<()>>;
pub type U32 = UInt<N32<()>>;
pub type U64 = UInt<N64<()>>;
pub type I8  =  Int<N8<()>>;
pub type I16 =  Int<N16<()>>;
pub type I32 =  Int<N32<()>>;
pub type I64 =  Int<N64<()>>;

#[cfg(target_pointer_width = "64")]
pub type USize = UInt<N64<()>>;
#[cfg(target_pointer_width = "32")]
pub type USize = UInt<N32<()>>;
#[cfg(target_pointer_width = "64")]
pub type ISize =  Int<N64<()>>;
#[cfg(target_pointer_width = "32")]
pub type ISize =  Int<N32<()>>;

impl<T> Vec2<T>
  where T: ScalarT + Copy,
{
  pub fn x(&self) -> T {
    unsafe {
      self::simd_extract(self.0, 0)
    }
  }
  pub fn y(&self) -> T {
    unsafe {
      self::simd_extract(self.0, 1)
    }
  }
}
impl<T> Vec3<T>
  where T: ScalarT + Copy,
{
  pub fn x(&self) -> T {
    unsafe {
      self::simd_extract(self.0, 0)
    }
  }
  pub fn y(&self) -> T {
    unsafe {
      self::simd_extract(self.0, 1)
    }
  }
  pub fn z(&self) -> T {
    unsafe {
      self::simd_extract(self.0, 2)
    }
  }
}
impl<T> Vec4<T>
  where T: ScalarT + Copy,
{
  pub fn w(&self) -> T {
    unsafe {
      self::simd_extract(self.0, 0)
    }
  }
  pub fn x(&self) -> T {
    unsafe {
      self::simd_extract(self.0, 1)
    }
  }
  pub fn y(&self) -> T {
    unsafe {
      self::simd_extract(self.0, 2)
    }
  }
  pub fn z(&self) -> T {
    unsafe {
      self::simd_extract(self.0, 3)
    }
  }
}
// XXX Rust places big restrictions on generic parameters bounds when used
// in const functions. For now, we basically have to do this manually for
// each type.
impl Vec4<f32> {
  pub const fn new_c(v: [f32; 4]) -> Self {
    Vec(InnerVecN4(v[0], v[1], v[2], v[3]))
  }
}
impl Vec3<u32> {
  pub const fn new_c(v: [u32; 3]) -> Self {
    Vec(InnerVecN3(v[0], v[1], v[2]))
  }
  /// Kludge: multiple in-scope `new_c`
  pub const fn new_u32_c(v: [u32; 3]) -> Self { Self::new_c(v) }
}
impl Vec3<usize> {
  pub const fn new_c(v: [usize; 3]) -> Self {
    Vec(InnerVecN3(v[0], v[1], v[2]))
  }
  /// Kludge: multiple in-scope `new_c`
  pub const fn new_usize_c(v: [usize; 3]) -> Self { Self::new_c(v) }
}
