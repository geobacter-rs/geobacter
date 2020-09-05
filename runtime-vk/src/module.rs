
use std::borrow::Cow;
use std::convert::TryFrom;
use std::ffi::{CStr, CString};
use std::geobacter::platform::spirv::ExeModel;
use std::geobacter::spirv::pipeline_layout::*;
use std::geobacter::spirv::shader_interface::*;
use std::iter::Iterator;
use std::ops::Range;
use std::slice;
use std::sync::Arc;

use smallvec::SmallVec;

use vk;
use vk::descriptor::descriptor::*;
use vk::descriptor::pipeline_layout::*;

use grt_core::context::PlatformModuleData;

use crate::codegen::CodegenShaderInterface;

#[derive(Debug)]
pub struct SpirvEntryPoint {
  /// Vulkano expects a CStr for the entry point name. This is guaranteed to be utf8.
  pub(crate) name: CString,
  pub(crate) spirv: vk::pipeline::shader::ShaderModule,
  pub(crate) exe_model: ExeModel,
  /// If Some(..), this function is a shader; if None, it is a kernel.
  pub(crate) shader: Option<CodegenShaderInterface>,
}

impl SpirvEntryPoint {
  #[inline(always)]
  fn vk_ty(&self) -> vk::pipeline::shader::GraphicsShaderType {
    match self.exe_model {
      ExeModel::Vertex => vk::pipeline::shader::GraphicsShaderType::Vertex,
      ExeModel::TessellationControl =>
        vk::pipeline::shader::GraphicsShaderType::TessellationControl,
      ExeModel::TessellationEval =>
        vk::pipeline::shader::GraphicsShaderType::TessellationEvaluation,
      ExeModel::Geometry => unimplemented!(),
      ExeModel::Fragment => vk::pipeline::shader::GraphicsShaderType::Fragment,
      ExeModel::GLCompute | ExeModel::Kernel => unreachable!(),
    }
  }
  #[inline(always)]
  fn shader_interface_ptr(&self) -> *const () {
    self.shader.as_ref()
      .map(|r| r as *const _ as *const () )
      .unwrap_or(std::ptr::null())
  }
}
impl Eq for SpirvEntryPoint { }
impl PartialEq for SpirvEntryPoint {
  fn eq(&self, rhs: &Self) -> bool {
    use vk::VulkanObject;

    self.spirv.internal_object() == rhs.spirv.internal_object() &&
      self.exe_model == rhs.exe_model &&
      self.shader_interface_ptr() == rhs.shader_interface_ptr() &&
      self.name == rhs.name
  }
}

