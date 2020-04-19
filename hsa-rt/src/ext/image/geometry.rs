use super::*;

decl! {
  pub enum Geometry<T> {
    /// One-dimensional image addressed by width coordinate.
    OneD<T> { width: T, },
    /// Two-dimensional image addressed by width and height coordinates.
    TwoD<T> { width: T, height: T, },
    /// Three-dimensional image addressed by width, height, and depth coordinates.
    ThreeD<T> { width: T, height: T, depth: T, },
    /// Array of one-dimensional images with the same size and format. 1D arrays are addressed
    /// by width and index coordinates.
    OneDArray<T> { width: T, array_size: T, },
    /// Array of two-dimensional images with the same size and format. 2D arrays are addressed
    /// by width, height, and index coordinates.
    TwoDArray<T> { width: T, height: T, array_size: T, },
    /// One-dimensional image addressed by width coordinate. It has specific restrictions
    /// compared to Geometry::OneD. An image with an opaque image data layout will always use a
    /// linear image data layout, and one with an explicit image data layout must specify
    /// HSA_EXT_IMAGE_DATA_LAYOUT_LINEAR.
    OneDB<T> { width: T, },
  }
}

pub trait GeometryDetail: Sized + Into<Region<Self>> + Add + Sub + Copy + PartialOrd
  where Self: CheckedAdd + CheckedSub + One + Zero + Send + Sync,
{
  type Elem: Copy + CheckedMul + One + TryInto<u32> + TryInto<usize>;
  type Idx;

  fn into_enum(self) -> Geometry<Self::Elem>;

  #[doc(hidden)]
  fn hsa_dim(&self, ffi: &mut ffi::hsa_dim3_t) -> Result<(), Error>;

  #[inline(always)]
  fn set_ffi(&self, ffi: &mut ffi::hsa_ext_image_descriptor_s)
    -> Result<(), Error>
  {
    self.into_enum().set_ffi(ffi)
  }

  #[inline(always)]
  fn len(&self) -> Result<Self::Elem, Error> {
    let h = self.height().unwrap_or_else(<Self::Elem as One>::one);
    let d = self.depth().unwrap_or_else(<Self::Elem as One>::one);
    self.width()
      .checked_mul(&h)
      .ok_or(Error::Overflow)?
      .checked_mul(&d)
      .ok_or(Error::Overflow)
  }

  fn width(&self) -> Self::Elem;
  fn height(&self) -> Option<Self::Elem>;
  fn depth(&self) -> Option<Self::Elem>;
}

