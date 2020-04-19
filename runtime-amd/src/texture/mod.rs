//! The HSA image extension leaves a lot to be desired. All of it's operations are synchronous.
//! It leaves most "bad behaviour" to UB land. I've crashed Linux using these APIs.
//!
//! Contrary to what you might think, masked channels always write `0`, instead of writing nothing!
//! Which is unfortunate, IMO.

#![allow(improper_ctypes)]

use std::convert::*;
use std::mem::transmute_copy;
use std::ops::*;

use hsa_rt::ext::amd::MemoryPoolAlloc;
use hsa_rt::ext::image::access::*;
use hsa_rt::ext::image::geometry::*;
use hsa_rt::ext::image::channel_order::*;
use hsa_rt::ext::image::layout::*;

use num_traits::identities::{One, Zero, };
use num_traits::ops::checked::*;

use crate::{HsaAmdGpuAccel, Error};

use self::channel_mask::*;
use self::channel_type::*;
use self::format::*;

pub use hsa_rt::ext::image::*;
pub use self::channel_mask::{Mask, Y, N, All as AllMask, };

pub type ImageData = hsa_rt::ext::image::ImageData<MemoryPoolAlloc>;

/// The image load/store/etc pixel types are vectors.
#[repr(simd)]
#[derive(Clone, Copy)]
struct T4<T>(T, T, T, T);

impl<T> Into<[T; 4]> for T4<T> {
  #[inline(always)]
  fn into(self) -> [T; 4] {
    unsafe { transmute_copy(&self) }
  }
}
impl<T> From<[T; 4]> for T4<T> {
  #[inline(always)]
  fn from(v: [T; 4]) -> Self {
    unsafe { transmute_copy(&v) }
  }
}