#[derive(Debug)]
pub struct SpirvModule {
  pub(crate) entries: SmallVec<[Arc<SpirvEntryPoint>; 2]>,
  pub(crate) pipeline_desc: StaticPipelineLayoutDesc,
}
impl SpirvModule {
  pub fn pipeline_layout(&self) -> StaticPipelineLayoutDesc {
    self.pipeline_desc
  }
  pub fn compute_entry_ref(&self) -> Option<SpirvComputeKernelRef> {
    self.entries_ref()
      .find(|v| {
        match v {
          SpirvEntryRef::Kernel(_) => true,
          _ => false,
        }
      })
      .map(|v| {
        match v {
          SpirvEntryRef::Kernel(v) => v,
          _ => unreachable!(),
        }
      })
  }
  pub fn compute_entry(self: Arc<Self>) -> Option<SpirvComputeKernel> {
    self.entries()
      .find(|v| {
        match v {
          SpirvEntry::Kernel(_) => true,
          _ => false,
        }
      })
      .map(|v| {
        match v {
          SpirvEntry::Kernel(v) => v,
          _ => unreachable!(),
        }
      })
  }
  pub fn vertex_entry(self: Arc<Self>) -> Option<SpirvGraphicsShader> {
    self.entries()
      .find(|v| {
        match v {
          SpirvEntry::Shader(ref s) => {
            s.exe_model() == ExeModel::Vertex
          },
          _ => false,
        }
      })
      .map(|v| {
        match v {
          SpirvEntry::Shader(v) => v,
          _ => unreachable!(),
        }
      })
  }
  pub fn frag_entry(self: Arc<Self>) -> Option<SpirvGraphicsShader> {
    self.entries()
      .find(|v| {
        match v {
          SpirvEntry::Shader(ref s) => {
            s.exe_model() == ExeModel::Fragment
          },
          _ => false,
        }
      })
      .map(|v| {
        match v {
          SpirvEntry::Shader(v) => v,
          _ => unreachable!(),
        }
      })
  }
  pub fn entries(self: Arc<Self>) -> impl ExactSizeIterator<Item = SpirvEntry> {
    (0..self.entries.len())
      .map(move |idx| {
        match self.entries[idx].exe_model {
          ExeModel::Kernel | ExeModel::GLCompute => {
            SpirvEntry::Kernel(SpirvComputeKernel {
              module: self.clone(),
              entry: idx as u16,
            })
          },
          _ => {
            SpirvEntry::Shader(SpirvGraphicsShader {
              module: self.clone(),
              entry: idx as u16,
            })
          },
        }
      })
  }
  pub fn entries_ref(&self) -> impl ExactSizeIterator<Item = SpirvEntryRef> {
    (0..self.entries.len())
      .map(move |idx| {
        match self.entries[idx].exe_model {
          ExeModel::Kernel | ExeModel::GLCompute => {
            SpirvEntryRef::Kernel(SpirvComputeKernelRef {
              module: self,
              entry: idx as u16,
            })
          },
          _ => {
            SpirvEntryRef::Shader(SpirvGraphicsShaderRef {
              module: self,
              entry: idx as u16,
            })
          },
        }
      })
  }
}
impl Eq for SpirvModule { }
impl PartialEq for SpirvModule {
  fn eq(&self, rhs: &Self) -> bool {
    self.entries == rhs.entries &&
      self.pipeline_desc.0.as_ptr() == rhs.pipeline_desc.0.as_ptr()
  }
}
impl PlatformModuleData for SpirvModule {
  fn eq(&self, rhs: &dyn PlatformModuleData) -> bool {
    let rhs: Option<&Self> = Self::downcast_ref(rhs);
    if let Some(rhs) = rhs {
      self == rhs
    } else {
      false
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub struct SpirvComputeKernelRef<'a> {
  module: &'a SpirvModule,
  entry: u16,
}
#[derive(Clone, Copy, Debug)]
pub struct SpirvGraphicsShaderRef<'a> {
  module: &'a SpirvModule,
  entry: u16,
}

impl<'a> SpirvGraphicsShaderRef<'a> {
  #[inline(always)]
  fn entry(&self) -> &SpirvEntryPoint {
    &self.module.entries[self.entry as usize]
  }
  #[inline(always)]
  pub fn exe_model(&self) -> ExeModel {
    self.entry().exe_model
  }
}

#[derive(Clone, Copy, Debug)]
pub enum SpirvEntryRef<'a> {
  Kernel(SpirvComputeKernelRef<'a>),
  Shader(SpirvGraphicsShaderRef<'a>),
}
#[derive(Clone, Debug)]
pub struct SpirvComputeKernel {
  module: Arc<SpirvModule>,
  entry: u16,
}
#[derive(Clone, Debug)]
pub struct SpirvGraphicsShader {
  module: Arc<SpirvModule>,
  entry: u16,
}

impl SpirvGraphicsShader {
  #[inline(always)]
  fn entry(&self) -> &SpirvEntryPoint {
    &self.module.entries[self.entry as usize]
  }
  #[inline(always)]
  pub fn exe_model(&self) -> ExeModel {
    self.entry().exe_model
  }
}

#[derive(Clone, Debug)]
pub enum SpirvEntry {
  Kernel(SpirvComputeKernel),
  Shader(SpirvGraphicsShader),
}

unsafe impl<'a> vk::pipeline::shader::EntryPointAbstract for SpirvComputeKernelRef<'a> {
  type PipelineLayout = StaticPipelineLayoutDesc;
  type SpecializationConstants = ();

  fn module(&self) -> &vk::pipeline::shader::ShaderModule {
    &self.module.entries[self.entry as usize].spirv
  }
  fn name(&self) -> &CStr {
    &self.module.entries[self.entry as usize].name
  }
  fn layout(&self) -> &Self::PipelineLayout { &self.module.pipeline_desc }
}
unsafe impl<'a> vk::pipeline::shader::EntryPointAbstract for SpirvGraphicsShaderRef<'a> {
  type PipelineLayout = StaticPipelineLayoutDesc;
  type SpecializationConstants = ();

  fn module(&self) -> &vk::pipeline::shader::ShaderModule {
    &self.module.entries[self.entry as usize].spirv
  }
  fn name(&self) -> &CStr {
    &self.module.entries[self.entry as usize].name
  }
  fn layout(&self) -> &Self::PipelineLayout {
    &self.module.pipeline_desc
  }
}
unsafe impl<'a> vk::pipeline::shader::GraphicsEntryPointAbstract for SpirvGraphicsShaderRef<'a> {
  type InputDefinition = StaticShaderInterfaceDef;
  type OutputDefinition = StaticShaderInterfaceDef;

  fn input(&self) -> &Self::InputDefinition {
    &self.entry()
      .shader
      .as_ref()
      .unwrap()
      .input
  }
  fn output(&self) -> &Self::OutputDefinition {
    &self.entry()
      .shader
      .as_ref()
      .unwrap()
      .output
  }
  fn ty(&self) -> vk::pipeline::shader::GraphicsShaderType {
    self.entry().vk_ty()
  }
}
unsafe impl vk::pipeline::shader::EntryPointAbstract for SpirvComputeKernel {
  type PipelineLayout = StaticPipelineLayoutDesc;
  type SpecializationConstants = ();

  fn module(&self) -> &vk::pipeline::shader::ShaderModule {
    &self.module.entries[self.entry as usize].spirv
  }
  fn name(&self) -> &CStr {
    &self.module.entries[self.entry as usize].name
  }
  fn layout(&self) -> &Self::PipelineLayout { &self.module.pipeline_desc }
}
unsafe impl vk::pipeline::shader::EntryPointAbstract for SpirvGraphicsShader {
  type PipelineLayout = StaticPipelineLayoutDesc;
  type SpecializationConstants = ();

  fn module(&self) -> &vk::pipeline::shader::ShaderModule {
    &self.module.entries[self.entry as usize].spirv
  }
  fn name(&self) -> &CStr {
    &self.module.entries[self.entry as usize].name
  }
  fn layout(&self) -> &Self::PipelineLayout { &self.module.pipeline_desc }
}
unsafe impl vk::pipeline::shader::GraphicsEntryPointAbstract for SpirvGraphicsShader {
  type InputDefinition = StaticShaderInterfaceDef;
  type OutputDefinition = StaticShaderInterfaceDef;

  fn input(&self) -> &Self::InputDefinition {
    &self.entry()
      .shader
      .as_ref()
      .unwrap()
      .input
  }
  fn output(&self) -> &Self::OutputDefinition {
    &self.entry()
      .shader
      .as_ref()
      .unwrap()
      .output
  }
  fn ty(&self) -> vk::pipeline::shader::GraphicsShaderType {
    self.entry().vk_ty()
  }
}

impl<'a> SpirvEntryRef<'a> {
  pub fn kernel_entry(self) -> Option<SpirvComputeKernelRef<'a>> {
    match self {
      SpirvEntryRef::Kernel(k) => Some(k),
      _ => None,
    }
  }
  pub fn shader_entry(self) -> Option<SpirvGraphicsShaderRef<'a>> {
    match self {
      SpirvEntryRef::Shader(s) => Some(s),
      _ => None,
    }
  }
}
impl SpirvEntry {
  pub fn kernel_entry(self) -> Result<SpirvComputeKernel, Self> {
    match self {
      SpirvEntry::Kernel(k) => Ok(k),
      _ => Err(self),
    }
  }
  pub fn shader_entry(self) -> Result<SpirvGraphicsShader, Self> {
    match self {
      SpirvEntry::Shader(s) => Ok(s),
      _ => Err(self),
    }
  }

