
//! Note: this crate does not provide any way to use created images on any accelerator.
//! `geobacter_runtime_amd` provides this for Geobacter projects.

use std::alloc::Layout as AllocLayout;
use std::convert::*;
use std::geobacter::amdgpu::workitem::ReadFirstLane;
use std::marker::PhantomData;
use std::mem::{MaybeUninit, size_of};
use std::ops::*;
use std::os::raw::c_void;
use std::ptr::{self, NonNull};
use std::sync::Arc;

use num_traits::identities::{One, Zero, };
use num_traits::ops::checked::*;

use agent::Agent;
use alloc::{HsaAlloc, NonNullAlignedPair};
use error::Error;
use ffi;

use self::access::*;
use self::channel_order::*;
use self::channel_type::*;
use self::format::*;
use self::geometry::*;
use self::layout::*;

pub use ffi::hsa_ext_image_data_info_s as ImageDataInfo;
pub use self::access::Access;
pub use self::addressing_mode::AddrMode;
pub use self::channel_order::ChannelOrder;
pub use self::channel_type::ChannelType;
pub use self::coord_mode::CoordMode;
pub use self::filter_mode::FilterMode;
pub use self::format::Format;
pub use self::geometry::{Geometry, Region, };
pub use self::layout::Layout;

macro_rules! decl_struct {
  ($(#[$attrs:meta])*
   pub struct $entry:ident $(<$($gen2:ident,)*>)? { $($fields:ident: $field_ty:ty,)* }) =>
  {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    $(#[$attrs])*
    pub struct $entry $(<$($gen2,)*>)? { $(pub $fields: $field_ty,)* }
  };
  ($(#[$attrs:meta])* pub struct $entry:ident) =>
  {
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
    $(#[$attrs])*
    pub struct $entry;
  };
}

macro_rules! decl {
  ($(#[$outer_attrs:meta])*
    pub enum $enum:ident $(<$($gen1:ident),*>)? {
      $($(#[$attrs:meta])*
        $entry:ident
        $(<$($gen2:ident),*>)?
        $({ $($fields:ident: $field_ty:ty,)* })?
        $(= $val:expr)?,)*
    }
  ) => {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    $(#[$outer_attrs])*
    pub enum $enum $(<$($gen1,)*>)? {
      $($(#[$attrs])* $entry $({ $($fields: $field_ty,)* })? $(= $val)?,)*
    }
    $(
      decl_struct! {
        $(#[$attrs])* pub struct $entry $(<$($gen2,)*>)? $({ $($fields: $field_ty,)* })?
      }
    )*
    $(
      impl $(<$($gen2,)*>)? Into<$enum $(<$($gen2,)*>)?> for $entry $(<$($gen2,)*>)? {
        #[inline(always)]
        fn into(self) -> $enum $(<$($gen2,)*>)? {
          $enum::$entry $({
            $($fields: self.$fields,)*
          })?
        }
      }
    )*
  };
  ($(#[$outer_attrs:meta])*
    pub enum $enum:ident $(<$($gen1:ident),*>)? {
      $($(#[$attrs:meta])*
        $entry:ident
        $(<$($gen2:ident),*>)?
        $({ $($fields:ident: $field_ty:ty,)* })?
        $(= $val:expr)?,)*
    }
    pub trait $detail:ident $(: $($dep:path,)*)? { }
  ) => {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    $(#[$outer_attrs])*
    pub enum $enum $(<$($gen1,)*>)? {
      $($(#[$attrs])* $entry $({ $($fields: $field_ty,)* })? $(= $val)?,)*
    }
    $(
      decl_struct! {
        $(#[$attrs])* pub struct $entry $(<$($gen2,)*>)? $({ $($fields: $field_ty,)* })?
      }
    )*
    $(
      impl $(<$($gen2,)*>)? Into<$enum $(<$($gen2,)*>)?> for $entry $(<$($gen2,)*>)? {
        #[inline(always)]
        fn into(self) -> $enum $(<$($gen2,)*>)? {
          $enum::$entry $({
            $($fields: self.$fields,)*
          })?
        }
      }
    )*
    pub trait $detail $(<$($gen1,)*>)?: Into<$enum $(<$($gen1,)*>)?>
      where Self: Clone + Send + Sync, $($(Self: $dep,)*)?
        $($($gen1: Clone,)*)?
    {
      #[inline(always)]
      fn into_enum(self) -> $enum $(<$($gen1,)*>)? { self.into() }
    }
    $(
      impl $(<$($gen2,)*>)? $detail $(<$($gen2,)*>)? for $entry $(<$($gen2,)*>)?
        where Self: Clone,
          $($($gen2: Clone,)*)?
      { }
    )*
  };
}

pub mod access;
pub mod layout;
pub mod geometry;
pub mod channel_type;
pub mod channel_order;
pub mod format;
pub mod coord_mode;
pub mod filter_mode;
pub mod addressing_mode;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Capability(ffi::hsa_ext_image_capability_t);
impl Capability {
  pub fn supported(&self) -> bool {
    self.0 != ffi::hsa_ext_image_capability_t_HSA_EXT_IMAGE_CAPABILITY_NOT_SUPPORTED
  }
  pub fn read_only(&self) -> bool {
    self.0 & ffi::hsa_ext_image_capability_t_HSA_EXT_IMAGE_CAPABILITY_READ_ONLY != 0
  }
  pub fn write_only(&self) -> bool {
    self.0 & ffi::hsa_ext_image_capability_t_HSA_EXT_IMAGE_CAPABILITY_WRITE_ONLY != 0
  }
  pub fn read_write(&self) -> bool {
    self.0 & ffi::hsa_ext_image_capability_t_HSA_EXT_IMAGE_CAPABILITY_READ_WRITE != 0
  }
  pub fn read_modify_write(&self) -> bool {
    self.0 & ffi::hsa_ext_image_capability_t_HSA_EXT_IMAGE_CAPABILITY_READ_MODIFY_WRITE != 0
  }
  pub fn access_invariant_data_layout(&self) -> bool {
    self.0 & ffi::hsa_ext_image_capability_t_HSA_EXT_IMAGE_CAPABILITY_ACCESS_INVARIANT_DATA_LAYOUT != 0
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Descriptor<G, F> {
  pub geometry: G,
  pub format: F,
}
impl<G, F> Descriptor<G, F>
  where G: GeometryDetail,
        F: FormatDetail,
{
  #[inline(always)]
  fn set_ffi(&self, ffi: &mut ffi::hsa_ext_image_descriptor_s)
    -> Result<(), Error>
  {
    use self::channel_order::*;
    use self::geometry::*;

    self.geometry.set_ffi(ffi)?;
    self.format.set_ffi(&mut ffi.format);

    match (self.geometry.into_enum(), self.format.into_enum().order) {
      (Geometry::TwoD { .. }, ChannelOrder::Depth) |
      (Geometry::TwoD { .. }, ChannelOrder::DepthStencil) => {
        ffi.geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_2DDEPTH;
      },
      (Geometry::TwoDArray { .. }, ChannelOrder::Depth) |
      (Geometry::TwoDArray { .. }, ChannelOrder::DepthStencil) => {
        ffi.geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_2DADEPTH;
      },
      _ => { }
    }

    Ok(())
  }
  pub fn data_info(&self, agent: &Agent,
                   access: Access, layout: Layout)
    -> Result<ImageDataInfo, Error>
  {
    let mut ffi = unsafe { MaybeUninit::zeroed().assume_init() };
    self.set_ffi(&mut ffi)?;
    let mut out = unsafe { MaybeUninit::zeroed().assume_init() };
    match layout {
      layout::Layout::Linear {
        row_pitch, slice_pitch,
      } => {
        let row_pitch = row_pitch.unwrap_or_default();
        let slice_pitch = slice_pitch.unwrap_or_default();

        check_err! {
          ffi::hsa_ext_image_data_get_info_with_layout(agent.0, &ffi, access as _,
                                                       ffi::hsa_ext_image_data_layout_t_HSA_EXT_IMAGE_DATA_LAYOUT_LINEAR,
                                                       row_pitch as _, slice_pitch as _,
                                                       &mut out) => out
        }
      },
      layout::Layout::Opaque => {
        check_err! {
          ffi::hsa_ext_image_data_get_info(agent.0, &ffi, access as _, &mut out) => out
        }
      },
    }
  }
}
impl<G, F> Descriptor<G, F>
  where F: FormatIntoEnum + Copy,
{
  #[inline(always)]
  pub fn capabilities<L, GE>(&self, layout: L, agent: &Agent)
    -> Result<Capability, Error>
    where L: Into<Layout>,
          G: Into<Geometry<GE>> + Copy,
  {
    agent.image_capabilities(self.geometry, self.format,
                             layout)
  }
}

#[derive(Debug)]
pub struct ImageData<A>
  where A: HsaAlloc,
{
  alloc: A,
  layout: AllocLayout,
  data: NonNullAlignedPair<u8>,
}
impl<A> ImageData<A>
  where A: HsaAlloc,
{
  pub fn alloc(info: &ImageDataInfo, mut alloc: A) -> Result<Self, Error> {
    let layout = AllocLayout::from_size_align(info.size as _,
                                              info.alignment as _)
      .ok().ok_or(Error::Overflow)?;
    let data = alloc.alloc(layout.clone())?;
    Ok(ImageData {
      alloc,
      layout,
      data,
    })
  }
}
impl<A> Drop for ImageData<A>
  where A: HsaAlloc,
{
  fn drop(&mut self) {
    unsafe {
      let _ = self.alloc.dealloc(self.data, self.layout);
    }
  }
}
unsafe impl<A> Send for ImageData<A>
  where A: HsaAlloc + Send,
{ }
unsafe impl<A> Sync for ImageData<A>
  where A: HsaAlloc + Sync,
{ }
impl<A> Eq for ImageData<A>
  where A: HsaAlloc,
{ }
impl<A, B> PartialEq<ImageData<B>> for ImageData<A>
  where A: HsaAlloc,
        B: HsaAlloc,
{
  fn eq(&self, rhs: &ImageData<B>) -> bool {
    &*self.data == &*rhs.data
  }
}
impl<A, B> PartialEq<Arc<ImageData<B>>> for ImageData<A>
  where A: HsaAlloc,
        B: HsaAlloc,
{
  fn eq(&self, rhs: &Arc<ImageData<B>>) -> bool {
    // XXX shouldn't this always be false?
    &*self.data == &*rhs.data
  }
}
pub trait ImageDataPtr {
  fn as_ptr(&self) -> NonNull<u8>;
}
impl<A> ImageDataPtr for ImageData<A>
  where A: HsaAlloc,
{
  #[inline(always)]
  fn as_ptr(&self) -> NonNull<u8> {
    *self.data
  }
}
impl<A> ImageDataPtr for Arc<ImageData<A>>
  where A: HsaAlloc,
{
  #[inline(always)]
  fn as_ptr(&self) -> NonNull<u8> {
    *self.data
  }
}

pub trait ImageHandle<A, F, G, L>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{
  #[inline(always)]
  fn access(&self) -> A { A::default() }
  fn geometry(&self) -> &G;
  #[inline(always)]
  fn format(&self) -> F { F::default() }
  fn layout(&self) -> &L;
  fn resource_desc(&self) -> &AmdImageResDesc;
  fn raw_handle(&self) -> ffi::hsa_ext_image_t;

  #[inline(always)]
  fn as_ref(&self) -> ImageRef<A, F, G, L> {
    ImageRef {
      resource_desc: *self.resource_desc(),
      handle: self.raw_handle(),
      g: *self.geometry(),
      l: *self.layout(),
      _a: PhantomData,
      _f: PhantomData,
      _owner: PhantomData,
    }
  }
}

/// To be used inside kernels without moving into them.
#[derive(Clone, Copy)]
pub struct ImageRef<'a, A, F, G, L>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{
  resource_desc: AmdImageResDesc,
  handle: ffi::hsa_ext_image_t,
  g: G,
  l: L,
  _a: PhantomData<A>,
  _f: PhantomData<F>,
  _owner: PhantomData<&'a ()>,
}

impl<'a, A, F, G, L> ReadFirstLane for ImageRef<'a, A, F, G, L>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail + ReadFirstLane,
        L: LayoutDetail + ReadFirstLane,
{
  #[inline(always)]
  unsafe fn read_first_lane(self) -> Self {
    ImageRef {
      resource_desc: self.resource_desc.read_first_lane(),
      handle: ffi::hsa_ext_image_t {
        handle: self.handle.handle.read_first_lane(),
      },
      g: self.g.read_first_lane(),
      l: self.l.read_first_lane(),
      .. self
    }
  }
}

impl<'a, A, F, G, L> ImageHandle<A, F, G, L> for ImageRef<'a, A, F, G, L>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{
  #[inline(always)]
  fn geometry(&self) -> &G { &self.g }
  #[inline(always)]
  fn layout(&self) -> &L { &self.l }
  #[inline(always)]
  fn resource_desc(&self) -> &AmdImageResDesc { &self.resource_desc }
  #[inline(always)]
  fn raw_handle(&self) -> ffi::hsa_ext_image_t { self.handle }
}

pub struct Image<A, F, G, L, R>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{
  agent: Agent,
  data: R,
  resource_desc: AmdImageResDesc,
  handle: ffi::hsa_ext_image_t,
  g: G,
  l: L,
  _a: PhantomData<A>,
  _f: PhantomData<F>,
}

impl<A, F, G, L, R> ImageHandle<A, F, G, L> for Image<A, F, G, L, R>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{
  #[inline(always)]
  fn geometry(&self) -> &G { &self.g }
  #[inline(always)]
  fn layout(&self) -> &L { &self.l }
  #[inline(always)]
  fn resource_desc(&self) -> &AmdImageResDesc { &self.resource_desc }
  #[inline(always)]
  fn raw_handle(&self) -> ffi::hsa_ext_image_t { self.handle }
}
/// TODO: impl copy when the geometry/channel type/order don't exactly match (but where HSA still
///       allows it). For example, from 2d to 3d, or from RGB to SRGB.
impl<A, F, G, L, DA> Image<A, F, G, L, ImageData<DA>>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
        DA: HsaAlloc,
{
  #[inline]
  pub fn into_shared(self) -> Image<A, F, G, L, Arc<ImageData<DA>>> {
    unsafe {
      let agent = ptr::read(&self.agent);
      let data = ptr::read(&self.data);
      let resource_desc = self.resource_desc;
      let handle = self.handle;
      let g = ptr::read(&self.g);
      let l = ptr::read(&self.l);

      std::mem::forget(self); // don't destroy the image handle.

      let data = Arc::new(data);

      // Note: copying `handle` here is still safe; cloning creates a new handle
      // using the now shared data.

      Image {
        agent,
        data,
        resource_desc,
        handle,
        g,
        l,
        _a: PhantomData,
        _f: PhantomData,
      }
    }
  }

  #[inline(always)]
  pub fn clear_all(&mut self, v: &F::HostType) -> Result<(), Error> {
    self.clear(v, self.g.into())
  }
  #[inline(always)]
  pub fn clear(&mut self, v: &F::HostType, region: Region<G>) -> Result<(), Error> {
    unsafe {
      self.clear_raw(v, region)
    }
  }

  #[inline(always)]
  pub fn copy<A2, L2, R2>(&mut self, offset: G,
                           src: &Image<A2, F, G, L2, R2>,
                           src_offset: G, range: G)
    -> Result<(), Error>
    where A2: AccessDetail,
          L2: LayoutDetail,
          ImageData<DA>: PartialEq<R2>,
  {
    unsafe {
      self.copy_raw(offset, src, src_offset, range)
    }
  }

  #[inline(always)]
  pub fn import_all_packed(&mut self, src: &[F::HostType]) -> Result<(), Error> {
    self.import(src, Default::default(), self.g.into())
  }
  #[inline(always)]
  pub fn import_packed(&mut self, src: &[F::HostType], region: Region<G>)
    -> Result<(), Error>
  {
    self.import(src, Default::default(), region)
  }
  #[inline(always)]
  pub fn import(&mut self, src: &[F::HostType], src_layout: Linear, region: Region<G>)
    -> Result<(), Error>
  {
    unsafe {
      self.import_raw(src, src_layout, region)
    }
  }

  #[inline(always)]
  pub fn export_all_packed(&self, dst: &mut [F::HostType]) -> Result<(), Error> {
    self.export(dst, Default::default(), self.g.into())
  }
  #[inline(always)]
  pub fn export_packed(&self, dst: &mut [F::HostType], region: Region<G>)
    -> Result<(), Error>
  {
    self.export(dst, Default::default(), region)
  }
  #[inline(always)]
  pub fn export(&self, dst: &mut [F::HostType], dst_layout: Linear, region: Region<G>)
    -> Result<(), Error>
  {
    unsafe {
      self.export_raw(dst, dst_layout, region)
    }
  }
}

fn check_slice<T, G>(slice: &[T], g: &G, layout: &Linear, region: &Region<G>)
  -> Result<(usize, usize), Error>
  where G: GeometryDetail,
{
  let region_len = region.len()?;
  if g < &region_len {
    return Err(Error::InvalidArgument);
  }

  let (row_stride, slice_stride) = layout.strides(&region)?;
  let slice_len = row_stride.checked_mul(slice_stride)
    .ok_or(Error::Overflow)?;
  if slice_len > slice.len() {
    Err(Error::InvalidArgument)
  } else {
    let row_pitch = size_of::<T>().checked_mul(row_stride)
      .ok_or(Error::Overflow)?;
    let slice_pitch = size_of::<T>().checked_mul(slice_stride)
      .ok_or(Error::Overflow)?;
    Ok((row_pitch, slice_pitch))
  }
}

impl<A, F, G, L, R> Image<A, F, G, L, R>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{
  pub unsafe fn clear_raw_all<T>(&self, v: &T) -> Result<(), Error>
    where T: Copy,
  {
    self.clear_raw(v, self.g.into())
  }
  pub unsafe fn clear_raw<T>(&self, v: &T, region: Region<G>) -> Result<(), Error>
    where T: Copy,
  {
    if F::host_type_size() != size_of::<T>() {
      return Err(Error::InvalidArgument);
    }

    let region = region.try_into()?;
    check_err!(
      ffi::hsa_ext_image_clear(self.agent.0, self.handle, v as *const _ as *const _,
                               &region)
    )
  }

  /// No checking is performed to ensure these images can really be copied. Not sure if the
  /// HSA ext checks, either. `G3` exists because we can't be sure which of `G` or `G2` has the
  /// higher rank.
  pub unsafe fn copy_raw<A2, F2, G2, L2, R2, G3>(&self, offset: G,
                                                 src: &Image<A2, F2, G2, L2, R2>,
                                                 src_offset: G2,
                                                 range: G3)
    -> Result<(), Error>
    where A2: AccessDetail,
          F2: FormatDetail,
          G2: GeometryDetail,
          L2: LayoutDetail,
          G3: GeometryDetail,
          R: PartialEq<R2>,
  {
    if self.agent != src.agent { return Err(Error::InvalidAgent); }
    if self.data == src.data { return Err(Error::InvalidArgument); }

    let mut ffi_dst_offset = ffi::hsa_dim3_t {
      x: 0, y: 0, z: 0,
    };
    offset.hsa_dim(&mut ffi_dst_offset)?;
    let mut ffi_src_offset = ffi::hsa_dim3_t {
      x: 0, y: 0, z: 0,
    };
    src_offset.hsa_dim(&mut ffi_src_offset)?;

    let mut ffi_range = ffi::hsa_dim3_t {
      x: 1, y: 1, z: 1,
    };
    range.hsa_dim(&mut ffi_range)?;

    check_err!(
      ffi::hsa_ext_image_copy(self.agent.0, src.handle, &ffi_src_offset,
                              self.handle, &ffi_dst_offset, &ffi_range)
    )
  }

  #[inline(always)]
  pub unsafe fn import_raw_all_packed<T>(&self, src: &[T])
    -> Result<(), Error>
    where T: Copy,
  {
    self.import_raw(src, Default::default(), self.g.into())
  }
  #[inline(always)]
  pub unsafe fn import_raw_packed<T>(&self, src: &[T], region: Region<G>)
    -> Result<(), Error>
    where T: Copy,
  {
    self.import_raw(src, Default::default(), region)
  }
  /// `T`'s size *must* match the size of memory components
  pub unsafe fn import_raw<T>(&self, src: &[T], src_layout: Linear,
                              region: Region<G>)
    -> Result<(), Error>
    where T: Copy,
  {
    if F::host_type_size() != size_of::<T>() {
      return Err(Error::InvalidArgument);
    }

    let (row_pitch, slice_pitch) =
      check_slice(src, &self.g, &src_layout,
                  &region)?;

    let ffi_region = region.try_into()?;

    let src_ptr = src.as_ptr() as *const c_void;

    check_err!(
      ffi::hsa_ext_image_import(self.agent.0, src_ptr,
                                row_pitch as _, slice_pitch as _,
                                self.handle, &ffi_region)
    )
  }

  #[inline(always)]
  pub unsafe fn export_raw_all_packed<T>(&self, dst: &mut [T])
    -> Result<(), Error>
    where T: Copy,
  {
    self.export_raw(dst, Default::default(), self.g.into())
  }
  #[inline(always)]
  pub unsafe fn export_raw_packed<T>(&self, dst: &mut [T], region: Region<G>)
    -> Result<(), Error>
    where T: Copy,
  {
    self.export_raw(dst, Default::default(), region)
  }
  pub unsafe fn export_raw<T>(&self, dst: &mut [T], dst_layout: Linear,
                              region: Region<G>)
    -> Result<(), Error>
    where T: Copy,
  {
    if F::host_type_size() != size_of::<T>() {
      return Err(Error::InvalidArgument);
    }

    let (row_pitch, slice_pitch) =
      check_slice(dst, &self.g, &dst_layout,
                  &region)?;

    let ffi_region = region.try_into()?;

    let dst_ptr = dst.as_mut_ptr() as *mut c_void;

    check_err!(
      ffi::hsa_ext_image_export(self.agent.0, self.handle,
                                dst_ptr, row_pitch as _, slice_pitch as _,
                                &ffi_region)
    )
  }
}
impl<A, F, G, L, R> Image<A, F, G, L, R>
  where A: AccessDetail,
        G: GeometryDetail,
        F: FormatDetail,
        L: LayoutDetail,
        R: ImageDataPtr + Clone,
{
  pub fn try_clone(&self) -> Result<Self, Error> {
    let access = A::default();
    let desc = Descriptor {
      geometry: self.g.clone(),
      format: self.format(),
    };
    let mut ffi = unsafe { MaybeUninit::zeroed().assume_init() };
    desc.set_ffi(&mut ffi)?;
    let ptr = self.data.as_ptr().as_ptr() as *const _;
    let access = access.clone().into_enum() as _;
    let mut out = Default::default();
    match self.l.into() {
      Layout::Opaque => {
        check_err!(
          ffi::hsa_ext_image_create(self.agent.0, &ffi, ptr, access, &mut out)
        )?;
      },
      Layout::Linear {
        row_pitch, slice_pitch,
      } => {
        let layout = ffi::hsa_ext_image_data_layout_t_HSA_EXT_IMAGE_DATA_LAYOUT_LINEAR;
        let row_pitch = row_pitch.unwrap_or_default();
        let slice_pitch = slice_pitch.unwrap_or_default();
        check_err!(
          ffi::hsa_ext_image_create_with_layout(self.agent.0, &ffi, ptr, access,
                                                layout, row_pitch as _, slice_pitch as _,
                                                &mut out)
        )?;
      }
    }
    Ok(Image {
      agent: self.agent.clone(),
      data: self.data.clone(),
      resource_desc: unsafe { load_amd_img_res_desc(out) },
      handle: out,
      g: self.g.clone(),
      l: self.l.clone(),
      _a: PhantomData,
      _f: PhantomData,
    })
  }
}
impl<A, F, G, L, R> Clone for Image<A, F, G, L, R>
  where A: AccessDetail,
        G: GeometryDetail,
        F: FormatDetail,
        L: LayoutDetail,
        R: ImageDataPtr + Clone,
{
  #[inline(always)]
  fn clone(&self) -> Self {
    self.try_clone().unwrap()
  }
}
impl<A, F, G, L, R> Drop for Image<A, F, G, L, R>
  where A: AccessDetail,
        G: GeometryDetail,
        F: FormatDetail,
        L: LayoutDetail,
{
  fn drop(&mut self) {
    unsafe {
      ffi::hsa_ext_image_destroy(self.agent.0, self.handle);
    }
  }
}

/// The image resource descriptor, as used by various AMDGPU instructions for image ops.
#[repr(simd)]
#[derive(Clone, Copy, Debug)]
pub struct AmdImageResDesc(u32, u32, u32, u32, u32, u32, u32, u32);
impl ReadFirstLane for AmdImageResDesc {
  #[inline(always)]
  unsafe fn read_first_lane(self) -> Self {
    let rd0 = self.0.read_first_lane();
    let rd1 = self.1.read_first_lane();
    let rd2 = self.2.read_first_lane();
    let rd3 = self.3.read_first_lane();
    let rd4 = self.4.read_first_lane();
    let rd5 = self.5.read_first_lane();
    let rd6 = self.6.read_first_lane();
    let rd7 = self.7.read_first_lane();

    AmdImageResDesc(rd0, rd1, rd2, rd3,
                    rd4, rd5, rd6, rd7)
  }
}

/// Amazingly, this resource descriptor is actually visible to the host.
#[inline(always)]
unsafe fn load_amd_img_res_desc(handle: ffi::hsa_ext_image_t) -> AmdImageResDesc {
  *(handle.handle as usize as *const AmdImageResDesc)
}

impl Agent {
  /// Retrieve the supported image capabilities for a given combination of agent, geometry, and
  /// image format for an image created with an opaque image data layout.
  pub fn image_capabilities<F, G, GE, L>(&self, geometry: G,
                                         format: F, layout: L)
    -> Result<Capability, Error>
    where F: FormatIntoEnum,
          L: Into<Layout>,
          G: Into<Geometry<GE>>,
  {
    let mut ffi_geometry;
    let mut ffi_format = unsafe { MaybeUninit::zeroed().assume_init() };
    let mut out = 0u32;

    let geometry = geometry.into();
    match geometry {
      Geometry::OneD { .. } => {
        ffi_geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_1D;
      },
      Geometry::TwoD { .. } => {
        ffi_geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_2D;
      },
      Geometry::ThreeD { .. } => {
        ffi_geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_3D;
      },
      Geometry::OneDArray { .. } => {
        ffi_geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_1DA;
      },
      Geometry::TwoDArray { .. } => {
        ffi_geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_2DA;
      },
      Geometry::OneDB { .. } => {
        ffi_geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_1DB;
      },
    }
    let format = format.into_enum();
    format.set_ffi(&mut ffi_format);

    match (geometry, format.order) {
      (Geometry::TwoD { .. }, ChannelOrder::Depth) |
      (Geometry::TwoD { .. }, ChannelOrder::DepthStencil) => {
        ffi_geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_2DDEPTH;
      },
      (Geometry::TwoDArray { .. }, ChannelOrder::Depth) |
      (Geometry::TwoDArray { .. }, ChannelOrder::DepthStencil) => {
        ffi_geometry = ffi::hsa_ext_image_geometry_t_HSA_EXT_IMAGE_GEOMETRY_2DADEPTH;
      },
      _ => { }
    }

    match layout.into() {
      Layout::Opaque => {
        check_err!(
          ffi::hsa_ext_image_get_capability(self.0, ffi_geometry,
                                            &ffi_format, &mut out)
        )?;
      },
      Layout::Linear { .. } => {
        let layout = ffi::hsa_ext_image_data_layout_t_HSA_EXT_IMAGE_DATA_LAYOUT_LINEAR;
        check_err!(
          ffi::hsa_ext_image_get_capability_with_layout(self.0, ffi_geometry,
                                                        &ffi_format, layout,
                                                        &mut out)
        )?;
      }
    }

    Ok(Capability(out))
  }

  /// Images with an explicit image data layout created with different access permissions but
  /// matching image descriptors and matching image layout can share the same image data if
  /// HSA_EXT_IMAGE_CAPABILITY_ACCESS_INVARIANT_DATA_LAYOUT is reported by
  /// hsa_ext_image_get_capability_with_layout for the image format specified in the image
  /// descriptor and specified image data layout. Image descriptors match if they have the same
  /// values, with the exception that s-form channel orders match the corresponding non-s-form
  /// channel order and vice versa. Image layouts match if they are the same image data layout
  /// and use the same image row and slice values.
  ///
  /// If necessary, an application can use image operations (import, export, copy, clear) to
  /// prepare the image for the intended use regardless of the access permissions.
  ///
  /// This is unsafe because the provided allocator *must* be a device pool. Sadly, using a host
  /// pool here (w/ requisite permission grants) causes the accelerator to segfault.
  ///
  /// Note: the initial texture contents are uninitialized. You'll need to either import, copy,
  /// or initialize directly (for host visible textures).
  pub unsafe fn create_amd_image<I, G, F, L, A>(&self, access: I,
                                                geometry: G, format: F,
                                                layout: L, alloc: A)
    -> Result<Image<I, F, G, L, ImageData<A>>, Error>
    where I: AccessDetail,
          G: GeometryDetail,
          F: FormatDetail,
          L: LayoutDetail,
          A: HsaAlloc,
  {
    let desc = Descriptor {
      geometry: geometry.clone(),
      format,
    };
    let info = desc.data_info(self, access.into(),
                              layout.into())?;
    let data = ImageData::alloc(&info, alloc)?;

    let mut ffi = MaybeUninit::zeroed().assume_init();
    desc.set_ffi(&mut ffi)?;
    let mut out = Default::default();
    match layout.into() {
      Layout::Opaque => {
        check_err!(
          ffi::hsa_ext_image_create(self.0, &ffi, data.data.as_ptr() as *const _,
                                    access.into() as _, &mut out)
        )?;
      },
      Layout::Linear {
        row_pitch, slice_pitch,
      } => {
        let layout = ffi::hsa_ext_image_data_layout_t_HSA_EXT_IMAGE_DATA_LAYOUT_LINEAR;
        let row_pitch = row_pitch.unwrap_or_default();
        let slice_pitch = slice_pitch.unwrap_or_default();
        check_err!(
          ffi::hsa_ext_image_create_with_layout(self.0, &ffi,
                                                data.data.as_ptr() as *const _,
                                                access.into() as _,
                                                layout, row_pitch as _ , slice_pitch as _,
                                                &mut out)
        )?;
      }
    }
    Ok(Image {
      agent: self.clone(),
      data,
      resource_desc: load_amd_img_res_desc(out),
      handle: out,
      g: geometry,
      l: layout,
      _a: PhantomData,
      _f: PhantomData,
    })
  }
}
