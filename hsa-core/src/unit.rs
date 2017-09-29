
//! A type that is held in whole in a register.
//! This is basically the primitive types.
//!

use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use serde::{Serialize, Serializer,
            Deserialize, Deserializer};

pub trait Nn {
  const N: usize;
}
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N1<T = ()>(PhantomData<T>);
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N2<T = ()>(PhantomData<T>);
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N3<T = ()>(PhantomData<T>);
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N4<T = ()>(PhantomData<T>);
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N8<T = ()>(PhantomData<T>);
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N16<T = ()>(PhantomData<T>);
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N24<T = ()>(PhantomData<T>);
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N32<T = ()>(PhantomData<T>);
#[derive(Clone, Copy, Serialize, Deserialize, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct N64<T = ()>(PhantomData<T>);

impl<T> Nn for N1<T> {
  const N: usize = 1;
}
impl<T> Nn for N2<T> {
  const N: usize = 2;
}
impl<T> Nn for N3<T> {
  const N: usize = 3;
}
impl<T> Nn for N4<T> {
  const N: usize = 4;
}
impl<T> Nn for N8<T> {
  const N: usize = 8;
}
impl<T> Nn for N16<T> {
  const N: usize = 16;
}
impl<T> Nn for N24<T> {
  const N: usize = 24;
}
impl<T> Nn for N32<T> {
  const N: usize = 32;
}
impl<T> Nn for N64<T> {
  const N: usize = 64;
}

pub trait VecN<T>: Nn
  where T: ScalarT,
{
  type InnerT: Copy + Debug;
}
impl<T> VecN<T> for N2<T>
  where T: ScalarT + Copy + Debug,
{
  type InnerT = [T; 2];
}
impl<T> VecN<T> for N3<T>
  where T: ScalarT + Copy + Debug,
{
  type InnerT = [T; 3];
}
impl<T> VecN<T> for N4<T>
  where T: ScalarT + Copy + Debug,
{
  type InnerT = [T; 4];
}
impl<T> VecN<T> for N8<T>
  where T: ScalarT + Copy + Debug,
{
  type InnerT = [T; 8];
}
impl<T> VecN<T> for N16<T>
  where T: ScalarT + Copy + Debug,
{
  type InnerT = [T; 16];
}

#[hsa_lang_type = "vec"]
#[derive(Clone, Copy, Debug)]
pub struct Vec<T, N>(pub(crate) <N as VecN<T>>::InnerT)
  where N: VecN<T>,
        T: ScalarT;
pub type Vec2<T> = Vec<T, N2<T>>;
pub type Vec3<T> = Vec<T, N3<T>>;
pub type Vec4<T> = Vec<T, N4<T>>;
pub type Vec8<T> = Vec<T, N8<T>>;
pub type Vec16<T> = Vec<T, N16<T>>;

pub trait MatN<T>: Nn {
  type InnerT: Copy + Debug;
}
impl<T> MatN<T> for N2<T>
  where T: Copy + Debug,
{
  type InnerT = [[T; 2]; 2];
}
impl<T> MatN<T> for N3<T>
  where T: Copy + Debug,
{
  type InnerT = [[T; 3]; 3];
}
impl<T> MatN<T> for N4<T>
  where T: Copy + Debug,
{
  type InnerT = [[T; 4]; 4];
}

#[hsa_lang_type = "mat"]
#[derive(Clone, Copy, Debug)]
pub struct Mat<T, N>(pub(crate) <N as MatN<T>>::InnerT)
  where N: MatN<T>,
        T: ScalarT;

pub trait RealBitWidth: Nn {
  type RealStorageTy: Copy + Serialize + Deserialize + PartialOrd + PartialEq + Debug;
}
impl RealBitWidth for N16 {
  type RealStorageTy = u16;
}
impl RealBitWidth for N24 {
  type RealStorageTy = [u8; 3];
}
impl RealBitWidth for N32 {
  type RealStorageTy = f32;
}
impl RealBitWidth for N64 {
  type RealStorageTy = f64;
}

#[hsa_lang_type = "real"]
#[derive(Clone, Copy)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct Real<Bits>(pub(crate) Bits::RealStorageTy)
  where Bits: RealBitWidth;

pub trait RealT { }
impl<Bits> RealT for Real<Bits>
  where Bits: RealBitWidth,
{ }

pub trait IntBitWidth: Nn {
  type UIntStorageTy: Copy + Serialize + Deserialize + Hash + PartialOrd + PartialEq + Ord + Eq + Debug;
  type IntStorageTy: Copy + Serialize + Deserialize + Hash + PartialOrd + PartialEq + Ord + Eq + Debug;
}
impl IntBitWidth for N8 {
  type UIntStorageTy = u8;
  type IntStorageTy  = i8;
}
impl IntBitWidth for N16 {
  type UIntStorageTy = u16;
  type IntStorageTy  = i16;
}
impl IntBitWidth for N32 {
  type UIntStorageTy = u32;
  type IntStorageTy  = i32;
}
impl IntBitWidth for N64 {
  type UIntStorageTy = u64;
  type IntStorageTy  = i64;
}

#[hsa_lang_type = "uint"]
#[derive(Clone, Copy, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct UInt<Bits>(pub(crate) Bits::UIntStorageTy)
  where Bits: IntBitWidth;
#[hsa_lang_type = "int"]
#[derive(Clone, Copy, Hash)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct Int<Bits>(pub(crate) Bits::IntStorageTy)
  where Bits: IntBitWidth;

pub trait ScalarT { }
impl ScalarT for Real<N16> { }
impl ScalarT for Real<N32> { }
impl ScalarT for Real<N24> { }
impl ScalarT for Real<N64> { }
impl ScalarT for UInt<N8>  { }
impl ScalarT for UInt<N16> { }
impl ScalarT for UInt<N32> { }
impl ScalarT for UInt<N64> { }
impl ScalarT for Int<N8>   { }
impl ScalarT for Int<N16>  { }
impl ScalarT for Int<N32>  { }
impl ScalarT for Int<N64>  { }

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

impl<B> Deserialize for $ty<B>
  where B: $bt,
        <B as $bt>::$storage_assoc: Deserialize,
{
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer,
  {
    let inner: <B as $bt>::$storage_assoc =
      <<B as $bt>::$storage_assoc as Deserialize>::deserialize(deserializer)?;

    Ok($ty::new_v(inner))
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

impl<T, N> Deserialize for $aty<T, N>
  where N: $bt<T>,
        T: ScalarT,
        <N as $bt<T>>::InnerT: Deserialize,
{
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer,
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

impl<T, N> $aty<T, N>
  where N: $bt<T>,
        T: ScalarT,
{
  pub fn new_v(v: <N as $bt<T>>::InnerT) -> Self {
    $aty(v)
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

  }
}

impl_from_for!(Real<N32>, f32);
impl_from_for!(Real<N64>, f64);
impl_from_for!(UInt<N8>,  u8);
impl_from_for!(UInt<N16>, u16);
impl_from_for!(UInt<N32>, u32);
impl_from_for!(UInt<N64>, u64);
impl_from_for!( Int<N8>,  i8);
impl_from_for!( Int<N16>, i16);
impl_from_for!( Int<N32>, i32);
impl_from_for!( Int<N64>, i64);