  pub fn as_ref(&self) -> SpirvEntryRef {
    match self {
      &SpirvEntry::Kernel(ref k) =>
        SpirvEntryRef::Kernel(SpirvComputeKernelRef {
          module: &*k.module,
          entry: k.entry,
        }),
      &SpirvEntry::Shader(ref s) =>
        SpirvEntryRef::Shader(SpirvGraphicsShaderRef {
          module: &*s.module,
          entry: s.entry,
        }),
    }
  }
}

fn convert_descriptor_img_array(d: &CompilerDescriptorImageArray) -> DescriptorImageDescArray {
  if !d.0 {
    DescriptorImageDescArray::NonArrayed
  } else {
    DescriptorImageDescArray::Arrayed {
      max_layers: (d.1).get(0)
        .map(|&v| v ),
    }
  }
}
fn convert_descriptor_img_desc(d: &CompilerDescriptorImageDesc) -> DescriptorImageDesc {
  DescriptorImageDesc {
    sampled: d.0,
    dimensions: match CompilerDescriptorImageDims::try_from(d.1).unwrap() {
      CompilerDescriptorImageDims::Dim1 => DescriptorImageDescDimensions::OneDimensional,
      CompilerDescriptorImageDims::Dim2 => DescriptorImageDescDimensions::TwoDimensional,
      CompilerDescriptorImageDims::Dim3 => DescriptorImageDescDimensions::ThreeDimensional,
      CompilerDescriptorImageDims::Cube => DescriptorImageDescDimensions::Cube,
    },
    format: (d.2).get(0)
      .cloned()
      .map(compiler_img_format_into_vk),
    multisampled: d.3,
    array_layers: convert_descriptor_img_array(&d.4),
  }
}
fn convert_descriptor_buffer_desc(d: &CompilerDescriptorBufferDesc) -> DescriptorBufferDesc {
  DescriptorBufferDesc {
    dynamic: (d.0)
      .get(0)
      .map(|&v| v ),
    storage: d.1,
  }
}
fn convert_descriptor_desc_ty(d: &CompilerDescriptorDescTy) -> DescriptorDescTy {
  let kind = CompilerDescriptorDescTyKind::try_from(d.0).unwrap();
  match kind {
    CompilerDescriptorDescTyKind::Sampler => {
      DescriptorDescTy::Sampler
    },
    CompilerDescriptorDescTyKind::CombinedImageSampler => {
      let desc = convert_descriptor_img_desc(&d.1[0]);
      DescriptorDescTy::CombinedImageSampler(desc)
    },
    CompilerDescriptorDescTyKind::Image => {
      let desc = convert_descriptor_img_desc(&d.2[0]);
      DescriptorDescTy::Image(desc)
    },
    CompilerDescriptorDescTyKind::TexelBuffer => {
      DescriptorDescTy::TexelBuffer {
        storage: d.3[0].0,
        format: (d.3[0].1).get(0)
          .cloned()
          .map(compiler_img_format_into_vk),
      }
    },
    CompilerDescriptorDescTyKind::InputAttachment => {
      DescriptorDescTy::InputAttachment {
        multisampled: d.4[0].0,
        array_layers: convert_descriptor_img_array(&d.4[0].1),
      }
    },
    CompilerDescriptorDescTyKind::Buffer => {
      let desc = convert_descriptor_buffer_desc(&d.5[0]);
      DescriptorDescTy::Buffer(desc)
    },
  }
}
fn convert_shader_stages(d: &CompilerShaderStages) -> ShaderStages {
  ShaderStages {
    vertex: d.0,
    tessellation_control: d.1,
    tessellation_evaluation: d.2,
    geometry: d.3,
    fragment: d.4,
    compute: d.5,
  }
}
fn convert_descriptor_desc(d: &CompilerDescriptorDesc) -> DescriptorDesc {
  DescriptorDesc {
    ty: convert_descriptor_desc_ty(&d.1),
    array_count: d.2,
    stages: convert_shader_stages(&d.3),
    readonly: d.4,
  }
}

