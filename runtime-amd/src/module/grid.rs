use std::fmt;
use std::ops::*;

use std::geobacter::amdgpu::workitem::*;

use num_traits::{AsPrimitive, ToPrimitive};
use num_traits::identities::{One, Zero, };
use num_traits::ops::{checked::*, saturating::*, };

use crate::Error;
use crate::texture::geometry::*;

/// None of these functions are intended to be used directly. `VectorParams` will
/// get you the workgroup/workitem/etc ids.
/// `Self` *must not* implement drop (but also doesn't need to be `Copy`).
pub unsafe trait GridDims: Sized + Clone + fmt::Debug {
  #[doc(hidden)]
  type Elem: Sized + Copy + fmt::Display + Default;
  #[doc(hidden)]
  type Idx: Copy;
  #[doc(hidden)]
  type Workgroup: WorkgroupDims;

  /// The grid size as given to the HSA packet.
  #[doc(hidden)]
  fn full_launch_grid(&self) -> Result<Dim3D<u32>, Error>;

  fn len(&self) -> Self::Idx;
  fn linear_len(&self) -> Option<Self::Elem>;

  #[doc(hidden)] #[inline(always)]
  fn workitem_id() -> <Self::Workgroup as WorkgroupDims>::Idx {
    Self::Workgroup::workitem_id()
  }
  #[doc(hidden)] fn workgroup_id() -> Self::Idx;
  #[doc(hidden)] fn workgroup_idx(wg_size: &Self::Workgroup, wg_id: &Self::Idx) -> Self::Idx;
  #[doc(hidden)] fn grid_id(&self,
                            wg_size: &Self::Workgroup,
                            wg_id: &Self::Idx,
                            wi_id: &<Self::Workgroup as WorkgroupDims>::Idx)
    -> Self::Idx;

  /// Return true if the current axis id is out of bounds. `self` is the requested grid size.
  #[doc(hidden)]
  fn grid_oob(&self,
              wg_size: &Self::Workgroup,
              wg_id: &Self::Idx,
              wi_id: &<Self::Workgroup as WorkgroupDims>::Idx) -> bool;

  #[doc(hidden)]
  fn global_linear_id(&self,
                      wg_size: &Self::Workgroup,
                      wg_id: &Self::Idx,
                      wi_id: &<Self::Workgroup as WorkgroupDims>::Idx)
    -> Self::Elem;
}

