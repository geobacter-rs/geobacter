/// Single source defs. These are used in the compiler and in
/// developer code

use std::intrinsics::assume;
use std::mem::transmute;

use crate::vk::descriptor::descriptor::DescriptorImageDescDimensions;

pub use crate::shared_defs::platform::vk::ExeModel;

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u32)]
#[doc(hidden)]
pub enum CompilerDescriptorDescTyKind {
  Sampler = 0,
  CombinedImageSampler,
  Image,
  TexelBuffer,
  InputAttachment,
  Buffer,
}
impl CompilerDescriptorDescTyKind {
  pub fn from(v: u32) -> Self {
    unsafe { assume(v < (CompilerDescriptorDescTyKind::Buffer as u32)) };

    unsafe { transmute(v) }
  }
  pub fn into(self) -> u32 {
    unsafe { transmute(self) }
  }
}
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u32)]
#[doc(hidden)]
pub enum CompilerDescriptorImageDims {
  Dim1 = 0,
  Dim2,
  Dim3,
  Cube,
}
impl CompilerDescriptorImageDims {
  pub fn from(v: u32) -> Self {
    unsafe { assume(v < (CompilerDescriptorImageDims::Cube as u32)) };

    unsafe { transmute(v) }
  }
  pub fn into(self) -> u32 {
    unsafe { transmute(self) }
  }
  pub fn into_vk(self) -> DescriptorImageDescDimensions {
    match self {
      CompilerDescriptorImageDims::Dim1 => DescriptorImageDescDimensions::OneDimensional,
      CompilerDescriptorImageDims::Dim2 => DescriptorImageDescDimensions::TwoDimensional,
      CompilerDescriptorImageDims::Dim3 => DescriptorImageDescDimensions::ThreeDimensional,
      CompilerDescriptorImageDims::Cube => DescriptorImageDescDimensions::Cube,
    }
  }
  pub fn from_vk(v: DescriptorImageDescDimensions) -> Self {
    match v {
      DescriptorImageDescDimensions::OneDimensional => CompilerDescriptorImageDims::Dim1,
      DescriptorImageDescDimensions::TwoDimensional => CompilerDescriptorImageDims::Dim2,
      DescriptorImageDescDimensions::ThreeDimensional => CompilerDescriptorImageDims::Dim3,
      DescriptorImageDescDimensions::Cube => CompilerDescriptorImageDims::Cube,
    }
  }
}