extern "rust-intrinsic" {
  fn geobacter_spirv_pipeline_layout_desc1<F>() -> CompilerDescriptorSetBindingsDesc;
  fn geobacter_spirv_pipeline_layout_desc2<F1, F2>() -> CompilerDescriptorSetBindingsDesc;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StaticPipelineLayoutDesc(pub CompilerDescriptorSetBindingsDesc);
impl StaticPipelineLayoutDesc {
  #[inline(always)]
  pub fn layout_for<F>() -> Self {
    StaticPipelineLayoutDesc(unsafe {
      geobacter_spirv_pipeline_layout_desc1::<F>()
    })
  }
  #[inline(always)]
  pub fn layout_for2<F1, F2>() -> Self {
    StaticPipelineLayoutDesc(unsafe {
      geobacter_spirv_pipeline_layout_desc2::<F1, F2>()
    })
  }

  #[inline(always)]
  pub fn set_iter(&self) -> impl Iterator<Item = StaticDescSet> {
    self.0.iter().map(|&set| StaticDescSet(set) )
  }
}
unsafe impl PipelineLayoutDesc for StaticPipelineLayoutDesc {
  fn num_sets(&self) -> usize {
    self.0
      .last()
      .map(|&(set, _)| set as usize + 1)
      .unwrap_or_default()
  }
  fn num_bindings_in_set(&self, set: usize) -> Option<usize> {
    self.0.binary_search_by_key(&set, |&(set, _)| set as usize )
      .ok()
      .and_then(|set| self.0[set].1.last() )
      .map(|&(binding, ..)| binding as usize + 1)
  }
  fn descriptor(&self, set: usize, binding: usize) -> Option<DescriptorDesc> {
    self.0.binary_search_by_key(&set, |&(set, _)| set as usize )
      .ok()
      .map(|set| &self.0[set].1 )
      .and_then(|bindings| {
        bindings
          .binary_search_by_key(&binding, |&(binding, ..)| binding as usize )
          .ok()
          .map(|id| (bindings, id) )
      })
      .map(|(bindings, id)| &bindings[id] )
      .map(convert_descriptor_desc)
  }

  // we currently don't allow any push constants
  fn num_push_constants_ranges(&self) -> usize { 0 }
  fn push_constants_range(&self, _num: usize) -> Option<PipelineLayoutDescPcRange> {
    None
  }
}
impl Eq for StaticPipelineLayoutDesc { }
impl PartialEq for StaticPipelineLayoutDesc {
  fn eq(&self, rhs: &Self) -> bool {
    self.0.as_ptr() == rhs.0.as_ptr()
  }
}
impl std::hash::Hash for StaticPipelineLayoutDesc {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.0.as_ptr().hash(state)
  }
}
#[derive(Clone, Copy, Debug, Default)]
#[repr(transparent)]
pub struct StaticDescSet(pub CompilerDescriptorBindingsDesc);