/// Internal.
#[doc(hidden)]
pub trait ImageOps<T, M>: GeometryDetail + Sized
  where M: MaskDetail,
{
  #[doc(hidden)]
  unsafe fn raw_load(hndl: AmdImageResDesc, idx: Self::Idx) -> T;
  #[doc(hidden)]
  unsafe fn raw_store(hndl: AmdImageResDesc, idx: Self::Idx, v: T);
}
macro_rules! impl_v4f32_image_load_store {
  ($mask_ty:ty, $entry:ident <$idx:ident: $idx_ty:ty,>
    { load: $l_intrinsic:literal, store: $s_intrinsic:literal, }
  ) => {
    impl<T> ImageOps<[f32; 4], $mask_ty> for geometry::$entry<T>
      where T: Copy + From<u8> + Add + Sub + PartialOrd + CheckedMul,
            T: CheckedAdd + CheckedSub + One + Zero + Send + Sync,
            T: TryInto<u32> + TryInto<usize>,
    {
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn raw_load(h: AmdImageResDesc, $idx: Self::Idx) -> [f32; 4] {
        extern "C" {
          #[link_name = $l_intrinsic]
          fn image_load(dmask: u32, $idx: u32, h: AmdImageResDesc,
                        texfailctrl: i32, cachepolicy: i32) -> T4<f32>;
        }
        <$mask_ty>::unshift(image_load(<$mask_ty>::MASK, $idx as _, h, 0, 0).into())
      }
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn raw_store(h: AmdImageResDesc, $idx: Self::Idx, v: [f32; 4]) {
        extern "C" {
          #[link_name = $s_intrinsic]
          fn image_store(p: T4<f32>, dmask: u32, $idx: u32, h: AmdImageResDesc,
                         texfailctrl: u32, cachepolicy: u32);
        }
        image_store(<$mask_ty>::shift(v).into(), <$mask_ty>::MASK, $idx as _, h,
                    0, 0)
      }
    }
  };
  ($mask_ty:ty, $entry:ident <$($idx:ident: $idx_ty:ty,)+>
    { load: $l_intrinsic:literal, store: $s_intrinsic:literal, }
  ) => {
    impl<T> ImageOps<[f32; 4], $mask_ty> for geometry::$entry<T>
      where T: Copy + From<u8> + Add + Sub + PartialOrd + CheckedMul,
            T: CheckedAdd + CheckedSub + One + Zero + Send + Sync,
            T: TryInto<u32> + TryInto<usize>,
    {
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn raw_load(h: AmdImageResDesc, ($($idx,)*): Self::Idx) -> [f32; 4] {
        extern "C" {
          #[link_name = $l_intrinsic]
          fn image_load(dmask: u32, $($idx: u32,)* h: AmdImageResDesc,
                        texfailctrl: i32, cachepolicy: i32) -> T4<f32>;
        }
        <$mask_ty>::unshift(image_load(<$mask_ty>::MASK, $($idx as _,)* h, 0, 0).into())
      }
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn raw_store(h: AmdImageResDesc, ($($idx,)*): Self::Idx, v: [f32; 4]) {
        extern "C" {
          #[link_name = $s_intrinsic]
          fn image_store(p: T4<f32>, dmask: u32, $($idx: u32,)* h: AmdImageResDesc,
                         texfailctrl: u32, cachepolicy: u32);
        }
        image_store(<$mask_ty>::shift(v).into(), <$mask_ty>::MASK, $($idx as _,)* h,
                    0, 0)
      }
    }
  };
  ($mask_ty:ty, {
    $(
      $entry:ident <$($idx:ident: $idx_ty:ty,)*>
      { load: $l_intrinsic:literal, store: $s_intrinsic:literal, },
    )*
  }) => {$(
    impl_v4f32_image_load_store!($mask_ty, $entry <$($idx: $idx_ty,)*>
      { load: $l_intrinsic, store: $s_intrinsic, }
    );
  )*};
}
macro_rules! impl_f32_image_load_store {
  ($mask_ty:ty, $entry:ident <$idx:ident: $idx_ty:ty,>
    { load: $l_intrinsic:literal, store: $s_intrinsic:literal, }
  ) => {
    impl<T> ImageOps<[f32; 1], $mask_ty> for geometry::$entry<T>
      where T: Copy + From<u8> + Add + Sub + PartialOrd + CheckedMul,
            T: CheckedAdd + CheckedSub + One + Zero + Send + Sync,
            T: TryInto<u32> + TryInto<usize>,
    {
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn raw_load(h: AmdImageResDesc, $idx: Self::Idx) -> [f32; 1] {
        extern "C" {
          #[link_name = $l_intrinsic]
          fn image_load(dmask: u32, $idx: u32, h: AmdImageResDesc,
                        texfailctrl: u32, cachepolicy: u32) -> f32;
        }
        [image_load(<$mask_ty>::MASK, $idx as _, h, 0, 0); 1]
      }
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn raw_store(h: AmdImageResDesc, $idx: Self::Idx, v: [f32; 1]) {
        extern "C" {
          #[link_name = $s_intrinsic]
          fn image_store(p: f32, dmask: u32, $idx: u32, h: AmdImageResDesc,
                         texfailctrl: u32, cachepolicy: u32);
        }
        image_store(v[0], <$mask_ty>::MASK, $idx as _, h, 0, 0)
      }
    }
  };
  ($mask_ty:ty, $entry:ident <$($idx:ident: $idx_ty:ty,)+>
    { load: $l_intrinsic:literal, store: $s_intrinsic:literal, }
  ) => {
    impl<T> ImageOps<[f32; 1], $mask_ty> for geometry::$entry<T>
      where T: Copy + From<u8> + Add + Sub + PartialOrd + CheckedMul,
            T: CheckedAdd + CheckedSub + One + Zero + Send + Sync,
            T: TryInto<u32> + TryInto<usize>,
    {
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn raw_load(h: AmdImageResDesc, ($($idx,)*): Self::Idx) -> [f32; 1] {
        extern "C" {
          #[link_name = $l_intrinsic]
          fn image_load(dmask: u32, $($idx: u32,)* h: AmdImageResDesc,
                        texfailctrl: u32, cachepolicy: u32) -> f32;
        }
        [image_load(<$mask_ty>::MASK, $($idx as _,)* h, 0, 0); 1]
      }
      #[inline(always)]
      #[doc(hidden)]
      unsafe fn raw_store(h: AmdImageResDesc, ($($idx,)*): Self::Idx, v: [f32; 1]) {
        extern "C" {
          #[link_name = $s_intrinsic]
          fn image_store(p: f32, dmask: u32, $($idx: u32,)* h: AmdImageResDesc,
                         texfailctrl: u32, cachepolicy: u32);
        }
        image_store(v[0], <$mask_ty>::MASK, $($idx as _,)* h, 0, 0)
      }
    }
  };
  ($mask_ty:ty, {
    $(
      $entry:ident <$($idx:ident: $idx_ty:ty,)*>
      { load: $l_intrinsic:literal, store: $s_intrinsic:literal, },
    )*
  }) => {$(
    impl_f32_image_load_store!($mask_ty, $entry <$($idx: $idx_ty,)*>
      { load: $l_intrinsic, store: $s_intrinsic, }
    );
  )*};
}
macro_rules! impl_v4f32_image_load_store_for_masks {
  (_, _, _, _, ) => {
    impl_v4f32_image_load_store_for_masks!(Y, _, _, _, );
    impl_v4f32_image_load_store_for_masks!(N, _, _, _, );
  };
  ($r:ident, _, _, _, ) => {
    impl_v4f32_image_load_store_for_masks!($r, Y, _, _, );
    impl_v4f32_image_load_store_for_masks!($r, N, _, _, );
  };
  ($r:ident, $g:ident, _, _, ) => {
    impl_v4f32_image_load_store_for_masks!($r, $g, Y, _, );
    impl_v4f32_image_load_store_for_masks!($r, $g, N, _, );
  };
  ($r:ident, $g:ident, $b:ident, _, ) => {
    impl_v4f32_image_load_store_for_masks!($r, $g, $b, Y, );
    impl_v4f32_image_load_store_for_masks!($r, $g, $b, N, );
  };
  ($r:ident, $g:ident, $b:ident, $a:ident, ) => {
    impl_v4f32_image_load_store! {
      Mask<$r, $g, $b, $a>, {
        OneD <x: u32, > {
          load: "llvm.amdgcn.image.load.1d.v4f32.i32",
          store: "llvm.amdgcn.image.store.1d.v4f32.i32",
        },
        TwoD <x: u32, y: u32, > {
          load: "llvm.amdgcn.image.load.2d.v4f32.i32",
          store: "llvm.amdgcn.image.store.2d.v4f32.i32",
        },
        ThreeD <x: u32, y: u32, z: u32, > {
          load: "llvm.amdgcn.image.load.3d.v4f32.i32",
          store: "llvm.amdgcn.image.store.3d.v4f32.i32",
        },
        OneDArray <x: u32, y: u32, > {
          load: "llvm.amdgcn.image.load.1darray.v4f32.i32",
          store: "llvm.amdgcn.image.store.1darray.v4f32.i32",
        },
        TwoDArray <x: u32, y: u32, z: u32, > {
          load: "llvm.amdgcn.image.load.2darray.v4f32.i32",
          store: "llvm.amdgcn.image.store.2darray.v4f32.i32",
        },
        OneDB <x: u32, > {
          load: "llvm.amdgcn.image.load.1d.v4f32.i32",
          store: "llvm.amdgcn.image.store.1d.v4f32.i32",
        },
      }
    }
  };
}
impl_v4f32_image_load_store_for_masks!(_, _, _, _, );