macro_rules! impl_detail {
  ($(#[$outer_attrs:meta])*
    pub enum $enum:ident {
      $($entry:ident <$idx:ty> { $($fields:ident: $field_dim:ident,)* },)*
    }
  ) => {$(
    impl<T> GeometryDetail for $entry<T>
      where T: Copy + From<u8> + Add + Sub + PartialOrd + CheckedMul,
            T: CheckedAdd + CheckedSub + One + Zero + Send + Sync,
            T: TryInto<u32> + TryInto<usize>,
    {
      type Elem = T;
      type Idx = $idx;

      #[inline(always)]
      fn into_enum(self) -> Geometry<Self::Elem> { self.into() }

      #[doc(hidden)]
      fn hsa_dim(&self, ffi: &mut ffi::hsa_dim3_t) -> Result<(), Error> {
        $(
          ffi.$field_dim = self.$fields.clone()
            .try_into()
            .ok()
            .ok_or(Error::Overflow)?;
        )*
        Ok(())
      }

      #[inline(always)]
      fn width(&self) -> T {
        self.width
      }
      #[allow(unused_variables, unused_mut, dead_code)]
      #[inline(always)]
      fn height(&self) -> Option<T> {
        /// a macro hack:
        struct TT<T> {
          x: Option<T>,
          y: Option<T>,
          z: Option<T>,
        }
        let mut tt = TT { x: None, y: None, z: None, };

        $(tt.$field_dim = Some(self.$fields);)*

        tt.y
      }
      #[allow(unused_variables, unused_mut, dead_code)]
      #[inline(always)]
      fn depth(&self) -> Option<T> {
        /// a macro hack:
        struct TT<T> {
          x: Option<T>,
          y: Option<T>,
          z: Option<T>,
        }
        let mut tt = TT { x: None, y: None, z: None, };

        $(tt.$field_dim = Some(self.$fields);)*

        tt.z
      }
    }

    impl<T> ReadFirstLane for $entry<T>
      where T: ReadFirstLane,
    {
      #[inline(always)]
      unsafe fn read_first_lane(self) -> Self {
        $entry {
          $($fields: self.$fields.read_first_lane(),)*
        }
      }
    }

    impl<T> Into<Region<Self>> for $entry<T>
      where T: Zero,
    {
      #[inline(always)]
      fn into(self) -> Region<Self> {
        Region {
          offset: Self::zero(),
          range: self,
        }
      }
    }

    impl<T> One for $entry<T>
      where T: One,
    {
      #[inline(always)]
      fn one() -> Self {
        $entry {
          $($fields: T::one(),)*
        }
      }
    }
    impl<T> Zero for $entry<T>
      where T: Zero,
    {
      #[inline(always)]
      fn zero() -> Self {
        $entry {
          $($fields: T::zero(),)*
        }
      }
      #[inline(always)]
      fn is_zero(&self) -> bool {
        let mut c = true;
        $(c &= self.$fields.is_zero();)*
        c
      }
    }

    impl<T> CheckedAdd for $entry<T>
      where T: CheckedAdd,
    {
      #[inline(always)]
      fn checked_add(&self, rhs: &Self) -> Option<Self> {
        Some($entry {
          $($fields: T::checked_add(&self.$fields, &rhs.$fields)?,)*
        })
      }
    }
    impl<T> CheckedSub for $entry<T>
      where T: CheckedSub,
    {
      #[inline(always)]
      fn checked_sub(&self, rhs: &Self) -> Option<Self> {
        Some($entry {
          $($fields: T::checked_sub(&self.$fields, &rhs.$fields)?,)*
        })
      }
    }

    impl<T> Add for $entry<T>
      where T: Add,
    {
      type Output = $entry<T::Output>;
      #[inline(always)]
      fn add(self, rhs: Self) -> Self::Output {
        $entry {
          $($fields: T::add(self.$fields, rhs.$fields),)*
        }
      }
    }
    // Require for `One` from num_traits..
    impl<T> Mul for $entry<T>
      where T: Mul,
    {
      type Output = $entry<T::Output>;
      #[inline(always)]
      fn mul(self, rhs: Self) -> Self::Output {
        $entry {
          $($fields: T::mul(self.$fields, rhs.$fields),)*
        }
      }
    }
    impl<T> Sub for $entry<T>
      where T: Sub,
    {
      type Output = $entry<T::Output>;
      #[inline(always)]
      fn sub(self, rhs: Self) -> Self::Output {
        $entry {
          $($fields: T::sub(self.$fields, rhs.$fields),)*
        }
      }
    }
  )*};
}
impl_detail! {
  pub enum Geometry {
    OneD<u32> { width: x, },
    TwoD<(u32, u32)> { width: x, height: y, },
    ThreeD<(u32, u32, u32)> { width: x, height: y, depth: z, },
    OneDArray<(u32, u32)> { width: x, array_size: y, },
    TwoDArray<(u32, u32, u32)> { width: x, height: y, array_size: z, },
    OneDB<u32> { width: x, },
  }
}

impl<T> Geometry<T>
  where T: TryInto<usize> + Clone,
{
  pub(super) fn set_ffi(&self, ffi: &mut ffi::hsa_ext_image_descriptor_s)
    -> Result<(), Error>
  {
    ffi.width = 0;
    ffi.height = 0;
    ffi.depth = 0;
    ffi.array_size = 0;

    match self {
      Geometry::OneD { width, } => {
        ffi.geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_1D;
        ffi.width = width.clone().try_into()
          .ok().ok_or(Error::Overflow)?;
      },
      Geometry::TwoD { width, height, } => {
        ffi.geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_2D;
        ffi.width = width.clone().try_into()
          .ok().ok_or(Error::Overflow)?;
        ffi.height = height.clone().try_into()
          .ok().ok_or(Error::Overflow)?;
      },
      Geometry::ThreeD { width, height, depth, } => {
        ffi.geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_3D;
        ffi.width = width.clone().try_into()
          .ok().ok_or(Error::Overflow)?;
        ffi.height = height.clone().try_into()
          .ok().ok_or(Error::Overflow)?;
        ffi.depth = depth.clone().try_into()
          .ok().ok_or(Error::Overflow)?;
      },
      Geometry::OneDArray { width, array_size, } => {
        ffi.geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_1DA;
        ffi.width = width.clone().try_into()
          .ok().ok_or(Error::Overflow)?;
        ffi.array_size = array_size.clone().try_into()
          .ok().ok_or(Error::Overflow)?;
      },
      Geometry::TwoDArray { width, height, array_size, } => {
        ffi.geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_2DA;
        ffi.width = width.clone().try_into()
          .ok().ok_or(Error::Overflow)?;
        ffi.height = height.clone().try_into()
          .ok().ok_or(Error::Overflow)?;
        ffi.array_size = array_size.clone().try_into()
          .ok().ok_or(Error::Overflow)?;
      },
      Geometry::OneDB { width, } => {
        ffi.geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_1DB;
        ffi.width = width.clone().try_into()
          .ok().ok_or(Error::Overflow)?;
      },
    }

    Ok(())
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Region<G> {
  pub offset: G,
  pub range: G,
}

impl<G> Region<G>
  where G: GeometryDetail,
{
  pub fn len(&self) -> Result<G, Error> {
    self.range.checked_sub(&self.offset)
      .ok_or(Error::Underflow)
  }
}

impl<G> TryInto<ffi::hsa_ext_image_region_t> for Region<G>
  where G: GeometryDetail,
{
  type Error = Error;
  fn try_into(self) -> Result<ffi::hsa_ext_image_region_t, Error> {
    let mut ffi_region = ffi::hsa_ext_image_region_t {
      offset: ffi::hsa_dim3_t {
        x: 0, y: 0, z: 0,
      },
      range: ffi::hsa_dim3_t {
        x: 1, y: 1, z: 1,
      },
    };
    self.offset.hsa_dim(&mut ffi_region.offset)?;
    self.range.hsa_dim(&mut ffi_region.range)?;

    Ok(ffi_region)
  }
}