impl StaticDescSet {
  #[inline(always)]
  pub fn len(&self) -> u32 {
    (self.0).1.last()
      .map(|&(binding, ..)| binding + 1 )
      .unwrap_or_default()
  }
  #[inline(always)]
  pub fn iter(&self) -> StaticDescSetIter {
    StaticDescSetIter {
      set: (self.0).0,
      iter: (self.0).1.iter(),
      last_binding_id: 0,
      len: self.len(),
      next_binding: None,
    }
  }
}
impl IntoIterator for StaticDescSet {
  type Item = Option<DescriptorDesc>;
  type IntoIter = StaticDescSetIter;
  #[inline(always)]
  fn into_iter(self) -> Self::IntoIter { self.iter() }
}

#[derive(Clone, Debug)]
pub struct StaticDescSetIter {
  set: u32,
  iter: slice::Iter<'static, CompilerDescriptorDesc>,
  last_binding_id: u32,
  len: u32,
  next_binding: Option<&'static CompilerDescriptorDesc>,
}

impl StaticDescSetIter {
  #[inline(always)]
  pub fn set_id(&self) -> u32 { self.set }
}

impl Iterator for StaticDescSetIter {
  type Item = Option<DescriptorDesc>;
  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    if let Some(next_binding) = self.next_binding {
      let last_id = self.last_binding_id;
      self.last_binding_id += 1;
      if last_id == next_binding.0 {
        self.next_binding = None;
        return Some(Some(convert_descriptor_desc(next_binding)))
      } else {
        return Some(None);
      }
    }

    self.next_binding = Some(self.iter.next()?);
    self.next()
  }
  #[inline(always)]
  fn size_hint(&self) -> (usize, Option<usize>) {
    let len = (self.len - self.last_binding_id) as usize;
    (len, Some(len))
  }
}
impl ExactSizeIterator for StaticDescSetIter { }

#[derive(Clone, Copy, Debug, Default)]
pub struct StaticShaderInterfaceDef(CompilerShaderInterfaceDef);