pub trait DimTranspose {
  fn transpose(self) -> Self;
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[derive(GeobacterDeps)]
pub struct Dim1D<T> {
  pub x: T,
}
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[derive(GeobacterDeps)]
pub struct Dim2D<T> {
  pub x: T,
  pub y: T,
}
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[derive(GeobacterDeps)]
pub struct Dim3D<T> {
  pub x: T,
  pub y: T,
  pub z: T,
}

pub trait RequireUpperBounds<T>: RangeBounds<T> { }
impl<T> RequireUpperBounds<T> for Range<T> { }
impl<T> RequireUpperBounds<T> for RangeInclusive<T> { }
impl<T> RequireUpperBounds<T> for RangeTo<T> { }
impl<T> RequireUpperBounds<T> for RangeToInclusive<T> { }

macro_rules! grid_impl {
  ($(($ty:ident, ($($field:ident,)*), ), )*) => {$(

    impl<T> $ty<T> {
      #[inline(always)]
      pub fn as_<U>(&self) -> $ty<U>
        where T: AsPrimitive<U>,
              U: Copy + 'static,
      {
        $ty {
         $($field: self.$field.as_(),)*
        }
      }
      #[inline(always)]
      pub fn checked_linear_len(&self) -> Result<T, Error>
        where T: CheckedMul + One,
      {
        let mut acc = T::one();
        $(acc = acc.checked_mul(&self.$field).ok_or(Error::Overflow)?;)*
        Ok(acc)
      }
    }
    impl<T> $ty<T> {
      #[inline(always)]
      pub fn start<U>(&self) -> $ty<U>
        where T: RangeBounds<U>,
              U: Zero + Copy + 'static,
      {
        $ty {
          $($field: match self.$field.start_bound() {
            Bound::Included(&v) => v,
            Bound::Excluded(_) => unreachable!(),
            Bound::Unbounded => U::zero(),
          },)*
        }
      }
      #[inline(always)]
      pub fn end<U>(&self) -> $ty<U>
        where T: RequireUpperBounds<U>,
              U: One + Saturating + Copy + 'static,
      {
        $ty {
          $($field: match self.$field.end_bound() {
            Bound::Included(v) => v.saturating_add(U::one()),
            Bound::Excluded(&v) => v,
            Bound::Unbounded => unreachable!(),
          },)*
        }
      }
      /// Used by the workgroup grid dim trait.
      #[inline(always)]
      pub fn _len<U>(&self) -> $ty<U>
        where T: RequireUpperBounds<U>,
              U: for<'a> Add<&'a U, Output = U> + for<'a> Sub<&'a U, Output = U>,
              U: Copy + One,
      {
        $ty {
          $($field: match (self.$field.start_bound(), self.$field.end_bound()) {
            (Bound::Included(&l), Bound::Included(&r)) => r - &l + &U::one(),
            (Bound::Included(&l), Bound::Excluded(&r)) => r - &l,
            // XXX possible overflow.
            (Bound::Unbounded, Bound::Included(&r)) => r + &U::one(),
            (Bound::Unbounded, Bound::Excluded(&r)) => r,
            _ => unreachable!(),
          },)*
        }
      }
      #[inline(always)]
      pub fn checked_len<U>(&self) -> Result<$ty<U>, Error>
        where T: RequireUpperBounds<U>,
              U: CheckedAdd + CheckedSub,
              U: Copy + One + Zero,
      {
        Ok($ty {
          $($field: match (self.$field.start_bound(), self.$field.end_bound()) {
            (Bound::Included(&l), Bound::Included(&r)) => {
              r.checked_sub(&l).ok_or(Error::Underflow)?
                .checked_add(&U::one()).ok_or(Error::Overflow)?
            },
            (Bound::Included(&l), Bound::Excluded(&r)) => {
              r.checked_sub(&l).ok_or(Error::Underflow)?
            },
            (Bound::Unbounded, Bound::Included(&r)) => {
              r.checked_add(&U::one()).ok_or(Error::Overflow)?
            },
            (Bound::Unbounded, Bound::Excluded(&r)) => r,
            _ => unreachable!(),
          },)*
        })
      }
    }
    impl<T> $ty<RangeTo<T>> {
      #[inline(always)]
      pub fn range_to_as<U>(&self) -> $ty<RangeTo<U>>
        where T: AsPrimitive<U>,
              U: Copy + 'static,
      {
        $ty {
         $($field: ..self.$field.end.as_(),)*
        }
      }
    }
    impl<T> One for $ty<T>
      where T: One,
    {
      #[inline(always)]
      fn one() -> Self {
        $ty {
          $($field: T::one(),)*
        }
      }
    }
    impl<T> Zero for $ty<T>
      where T: Zero,
    {
      #[inline(always)]
      fn is_zero(&self) -> bool {
        let mut c = true;
        $(c &= self.$field.is_zero();)*
        c
      }
      #[inline(always)]
      fn zero() -> Self {
        $ty {
          $($field: T::zero(),)*
        }
      }
    }

  )*}
}
grid_impl! {
  (Dim1D, (x, ), ),
  (Dim2D, (x, y, ), ),
  (Dim3D, (x, y, z, ), ),
}

impl<T> From<T> for Dim1D<T> {
  #[inline(always)]
  fn from(v: T) -> Self {
    Dim1D {
      x: v,
    }
  }
}
impl<T> From<T> for Dim2D<T>
  where T: Clone,
{
  #[inline(always)]
  fn from(v: T) -> Self {
    Dim2D {
      x: v.clone(),
      y: v,
    }
  }
}
impl<T> From<T> for Dim3D<T>
  where T: Clone,
{
  #[inline(always)]
  fn from(v: T) -> Self {
    Dim3D {
      x: v.clone(),
      y: v.clone(),
      z: v,
    }
  }
}
impl<T> DimTranspose for Dim1D<T> {
  #[inline(always)]
  fn transpose(self) -> Self {
    self
  }
}
impl<T> DimTranspose for Dim2D<T> {
  #[inline(always)]
  fn transpose(self) -> Self {
    Dim2D {
      x: self.y,
      y: self.x,
    }
  }
}
impl<T> DimTranspose for Dim3D<T> {
  #[inline(always)]
  fn transpose(self) -> Self {
    Dim3D {
      x: self.z,
      y: self.y,
      z: self.x,
    }
  }
}
impl<T> Dim1D<T> {
  #[inline(always)]
  fn glid(self, _end: Self) -> T {
    self.x
  }
  #[inline(always)]
  fn wi_linear_id(self, wi_id: Self) -> T {
    wi_id.x
  }
}
impl<T> Dim2D<T>
  where T: Add<T, Output = T> + Mul<T, Output = T> + Copy,
{
  #[inline(always)]
  fn glid(&self, end: Self) -> T {
    self.y * end.x + self.x
  }
  #[inline(always)]
  fn wi_linear_id(self, wi_id: Self) -> T {
    wi_id.y * self.x + wi_id.x
  }
}
impl<T> Dim3D<T>
  where T: Add<T, Output = T> + Mul<T, Output = T> + Copy,
{
  #[inline(always)]
  fn glid(&self, end: Self) -> T {
    (self.z * end.y + self.y) * end.x + self.x
  }
  #[inline(always)]
  fn wi_linear_id(self, wi_id: Self) -> T {
    (wi_id.z * self.y + wi_id.y) * self.x + wi_id.x
  }
}
impl<T> From<Dim1D<T>> for Dim3D<T>
  where T: One,
{
  #[inline(always)]
  fn from(v: Dim1D<T>) -> Self {
    Dim3D {
      x: v.x,
      y: One::one(),
      z: One::one(),
    }
  }
}
impl<T> From<Dim2D<T>> for Dim3D<T>
  where T: One,
{
  #[inline(always)]
  fn from(v: Dim2D<T>) -> Self {
    Dim3D {
      x: v.x,
      y: v.y,
      z: One::one(),
    }
  }
}
impl<T> Into<OneD<T>> for Dim1D<T> {
  #[inline(always)]
  fn into(self) -> OneD<T> {
    OneD {
      width: self.x,
    }
  }
}
impl<T> Into<TwoD<T>> for Dim2D<T> {
  #[inline(always)]
  fn into(self) -> TwoD<T> {
    TwoD {
      width: self.x,
      height: self.y,
    }
  }
}
impl<T> Into<ThreeD<T>> for Dim3D<T> {
  #[inline(always)]
  fn into(self) -> ThreeD<T> {
    ThreeD {
      width: self.x,
      height: self.y,
      depth: self.z,
    }
  }
}
impl<T> From<OneD<T>> for Dim1D<T> {
  #[inline(always)]
  fn from(v: OneD<T>) -> Self {
    Self {
      x: v.width,
    }
  }
}
impl<T> From<TwoD<T>> for Dim2D<T> {
  #[inline(always)]
  fn from(v: TwoD<T>) -> Self {
    Self {
      x: v.width,
      y: v.height,
    }
  }
}
impl<T> From<ThreeD<T>> for Dim3D<T> {
  #[inline(always)]
  fn from(v: ThreeD<T>) -> Self {
    Self {
      x: v.width,
      y: v.height,
      z: v.depth,
    }
  }
}

pub mod ops {
  use super::*;

