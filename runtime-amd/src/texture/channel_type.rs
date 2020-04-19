pub use hsa_rt::ext::image::channel_type::*;

pub trait AccessTypeDetail<T> {
  #[doc(hidden)]
  type RawPixel;
  #[doc(hidden)]
  unsafe fn map_load(raw: Self::RawPixel) -> T;
  #[doc(hidden)]
  unsafe fn map_store(p: T) -> Self::RawPixel;
}
macro_rules! impl_access_ty_v4f32 {
  ($((($($channel_ty:ty $(=> $inter:ty)?,)*), $pixel_ty:ty), )*) => {$($(
    impl AccessTypeDetail<[$pixel_ty; 4]> for $channel_ty {
      #[doc(hidden)]
      type RawPixel = [f32; 4];
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn map_load(v: Self::RawPixel) -> [$pixel_ty; 4] {
        #![allow(unreachable_code)]
        $(
          let o: [$inter; 4] = std::mem::transmute(v);
          return [o[0] as _, o[1] as _, o[2] as _, o[3] as _];
        )?
        std::mem::transmute_copy(&v)
      }
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn map_store(p: [$pixel_ty; 4]) -> Self::RawPixel {
        #![allow(unreachable_code)]
        $(
          let v = [p[0] as $inter, p[1] as _, p[2] as _, p[3] as _];
          return std::mem::transmute(v);
        )?
        std::mem::transmute_copy(&p)
      }
    }
  )*)*}
}
impl_access_ty_v4f32! {
  ((NormI8, NormI16, NormU8, NormU16, NormU24,
    UNormShort555, UNormShort565, UNormShort101010, ), f32),
  ((I8 => i32, i8 => i32, ), i8),
  ((I16 => i32, i16 => i32, ), i16),
  ((I32, i32, ), i32),
  ((U8 => u32, u8 => u32, ), u8),
  ((U16 => u32, u16 => u32, ), u16),
  ((U32, u32, ), u32),
  ((F16F32, F32, f32, ), f32),
}
macro_rules! impl_access_ty_f32 {
  ($((($($channel_ty:ty $(=> $inter:ty)?,)*), $pixel_ty:ty), )*) => {$($(
    impl AccessTypeDetail<[$pixel_ty; 1]> for $channel_ty {
      #[doc(hidden)]
      type RawPixel = [f32; 1];
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn map_load(v: Self::RawPixel) -> [$pixel_ty; 1] {
        #![allow(unreachable_code)]
        $(
          let o: [$inter; 1] = std::mem::transmute(v);
          return [o[0] as _; 1];
        )?
        std::mem::transmute_copy(&v)
      }
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn map_store(p: [$pixel_ty; 1]) -> Self::RawPixel {
        #![allow(unreachable_code)]
        $(
          let v = [p[0] as $inter; 1];
          return std::mem::transmute(v);
        )?
        std::mem::transmute_copy(&p)
      }
    }
    impl AccessTypeDetail<$pixel_ty> for $channel_ty {
      #[doc(hidden)]
      type RawPixel = [f32; 1];
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn map_load(v: Self::RawPixel) -> $pixel_ty {
        #![allow(unreachable_code)]
        $(
          let o: [$inter; 1] = std::mem::transmute(v);
          return o[0] as _;
        )?
        std::mem::transmute_copy(&v)
      }
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn map_store(p: $pixel_ty) -> Self::RawPixel {
        #![allow(unreachable_code)]
        $(
          let v = [p as $inter; 1];
          return std::mem::transmute(v);
        )?
        std::mem::transmute_copy(&p)
      }
    }
  )*)*}
}
impl_access_ty_f32! {
  ((NormI8, NormI16, NormU8, NormU16, NormU24,
    UNormShort555, UNormShort565, UNormShort101010, ), f32),
  ((I8 => i32, i8 => i32, ), i8),
  ((I16 => i32, i16 => i32, ), i16),
  ((I32, i32, ), i32),
  ((U8 => u32, u8 => u32, ), u8),
  ((U16 => u32, u16 => u32, ), u16),
  ((U32, u32, ), u32),
  ((F16F32, F32, f32, ), f32),
}