impl StaticShaderInterfaceDef { }
unsafe impl vk::pipeline::shader::ShaderInterfaceDef for StaticShaderInterfaceDef {
  type Iter = StaticShaderInterfaceDefEntryIter;
  fn elements(&self) -> Self::Iter {
    StaticShaderInterfaceDefEntryIter(self.0.iter())
  }
}
impl Eq for StaticShaderInterfaceDef { }
impl PartialEq for StaticShaderInterfaceDef {
  fn eq(&self, rhs: &Self) -> bool {
    self.0.as_ptr() == rhs.0.as_ptr()
  }
}
impl std::hash::Hash for StaticShaderInterfaceDef {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.0.as_ptr().hash(state)
  }
}

#[derive(Clone, Debug)]
pub struct StaticShaderInterfaceDefEntryIter(slice::Iter<'static, CompilerShaderInterfaceDefEntry>);
impl Iterator for StaticShaderInterfaceDefEntryIter {
  type Item = vk::pipeline::shader::ShaderInterfaceDefEntry;
  fn next(&mut self) -> Option<Self::Item> {
    self.0.next()
      .map(|entry| {
        vk::pipeline::shader::ShaderInterfaceDefEntry {
          location: Range {
            start: (entry.0).0,
            end: (entry.0).1,
          },
          format: compiler_img_format_into_vk(entry.1),
          name: (entry.2)
            .get(0)
            .map(|&s| s )
            .map(Cow::Borrowed),
        }
      })
  }
}
impl ExactSizeIterator for StaticShaderInterfaceDefEntryIter {
  fn len(&self) -> usize { self.0.len() }
}

macro_rules! img_format_into_vk {
  (pub enum $ename:ident {
    $($name:ident,)*
  }) => {
    fn compiler_img_format_into_vk(f: u32) -> vk::format::Format {
      const T: &'static [vk::format::Format] = &[
        $(vk::format::Format::$name,)*
      ];
      T.get(f as usize)
        .cloned()
        .unwrap()
    }
  };
}

