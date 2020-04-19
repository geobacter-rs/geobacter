decl! {
  #[repr(u32)]
  pub enum ChannelOrder {
    A = 0,
    R,
    RX,
    RG,
    RGX,
    RA,
    RGB,
    RGBX,
    RGBA,
    BGRA,
    ARGB,
    ABGR,
    SRGB,
    SRGBX,
    SRGBA,
    SBGRA,
    Intensity,
    Luminance,
    Depth,
    DepthStencil,
  }
}

pub trait ChannelOrderDetail: Into<ChannelOrder> + Copy + Default + Send + Sync {
  const HOST_CHANNELS: usize;
  const CODEGEN_CHANNELS: usize;

  #[inline(always)]
  fn into_enum(self) -> ChannelOrder { self.into() }
}
macro_rules! impl_details {
  (pub enum $enum:ident {
    $($entry:ident $({
      h_c: $h_c:expr,
      k_c: $k_c:expr,
    })*,)*
  }) => {$($(
    impl ChannelOrderDetail for $entry {
      const HOST_CHANNELS: usize = $h_c;
      const CODEGEN_CHANNELS: usize = $k_c;
    }
  )*)*};
}
impl_details! {
  pub enum ChannelOrder {
    A { h_c: 1, k_c: 4, },
    R { h_c: 1, k_c: 4, },
    RX { h_c: 1, k_c: 4, },
    RG { h_c: 2, k_c: 4, },
    RGX { h_c: 2, k_c: 4, },
    RA { h_c: 2, k_c: 4, },
    RGB { h_c: 3, k_c: 4, },
    RGBX { h_c: 3, k_c: 4, },
    RGBA { h_c: 4, k_c: 4, },
    BGRA { h_c: 4, k_c: 4, },
    ARGB { h_c: 4, k_c: 4, },
    ABGR { h_c: 4, k_c: 4, },
    SRGB { h_c: 3, k_c: 4, },
    SRGBX { h_c: 3, k_c: 4, },
    SRGBA { h_c: 4, k_c: 4, },
    SBGRA { h_c: 4, k_c: 4, },
    Intensity { h_c: 1, k_c: 4, },
    Luminance { h_c: 1, k_c: 4, },
    Depth { h_c: 1, k_c: 1, },
    DepthStencil { h_c: 2, k_c: 4, },
  }
}