  macro_rules! impl_math_op {
    ($(($t:ident, $tf:ident), )*) => ($(
      impl<T, U> $t<Dim1D<U>> for Dim1D<T>
        where T: $t<U>,
      {
        type Output = Dim1D<T::Output>;
        #[inline(always)]
        fn $tf(self, rhs: Dim1D<U>) -> Self::Output {
          Dim1D {
            x: $t::$tf(self.x, rhs.x),
          }
        }
      }
      impl<T, U> $t<Dim2D<U>> for Dim2D<T>
        where T: $t<U>,
      {
        type Output = Dim2D<T::Output>;
        #[inline(always)]
        fn $tf(self, rhs: Dim2D<U>) -> Self::Output {
          Dim2D {
            x: $t::$tf(self.x, rhs.x),
            y: $t::$tf(self.y, rhs.y),
          }
        }
      }
      impl<T, U> $t<Dim3D<U>> for Dim3D<T>
        where T: $t<U>,
      {
        type Output = Dim3D<T::Output>;
        #[inline(always)]
        fn $tf(self, rhs: Dim3D<U>) -> Self::Output {
          Dim3D {
            x: $t::$tf(self.x, rhs.x),
            y: $t::$tf(self.y, rhs.y),
            z: $t::$tf(self.z, rhs.z),
          }
        }
      }

      impl<'a, T> $t<Self> for &'a Dim1D<T>
        where &'a T: $t<&'a T, Output = T>,
      {
        type Output = Dim1D<T>;
        #[inline(always)]
        fn $tf(self, rhs: Self) -> Self::Output {
          Dim1D {
            x: $t::$tf(&self.x, &rhs.x),
          }
        }
      }
      impl<'a, T> $t<Self> for &'a Dim2D<T>
        where &'a T: $t<&'a T, Output = T>,
      {
        type Output = Dim2D<T>;
        #[inline(always)]
        fn $tf(self, rhs: Self) -> Self::Output {
          Dim2D {
            x: $t::$tf(&self.x, &rhs.x),
            y: $t::$tf(&self.y, &rhs.y),
          }
        }
      }
      impl<'a, T> $t<Self> for &'a Dim3D<T>
        where &'a T: $t<&'a T, Output = T>,
      {
        type Output = Dim3D<T>;
        #[inline(always)]
        fn $tf(self, rhs: Self) -> Self::Output {
          Dim3D {
            x: $t::$tf(&self.x, &rhs.x),
            y: $t::$tf(&self.y, &rhs.y),
            z: $t::$tf(&self.z, &rhs.z),
          }
        }
      }

      impl<'a, T> $t<Dim1D<T>> for &'a Dim1D<T>
        where T: 'a,
              &'a T: for<'b> $t<&'b T, Output = T>,
      {
        type Output = Dim1D<T>;
        #[inline(always)]
        fn $tf(self, rhs: Dim1D<T>) -> Self::Output {
          Dim1D {
            x: $t::$tf(&self.x, &rhs.x),
          }
        }
      }
      impl<'a, T> $t<Dim2D<T>> for &'a Dim2D<T>
        where T: 'a,
              &'a T: for<'b> $t<&'b T, Output = T>,
      {
        type Output = Dim2D<T>;
        #[inline(always)]
        fn $tf(self, rhs: Dim2D<T>) -> Self::Output {
          Dim2D {
            x: $t::$tf(&self.x, &rhs.x),
            y: $t::$tf(&self.y, &rhs.y),
          }
        }
      }
      impl<'a, T> $t<Dim3D<T>> for &'a Dim3D<T>
        where T: 'a,
              &'a T: for<'b> $t<&'b T, Output = T>,
      {
        type Output = Dim3D<T>;
        #[inline(always)]
        fn $tf(self, rhs: Dim3D<T>) -> Self::Output {
          Dim3D {
            x: $t::$tf(&self.x, &rhs.x),
            y: $t::$tf(&self.y, &rhs.y),
            z: $t::$tf(&self.z, &rhs.z),
          }
        }
      }

      impl<'a, T> $t<&'a Dim1D<T>> for Dim1D<T>
        where T: 'a,
              T: for<'b> $t<&'b T, Output = T>,
      {
        type Output = Dim1D<T>;
        #[inline(always)]
        fn $tf(self, rhs: &'a Dim1D<T>) -> Self::Output {
          Dim1D {
            x: $t::$tf(self.x, &rhs.x),
          }
        }
      }
      impl<'a, T> $t<&'a Dim2D<T>> for Dim2D<T>
        where T: 'a,
              T: for<'b> $t<&'b T, Output = T>,
      {
        type Output = Dim2D<T>;
        #[inline(always)]
        fn $tf(self, rhs: &'a Dim2D<T>) -> Self::Output {
          Dim2D {
            x: $t::$tf(self.x, &rhs.x),
            y: $t::$tf(self.y, &rhs.y),
          }
        }
      }
      impl<'a, T> $t<&'a Dim3D<T>> for Dim3D<T>
        where T: 'a,
              T: for<'b> $t<&'b T, Output = T>,
      {
        type Output = Dim3D<T>;
        #[inline(always)]
        fn $tf(self, rhs: &'a Dim3D<T>) -> Self::Output {
          Dim3D {
            x: $t::$tf(self.x, &rhs.x),
            y: $t::$tf(self.y, &rhs.y),
            z: $t::$tf(self.z, &rhs.z),
          }
        }
      }
    )* );
  }
  impl_math_op!((Add, add), (Sub, sub), (Mul, mul), (Div, div), (Rem, rem), );

