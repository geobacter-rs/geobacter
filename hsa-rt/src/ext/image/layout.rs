use super::*;

decl! {
  pub enum Layout {
    /// An implementation specific opaque image data layout which can vary depending on the
    /// agent, geometry, image format, image size, and access permissions.
    Opaque,
    /// The image data layout is specified by the following rules in ascending byte address order.
    /// For a 3D image, 2DA image array, or 1DA image array, the image data is stored as a linear
    /// sequence of adjacent 2D image slices, 2D images, or 1D images respectively, spaced
    /// according to the slice pitch. Each 2D image is stored as a linear sequence of adjacent
    /// image rows, spaced according to the row pitch. Each 1D or 1DB image is stored as a single
    /// image row. Each image row is stored as a linear sequence of image elements. Each image
    /// element is stored as a linear sequence of image components specified by the left to right
    /// channel order definition. Each image component is stored using the memory type specified by
    /// the channel type.
    ///
    /// The 1DB image geometry always uses the linear image data layout.
    Linear { row_pitch: Option<usize>, slice_pitch: Option<usize>, },
  }
  pub trait LayoutDetail: Default, Copy, ReadFirstLane, { }
}

impl ReadFirstLane for Opaque {
  #[inline(always)]
  unsafe fn read_first_lane(self) -> Self {
    self
  }
}
impl ReadFirstLane for Linear {
  #[inline(always)]
  unsafe fn read_first_lane(self) -> Self {
    Linear {
      row_pitch: self.row_pitch
        .map(|v| v.read_first_lane() ),
      slice_pitch: self.slice_pitch
        .map(|v| v.read_first_lane() ),
    }
  }
}

impl Default for Linear {
  #[inline(always)]
  fn default() -> Linear {
    Linear {
      row_pitch: None,
      slice_pitch: None,
    }
  }
}

impl Linear {
  #[inline]
  pub fn strides<G>(&self, region: &Region<G>) -> Result<(usize, usize), Error>
    where G: GeometryDetail,
  {
    let len = region.len()?;
    let width = len.width().try_into()
      .ok().ok_or(Error::Overflow)?;
    let row_stride = self.row_pitch
      .unwrap_or(width);
    let slice_stride = if let Some(p) = self.slice_pitch {
      p
    } else {
      len.height()
        .unwrap_or_else(<G::Elem as One>::one)
        .try_into()
        .ok().ok_or(Error::Overflow)?
    };

    Ok((row_stride, slice_stride))
  }
}
