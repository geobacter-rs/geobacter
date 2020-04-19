use super::*;
use hsa_rt::ext::image::format;

pub trait FormatDetail: format::FormatDetail + Copy + Default {
  type DefaultMask: Copy + Default + MaskDetail;
  type HostType: Copy = <Self as hsa_rt::ext::image::format::FormatDetail>::HostType;
}

macro_rules! impl_details {
  ($entry:ident {
    m: $m:ty,
  }) => {
    impl<T> FormatDetail for Format<T, $entry>
      where Self: format::FormatDetail,
            T: ChannelTypeDetail,
    {
      type DefaultMask = $m;
    }
  };
  (pub enum ChannelOrder {
    $($entry:ident {
      m: $m:ty,
    },)*
  }) => {$(
    impl_details!($entry {
      m: $m,
    });
  )*};
}
impl_details! {
  pub enum ChannelOrder {
    A { m: Ma, },
    R { m: Mr, },
    RX { m: Mr, },
    RG { m: Mrg, },
    RGX { m: Mrg, },
    RA { m: Mra, },
    RGB { m: Mrgb, },
    RGBX { m: Mrgb, },
    RGBA { m: Mrgba, },
    BGRA { m: Mrgba, },
    ARGB { m: Mrgba, },
    ABGR { m: Mrgba, },
    SRGB { m: Mrgb, },
    SRGBX { m: Mrgb, },
    SRGBA { m: Mrgba, },
    SBGRA { m: Mrgba, },
    Intensity { m: Mr, },
    Luminance { m: Mr, },
    Depth { m: Mr, },
    DepthStencil { m: Mrg, },
  }
}