macro_rules! impl_f32_image_load_store_for_single_masks {
  ($r:ident, $g:ident, $b:ident, $a:ident, ) => {
    impl_f32_image_load_store! {
      Mask<$r, $g, $b, $a>, {
        OneD <x: u32, > {
          load: "llvm.amdgcn.image.load.1d.f32.i32",
          store: "llvm.amdgcn.image.store.1d.f32.i32",
        },
        TwoD <x: u32, y: u32, > {
          load: "llvm.amdgcn.image.load.2d.f32.i32",
          store: "llvm.amdgcn.image.store.2d.f32.i32",
        },
        ThreeD <x: u32, y: u32, z: u32, > {
          load: "llvm.amdgcn.image.load.3d.f32.i32",
          store: "llvm.amdgcn.image.store.3d.f32.i32",
        },
        OneDArray <x: u32, y: u32, > {
          load: "llvm.amdgcn.image.load.1darray.f32.i32",
          store: "llvm.amdgcn.image.store.1darray.f32.i32",
        },
        TwoDArray <x: u32, y: u32, z: u32, > {
          load: "llvm.amdgcn.image.load.2darray.f32.i32",
          store: "llvm.amdgcn.image.store.2darray.f32.i32",
        },
        OneDB <x: u32, > {
          load: "llvm.amdgcn.image.load.1d.f32.i32",
          store: "llvm.amdgcn.image.store.1d.f32.i32",
        },
      }
    }
  };
}
impl_f32_image_load_store_for_single_masks!(Y, N, N, N, );
impl_f32_image_load_store_for_single_masks!(N, Y, N, N, );
impl_f32_image_load_store_for_single_masks!(N, N, Y, N, );
impl_f32_image_load_store_for_single_masks!(N, N, N, Y, );