img_format_into_vk! {
  pub enum CompilerImgFormat {
    R4G4UnormPack8,
    R4G4B4A4UnormPack16,
    B4G4R4A4UnormPack16,
    R5G6B5UnormPack16,
    B5G6R5UnormPack16,
    R5G5B5A1UnormPack16,
    B5G5R5A1UnormPack16,
    A1R5G5B5UnormPack16,
    R8Unorm,
    R8Snorm,
    R8Uscaled,
    R8Sscaled,
    R8Uint,
    R8Sint,
    R8Srgb,
    R8G8Unorm,
    R8G8Snorm,
    R8G8Uscaled,
    R8G8Sscaled,
    R8G8Uint,
    R8G8Sint,
    R8G8Srgb,
    R8G8B8Unorm,
    R8G8B8Snorm,
    R8G8B8Uscaled,
    R8G8B8Sscaled,
    R8G8B8Uint,
    R8G8B8Sint,
    R8G8B8Srgb,
    B8G8R8Unorm,
    B8G8R8Snorm,
    B8G8R8Uscaled,
    B8G8R8Sscaled,
    B8G8R8Uint,
    B8G8R8Sint,
    B8G8R8Srgb,
    R8G8B8A8Unorm,
    R8G8B8A8Snorm,
    R8G8B8A8Uscaled,
    R8G8B8A8Sscaled,
    R8G8B8A8Uint,
    R8G8B8A8Sint,
    R8G8B8A8Srgb,
    B8G8R8A8Unorm,
    B8G8R8A8Snorm,
    B8G8R8A8Uscaled,
    B8G8R8A8Sscaled,
    B8G8R8A8Uint,
    B8G8R8A8Sint,
    B8G8R8A8Srgb,
    A8B8G8R8UnormPack32,
    A8B8G8R8SnormPack32,
    A8B8G8R8UscaledPack32,
    A8B8G8R8SscaledPack32,
    A8B8G8R8UintPack32,
    A8B8G8R8SintPack32,
    A8B8G8R8SrgbPack32,
    A2R10G10B10UnormPack32,
    A2R10G10B10SnormPack32,
    A2R10G10B10UscaledPack32,
    A2R10G10B10SscaledPack32,
    A2R10G10B10UintPack32,
    A2R10G10B10SintPack32,
    A2B10G10R10UnormPack32,
    A2B10G10R10SnormPack32,
    A2B10G10R10UscaledPack32,
    A2B10G10R10SscaledPack32,
    A2B10G10R10UintPack32,
    A2B10G10R10SintPack32,
    R16Unorm,
    R16Snorm,
    R16Uscaled,
    R16Sscaled,
    R16Uint,
    R16Sint,
    R16Sfloat,
    R16G16Unorm,
    R16G16Snorm,
    R16G16Uscaled,
    R16G16Sscaled,
    R16G16Uint,
    R16G16Sint,
    R16G16Sfloat,
    R16G16B16Unorm,
    R16G16B16Snorm,
    R16G16B16Uscaled,
    R16G16B16Sscaled,
    R16G16B16Uint,
    R16G16B16Sint,
    R16G16B16Sfloat,
    R16G16B16A16Unorm,
    R16G16B16A16Snorm,
    R16G16B16A16Uscaled,
    R16G16B16A16Sscaled,
    R16G16B16A16Uint,
    R16G16B16A16Sint,
    R16G16B16A16Sfloat,
    R32Uint,
    R32Sint,
    R32Sfloat,
    R32G32Uint,
    R32G32Sint,
    R32G32Sfloat,
    R32G32B32Uint,
    R32G32B32Sint,
    R32G32B32Sfloat,
    R32G32B32A32Uint,
    R32G32B32A32Sint,
    R32G32B32A32Sfloat,
    R64Uint,
    R64Sint,
    R64Sfloat,
    R64G64Uint,
    R64G64Sint,
    R64G64Sfloat,
    R64G64B64Uint,
    R64G64B64Sint,
    R64G64B64Sfloat,
    R64G64B64A64Uint,
    R64G64B64A64Sint,
    R64G64B64A64Sfloat,
    B10G11R11UfloatPack32,
    E5B9G9R9UfloatPack32,
    D16Unorm,
    X8_D24UnormPack32,
    D32Sfloat,
    S8Uint,
    D16Unorm_S8Uint,
    D24Unorm_S8Uint,
    D32Sfloat_S8Uint,
    BC1_RGBUnormBlock,
    BC1_RGBSrgbBlock,
    BC1_RGBAUnormBlock,
    BC1_RGBASrgbBlock,
    BC2UnormBlock,
    BC2SrgbBlock,
    BC3UnormBlock,
    BC3SrgbBlock,
    BC4UnormBlock,
    BC4SnormBlock,
    BC5UnormBlock,
    BC5SnormBlock,
    BC6HUfloatBlock,
    BC6HSfloatBlock,
    BC7UnormBlock,
    BC7SrgbBlock,
    ETC2_R8G8B8UnormBlock,
    ETC2_R8G8B8SrgbBlock,
    ETC2_R8G8B8A1UnormBlock,
    ETC2_R8G8B8A1SrgbBlock,
    ETC2_R8G8B8A8UnormBlock,
    ETC2_R8G8B8A8SrgbBlock,
    EAC_R11UnormBlock,
    EAC_R11SnormBlock,
    EAC_R11G11UnormBlock,
    EAC_R11G11SnormBlock,
    ASTC_4x4UnormBlock,
    ASTC_4x4SrgbBlock,
    ASTC_5x4UnormBlock,
    ASTC_5x4SrgbBlock,
    ASTC_5x5UnormBlock,
    ASTC_5x5SrgbBlock,
    ASTC_6x5UnormBlock,
    ASTC_6x5SrgbBlock,
    ASTC_6x6UnormBlock,
    ASTC_6x6SrgbBlock,
    ASTC_8x5UnormBlock,
    ASTC_8x5SrgbBlock,
    ASTC_8x6UnormBlock,
    ASTC_8x6SrgbBlock,
    ASTC_8x8UnormBlock,
    ASTC_8x8SrgbBlock,
    ASTC_10x5UnormBlock,
    ASTC_10x5SrgbBlock,
    ASTC_10x6UnormBlock,
    ASTC_10x6SrgbBlock,
    ASTC_10x8UnormBlock,
    ASTC_10x8SrgbBlock,
    ASTC_10x10UnormBlock,
    ASTC_10x10SrgbBlock,
    ASTC_12x10UnormBlock,
    ASTC_12x10SrgbBlock,
    ASTC_12x12UnormBlock,
    ASTC_12x12SrgbBlock,
  }
}