  macro_rules! impl_math_checked_op {
    ($(($gty:ident, ($($t:ident, $tf:ident, ($($field:ident,)*), )*), ), )*) => ($($(
      impl<T> $t for $gty<T>
        where T: $t,
      {
        #[inline(always)]
        fn $tf(&self, rhs: &$gty<T>) -> Option<Self> {
          Some($gty {
            $($field: $t::$tf(&self.$field, &rhs.$field)?,)*
          })
        }
      }
    )*)*);
  }
  impl_math_checked_op! {
    (Dim1D,
     (CheckedAdd, checked_add, (x, ),
      CheckedSub, checked_sub, (x, ),
      CheckedMul, checked_mul, (x, ),
      CheckedDiv, checked_div, (x, ),
     ), ),
    (Dim2D,
     (CheckedAdd, checked_add, (x, y, ),
      CheckedSub, checked_sub, (x, y, ),
      CheckedMul, checked_mul, (x, y, ),
      CheckedDiv, checked_div, (x, y, ),
     ), ),
    (Dim3D,
     (CheckedAdd, checked_add, (x, y, z, ),
      CheckedSub, checked_sub, (x, y, z, ),
      CheckedMul, checked_mul, (x, y, z, ),
      CheckedDiv, checked_div, (x, y, z, ),
     ), ),
  }
}

macro_rules! grid_dims_impl {
  ($(
    ($gty:ident, $range_ty:ident,
      ($($field:ident, $axis_ty:ident, )*),
    ),
  )*) => {$(
    unsafe impl GridDims for $gty<$range_ty<u32>> {
      type Elem = u32;
      type Idx = $gty<u32>;
      type Workgroup = $gty<RangeTo<u16>>;

      #[doc(hidden)]
      #[inline(always)]
      fn full_launch_grid(&self) -> Result<Dim3D<u32>, Error> {
        Ok(self.checked_len()?.into())
      }

      #[inline(always)]
      fn len(&self) -> $gty<Self::Elem> {
        $gty {
          $($field: match (self.$field.start_bound(), self.$field.end_bound()) {
            (Bound::Included(&l), Bound::Included(&r)) => r - &l + &<Self::Elem as One>::one(),
            (Bound::Included(&l), Bound::Excluded(&r)) => r - &l,
            // XXX possible overflow.
            (Bound::Unbounded, Bound::Included(&r)) => r + &<Self::Elem as One>::one(),
            (Bound::Unbounded, Bound::Excluded(&r)) => r,
            _ => unreachable!(),
          },)*
        }
      }

      #[inline(always)]
      fn linear_len(&self) -> Option<Self::Elem> {
        let len = self.checked_len().ok()?;
        let mut acc: Self::Elem = One::one();
        $(acc *= len.$field;)*
        Some(acc)
      }

      #[doc(hidden)]
      #[inline(always)]
      fn workgroup_id() -> Self::Idx {
        $gty { $($field: $axis_ty.workgroup_id(),)* }
      }

      #[doc(hidden)]
      #[inline(always)]
      fn workgroup_idx(wg_size: &Self::Workgroup, wg_id: &Self::Idx) -> Self::Idx {
        let wg_size = wg_size.len().as_::<u32>();
        wg_id * wg_size
      }
      #[doc(hidden)]
      #[inline(always)]
      fn grid_id(&self,
                 wg_size: &Self::Workgroup,
                 wg_id: &Self::Idx,
                 wi_id: &<Self::Workgroup as WorkgroupDims>::Idx)
        -> Self::Idx
      {
        let wg_idx = Self::workgroup_idx(wg_size, wg_id);
        wg_idx + wi_id.as_::<u32>() + self.start()
      }

      #[doc(hidden)]
      #[inline(always)]
      fn grid_oob(&self,
                  wg_size: &Self::Workgroup,
                  wg_id: &Self::Idx,
                  wi_id: &<Self::Workgroup as WorkgroupDims>::Idx)
        -> bool
      {
        let idx = self.grid_id(wg_size, wg_id, wi_id);
        let end = self.end();
        $(idx.$field >= end.$field)||*
      }

      #[inline(always)]
      fn global_linear_id(&self,
                          wg_size: &Self::Workgroup,
                          wg_id: &Self::Idx,
                          wi_id: &<Self::Workgroup as WorkgroupDims>::Idx)
        -> Self::Elem
      {
        self.grid_id(wg_size, wg_id, wi_id).glid(self.end())
      }
    }
  )*}
}
grid_dims_impl! {
  (Dim1D, Range,
   (x, XAxis, ),
  ),
  (Dim1D, RangeInclusive,
   (x, XAxis, ),
  ),
  (Dim1D, RangeTo,
   (x, XAxis, ),
  ),
  (Dim1D, RangeToInclusive,
   (x, XAxis, ),
  ),
  (Dim2D, Range,
   (x, XAxis, y, YAxis, ),
  ),
  (Dim2D, RangeInclusive,
   (x, XAxis, y, YAxis, ),
  ),
  (Dim2D, RangeTo,
   (x, XAxis, y, YAxis, ),
  ),
  (Dim2D, RangeToInclusive,
   (x, XAxis, y, YAxis, ),
  ),
  (Dim3D, Range,
   (x, XAxis, y, YAxis, z, ZAxis, ),
  ),
  (Dim3D, RangeInclusive,
   (x, XAxis, y, YAxis, z, ZAxis, ),
  ),
  (Dim3D, RangeTo,
   (x, XAxis, y, YAxis, z, ZAxis, ),
  ),
  (Dim3D, RangeToInclusive,
   (x, XAxis, y, YAxis, z, ZAxis, ),
  ),
}
unsafe impl<'a, G> GridDims for &'a G
  where G: GridDims,
{
  #[doc(hidden)]
  type Elem = G::Elem;
  #[doc(hidden)]
  type Idx = G::Idx;
  #[doc(hidden)]
  type Workgroup = &'a G::Workgroup;

  /// The grid size as given to the HSA packet.
  #[doc(hidden)] #[inline(always)]
  fn full_launch_grid(&self) -> Result<Dim3D<u32>, Error> {
    (&**self).full_launch_grid()
  }

  #[inline(always)]
  fn len(&self) -> Self::Idx {
    (&**self).len()
  }
  #[inline(always)]
  fn linear_len(&self) -> Option<Self::Elem> {
    (&**self).linear_len()
  }

  #[doc(hidden)] #[inline(always)]
  fn workitem_id() -> <Self::Workgroup as WorkgroupDims>::Idx {
    G::Workgroup::workitem_id()
  }
  #[doc(hidden)] #[inline(always)]
  fn workgroup_id() -> Self::Idx {
    G::workgroup_id()
  }
  #[doc(hidden)] #[inline(always)]
  fn workgroup_idx(wg_size: &Self::Workgroup, wg_id: &Self::Idx) -> Self::Idx {
    G::workgroup_idx(wg_size, wg_id)
  }
  #[doc(hidden)] #[inline(always)]
  fn grid_id(&self,
             wg_size: &Self::Workgroup,
             wg_id: &Self::Idx,
             wi_id: &<Self::Workgroup as WorkgroupDims>::Idx)
    -> Self::Idx
  {
    (&**self).grid_id(wg_size, wg_id, wi_id)
  }

  /// Return true if the current axis id is out of bounds. `self` is the requested grid size.
  #[doc(hidden)] #[inline(always)]
  fn grid_oob(&self,
              wg_size: &Self::Workgroup,
              wg_id: &Self::Idx,
              wi_id: &<Self::Workgroup as WorkgroupDims>::Idx) -> bool
  {
    (&**self).grid_oob(wg_size, wg_id, wi_id)
  }

  #[doc(hidden)] #[inline(always)]
  fn global_linear_id(&self,
                      wg_size: &Self::Workgroup,
                      wg_id: &Self::Idx,
                      wi_id: &<Self::Workgroup as WorkgroupDims>::Idx)
    -> Self::Elem
  {
    (&**self).global_linear_id(wg_size, wg_id, wi_id)
  }
}

pub trait WorkgroupDims: Sized + Copy + fmt::Debug {
  #[doc(hidden)]
  type Elem: Sized + Copy + fmt::Display;
  #[doc(hidden)]
  type Idx: Copy;