pub mod channel_mask;
pub mod channel_type;
pub mod format;

pub trait ReadDeviceImageOps<A, F, G, L>: ImageHandle<A, F, G, L>
  where A: ReadAccess,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{
  #[inline(always)]
  fn load<T>(&self, idx: G::Idx) -> T
    where F::Type: AccessTypeDetail<T>,
          G: ImageOps<<F::Type as AccessTypeDetail<T>>::RawPixel, F::DefaultMask>,
  {
    self.load_masked2::<T, F::DefaultMask>(idx)
  }
  #[inline(always)]
  fn load_masked<T, M>(&self, idx: G::Idx, _: M) -> T
    where F::Type: AccessTypeDetail<T>,
          G: ImageOps<<F::Type as AccessTypeDetail<T>>::RawPixel, M>,
          M: MaskDetail,
  {
    self.load_masked2::<T, M>(idx)
  }
  #[inline(always)]
  fn load_masked2<T, M>(&self, idx: G::Idx) -> T
    where F::Type: AccessTypeDetail<T>,
          G: ImageOps<<F::Type as AccessTypeDetail<T>>::RawPixel, M>,
          M: MaskDetail,
  {
    unsafe {
      let f = <G as ImageOps<<F::Type as AccessTypeDetail<T>>::RawPixel, M>>::raw_load;
      let p = f(*self.resource_desc(), idx);
      <F::Type as AccessTypeDetail<T>>::map_load(p)
    }
  }
}
pub trait WriteDeviceImageOps<A, F, G, L>: ImageHandle<A, F, G, L>
  where A: WriteAccess,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{
  #[inline(always)]
  fn store<T>(&self, idx: G::Idx, pixel: T)
    where F::Type: AccessTypeDetail<T>,
          G: ImageOps<<F::Type as AccessTypeDetail<T>>::RawPixel, F::DefaultMask>,
  {
    self.store_masked2::<T, F::DefaultMask>(idx, pixel)
  }
  #[inline(always)]
  fn store_masked<T, M>(&self, idx: G::Idx, pixel: T, _: M)
    where F::Type: AccessTypeDetail<T>,
          G: ImageOps<<F::Type as AccessTypeDetail<T>>::RawPixel, M>,
          M: MaskDetail,
  {
    self.store_masked2::<T, M>(idx, pixel)
  }
  #[inline(always)]
  fn store_masked2<T, M>(&self, idx: G::Idx, pixel: T)
    where F::Type: AccessTypeDetail<T>,
          G: ImageOps<<F::Type as AccessTypeDetail<T>>::RawPixel, M>,
          M: MaskDetail,
  {
    unsafe {
      let f = <G as ImageOps<<F::Type as AccessTypeDetail<T>>::RawPixel, M>>::raw_store;
      let p = <F::Type as AccessTypeDetail<T>>::map_store(pixel);
      f(*self.resource_desc(), idx, p)
    }
  }
}
pub trait ReadWriteDeviceImageOps<F, G, L>: ImageHandle<ReadWrite, F, G, L>
  where F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{
  // TODO atomic cmpxchg etc
}

