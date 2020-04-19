decl! {
  #[repr(u32)]
  pub enum ChannelType {
    NormI8 = 0,
    NormI16,
    NormU8,
    NormU16,
    NormU24,
    UNormShort555,
    UNormShort565,
    UNormShort101010,
    I8,
    I16,
    I32,
    U8,
    U16,
    U32,
    // Imported f16, access f32: Rust has no f16 ATM, but maybe someday.
    F16F32,
    F32,
  }
}
macro_rules! impl_into {
  ($($prim:ty, $enum:ident, )*) => {$(
    impl Into<ChannelType> for $prim {
      #[inline(always)]
      fn into(self) -> ChannelType {
        ChannelType::$enum
      }
    }
  )*};
}
impl_into!(i8, I8, u8, U8, i16, I16, u16, U16, i32, I32, u32, U32,
           f32, F32, );

pub trait ChannelTypeDetail: Into<ChannelType> + Copy + Default + Send + Sync {
  #[inline(always)]
  fn into_enum(self) -> ChannelType { self.into() }

  /// The type imported/exported on the host.
  /// TODO: multiple options are permissible here too.
  type MemoryType: Copy;
}

macro_rules! impl_details {
  (pub enum $enum:ident {
    $($entry:ident {
      mem: $mem_ty:ty,
      k: $k_ty:ty,
    },)*
  }) => {$(
    impl ChannelTypeDetail for $entry {
      type MemoryType = $mem_ty;
    }
  )*};
}
impl_details! {
  pub enum ChannelType {
    // TODO: all these also allow f16
    NormI8 { mem: i8, k: f32, },
    NormI16 { mem: i16, k: f32, },
    NormU8 { mem: u8, k: f32, },
    NormU16 { mem: u16, k: f32, },
    NormU24 { mem: [u8; 3], k: f32, },
    UNormShort555 { mem: u16, k: f32, },
    UNormShort565 { mem: u16, k: f32, },
    UNormShort101010 { mem: u32, k: f32, },

    I8 { mem: i8, k: f32, },
    I16 { mem: i16, k: f32, },
    I32 { mem: i32, k: f32, },
    U8 { mem: u8, k: f32, },
    U16 { mem: u16, k: f32, },
    U32 { mem: u32, k: f32, },
    i8 { mem: i8, k: f32, },
    i16 { mem: i16, k: f32, },
    i32 { mem: i32, k: f32, },
    u8 { mem: u8, k: f32, },
    u16 { mem: u16, k: f32, },
    u32 { mem: u32, k: f32, },
    F16F32 {
      // TODO: should we use the `half` crate here?
      mem: u16,
      // TODO: add `f16` to Rust.
      k: f32,
    },
    F32 { mem: f32, k: f32, },
    f32 { mem: f32, k: f32, },
  }
}