  #[doc(hidden)] fn full_launch_grid(&self) -> Result<Dim3D<u16>, Error>;

  fn len(&self) -> Self::Idx;

  #[doc(hidden)] fn workitem_id() -> Self::Idx;
  #[doc(hidden)] fn workitem_linear_id(self, wi_id: Self::Idx) -> Self::Elem;
}
macro_rules! workgroup_dims_impl {
  ($(($gty:ident, $range:ident, $prim:ty, ($($field:ident, $axis_dim:ident, )*), ), )*) => {$(
    impl WorkgroupDims for $gty<$range<$prim>> {
      type Elem = $prim;
      type Idx = $gty<$prim>;

      #[doc(hidden)]
      #[inline(always)]
      fn full_launch_grid(&self) -> Result<Dim3D<u16>, Error> {
        let l = self._len();
        let l = $gty {
          $($field: l.$field.to_u16().ok_or(Error::Overflow)?,)*
        };
        Ok(l.into())
      }

      #[inline(always)]
      fn len(&self) -> Self::Idx {
        self._len()
      }

      #[doc(hidden)]
      #[inline(always)]
      fn workitem_id() -> Self::Idx {
        $gty {
          $($field: $axis_dim.workitem_id().as_(),)*
        }
      }
      #[doc(hidden)]
      #[inline(always)]
      fn workitem_linear_id(self, wi_id: Self::Idx) -> Self::Elem {
        self.len().wi_linear_id(wi_id)
      }
    }
  )*}
}
workgroup_dims_impl! {
  (Dim1D, RangeTo, u16, (x, XAxis, ), ),
  (Dim2D, RangeTo, u16, (x, XAxis, y, YAxis, ), ),
  (Dim3D, RangeTo, u16, (x, XAxis, y, YAxis, z, ZAxis, ), ),
}
impl<'a, G> WorkgroupDims for &'a G
  where G: WorkgroupDims + Clone,
{
  #[doc(hidden)]
  type Elem = G::Elem;
  #[doc(hidden)]
  type Idx = G::Idx;

  #[doc(hidden)] #[inline(always)]
  fn full_launch_grid(&self) -> Result<Dim3D<u16>, Error> {
    (&**self).full_launch_grid()
  }

  fn len(&self) -> Self::Idx {
    (&**self).len()
  }

  #[doc(hidden)] #[inline(always)]
  fn workitem_id() -> Self::Idx {
    G::workitem_id()
  }
  #[doc(hidden)] #[inline(always)]
  fn workitem_linear_id(self, wi_id: Self::Idx) -> Self::Elem {
    G::workitem_linear_id(self.clone(), wi_id)
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn is_zero_1d() {
    assert!(Dim1D {
      x: 0u32,
    }.is_zero());
    assert!(!Dim1D {
      x: 1u32,
    }.is_zero());
  }
  #[test]
  fn is_zero_2d() {
    assert!(Dim2D {
      x: 0u32,
      y: 0u32,
    }.is_zero());
    assert!(!Dim2D {
      x: 0u32,
      y: 1u32,
    }.is_zero());
    assert!(!Dim2D {
      x: 1u32,
      y: 0u32,
    }.is_zero());
  }
  #[test]
  fn is_zero_3d() {
    assert!(Dim3D {
      x: 0u32,
      y: 0u32,
      z: 0u32,
    }.is_zero());
    assert!(!Dim3D {
      x: 0u32,
      y: 1u32,
      z: 0u32,
    }.is_zero());
    assert!(!Dim3D {
      x: 1u32,
      y: 0u32,
      z: 0u32,
    }.is_zero());
    assert!(!Dim3D {
      x: 0u32,
      y: 0u32,
      z: 1u32,
    }.is_zero());
  }

  #[test]
  fn glid_2d_a() {
    let grid = Dim2D {
      x: 1..3,
      y: 1..3,
    };
    let wg_size = Dim2D {
      x: ..4u16,
      y: ..4,
    };
    let wg = Dim2D::zero();
    let wi = Dim2D::zero();

    let glid = grid.global_linear_id(&wg_size, &wg, &wi);
    assert_eq!(glid, 4);
  }
  #[test]
  fn grid_id_2d() {
    let grid = Dim2D {
      x: 1..3,
      y: 1..3,
    };
    let wg_size = Dim2D {
      x: ..4u16,
      y: ..4,
    };
    let wg = Dim2D::zero();
    let wi = Dim2D::zero();

    let grid_id = grid.grid_id(&wg_size, &wg, &wi);
    assert_eq!(grid_id, Dim2D {
      x: 1,
      y: 1,
    });
  }
  #[test]
  fn glid_2d_oob() {
    let grid = Dim2D {
      x: 1..3,
      y: 1..3,
    };
    let wg_size = Dim2D {
      x: ..4u16,
      y: ..4,
    };
    let wg = Dim2D::zero();
    let wi = Dim2D {
      x: 3,
      y: 3,
    };
    assert!(grid.grid_oob(&wg_size, &wg, &wi));
  }
}