impl<T, A, F, G, L> ReadDeviceImageOps<A, F, G, L> for T
  where T: ImageHandle<A, F, G, L>,
        A: ReadAccess,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{ }
impl<T, A, F, G, L> WriteDeviceImageOps<A, F, G, L> for T
  where T: ImageHandle<A, F, G, L>,
        A: WriteAccess,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{ }
impl<T, F, G, L> ReadWriteDeviceImageOps<F, G, L> for T
  where T: ImageHandle<ReadWrite, F, G, L>,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{ }

impl HsaAmdGpuAccel {
  pub fn create_texture<I, G, F, L>(&self, geometry: G, layout: L)
    -> Result<Image<I, F, G, L, ImageData>, Error>
    where I: AccessDetail,
          G: GeometryDetail,
          F: FormatDetail,
          L: LayoutDetail,
  {
    unsafe {
      let img = self.agent()
        .create_amd_image(I::default(), geometry,
                          F::default(), layout,
                          self.device.coarse.clone())?;
      Ok(img)
    }
  }

  pub fn create_opaque_texture<I, G, F>(&self, geometry: G)
    -> Result<Image<I, F, G, Opaque, ImageData>, Error>
    where I: AccessDetail,
          G: GeometryDetail,
          F: FormatDetail,
  {
    unsafe {
      let img = self.agent()
        .create_amd_image(I::default(), geometry,
                          F::default(), Opaque,
                          self.device.coarse.clone())?;
      Ok(img)
    }
  }
  #[inline]
  pub fn create_ro_texture<G, F>(&self, geometry: G)
    -> Result<Image<ReadOnly, F, G, Opaque, ImageData>, Error>
    where G: GeometryDetail,
          F: FormatDetail,
  {
    self.create_opaque_texture(geometry)
  }
  #[inline]
  pub fn create_wo_texture<G, F>(&self, geometry: G)
    -> Result<Image<WriteOnly, F, G, Opaque, ImageData>, Error>
    where G: GeometryDetail,
          F: FormatDetail,
  {
    self.create_opaque_texture(geometry)
  }
  #[inline]
  pub fn create_rw_texture<G, F>(&self, geometry: G)
    -> Result<Image<ReadWrite, F, G, Opaque, ImageData>, Error>
    where G: GeometryDetail,
          F: FormatDetail,
  {
    self.create_opaque_texture(geometry)
  }

  pub fn create_linear_texture<I, G, F>(&self, geometry: G, layout: Linear)
    -> Result<Image<I, F, G, Linear, ImageData>, Error>
    where I: AccessDetail,
          G: GeometryDetail,
          F: FormatDetail,
  {
    unsafe {
      let img = self.agent()
        .create_amd_image(I::default(), geometry,
                          F::default(), layout,
                          self.device.coarse.clone())?;
      Ok(img)
    }
  }
  #[inline]
  pub fn create_ro_linear_texture<G, F>(&self, geometry: G, layout: Linear)
    -> Result<Image<ReadOnly, F, G, Linear, ImageData>, Error>
    where G: GeometryDetail,
          F: FormatDetail,
  {
    self.create_linear_texture(geometry, layout)
  }
  #[inline]
  pub fn create_wo_linear_texture<G, F>(&self, geometry: G, layout: Linear)
    -> Result<Image<WriteOnly, F, G, Linear, ImageData>, Error>
    where G: GeometryDetail,
          F: FormatDetail,
  {
    self.create_linear_texture(geometry, layout)
  }
  #[inline]
  pub fn create_rw_linear_texture<G, F>(&self, geometry: G, layout: Linear)
    -> Result<Image<ReadWrite, F, G, Linear, ImageData>, Error>
    where G: GeometryDetail,
          F: FormatDetail,
  {
    self.create_linear_texture(geometry, layout)
  }
}

#[cfg(test)]
mod test;
