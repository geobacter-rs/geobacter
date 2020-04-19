use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Format<T, O> {
  pub ty: T,
  pub order: O,
}

impl Format<ChannelType, ChannelOrder> {
  #[inline(always)]
  pub(super) fn set_ffi(self, ffi: &mut ffi::hsa_ext_image_format_s) {
    let e = self.into_enum();
    ffi.channel_type = e.ty as _;
    ffi.channel_order = e.order as _;
  }
}

pub trait FormatDetail: FormatIntoEnum + Copy + Default + Send + Sync {
  type Type: ChannelTypeDetail;
  type Order: ChannelOrderDetail;

  type HostType: Copy;

  fn host_type_size() -> usize;

  #[doc(hidden)] #[inline(always)]
  fn set_ffi(self, ffi: &mut ffi::hsa_ext_image_format_s) {
    let e = self.into_enum();
    ffi.channel_type = e.ty as _;
    ffi.channel_order = e.order as _;
  }
}

// another hack so below can patter match
const fn two() -> usize { 2 }
const fn three() -> usize { 3 }
const fn four() -> usize { 4 }

// Const generic compiler bug: `type HostType = [T::MemoryType; O::HOST_CHANNELS];` can't be
// made to be always WF, so that errors no matter what. See
// https://github.com/rust-lang/rust/issues/68436.
macro_rules! impl_details {
  ($entry:ident {
    h_c: one,
  }) => {
    impl<T> FormatDetail for Format<T, $entry>
      where T: ChannelTypeDetail,
    {
      type Type = T;
      type Order = $entry;

      type HostType = T::MemoryType;

      fn host_type_size() -> usize {
        size_of::<Self::HostType>()
      }
    }
  };
  ($entry:ident {
    h_c: $h_c:ident,
  }) => {
    impl<T> FormatDetail for Format<T, $entry>
      where T: ChannelTypeDetail,
    {
      type Type = T;
      type Order = $entry;

      type HostType = [T::MemoryType; $h_c()];

      fn host_type_size() -> usize {
        size_of::<Self::HostType>()
      }
    }
  };
  (pub enum ChannelOrder {
    $($entry:ident {
      h_c: $h_c:ident,
    },)*
  }) => {$(
    impl_details!($entry {
      h_c: $h_c,
    });
  )*};
}
impl_details! {
  pub enum ChannelOrder {
    A { h_c: one, },
    R { h_c: one, },
    RX { h_c: one, },
    RG { h_c: two, },
    RGX { h_c: two, },
    RA { h_c: two, },
    RGB { h_c: three, },
    RGBX { h_c: three, },
    RGBA { h_c: four, },
    BGRA { h_c: four, },
    ARGB { h_c: four, },
    ABGR { h_c: four, },
    SRGB { h_c: three, },
    SRGBX { h_c: three, },
    SRGBA { h_c: four, },
    SBGRA { h_c: four, },
    Intensity { h_c: one, },
    Luminance { h_c: one, },
    Depth { h_c: one, },
    DepthStencil { h_c: two, },
  }
}

/// Damn "conflicting" `Into` implementations. Ugh.
pub trait FormatIntoEnum {
  fn into_enum(self) -> Format<ChannelType, ChannelOrder>;
}

impl<T, O> FormatIntoEnum for Format<T, O>
  where T: Into<ChannelType>,
        O: Into<ChannelOrder>,
{
  #[inline(always)]
  fn into_enum(self) -> Format<ChannelType, ChannelOrder> {
    let Format {
      ty, order,
    } = self;

    Format {
      ty: ty.into(),
      order: order.into(),
    }
  }
}
