
/// Various definitions of wrapping types around the compile time generated Vulkan descriptors.
/// For example, `StaticPipelineLayoutDesc` wraps the metadata produced by a custom intrinsic
/// for a specific shader function and implements
/// `vulkano::descriptor::pipeline_layout::PipelineLayoutDesc`.
///

use std::borrow::Cow;
use std::iter::{Iterator, ExactSizeIterator, };
use std::mem::{transmute, transmute_copy, };
use std::slice::{self, from_raw_parts, };
use std::sync::{Arc, };
use std::ops::Range;

use vk::OomError;
use vk::descriptor::descriptor::{DescriptorDesc, DescriptorDescTy,
                                 DescriptorImageDesc,
                                 DescriptorImageDescArray, DescriptorBufferDesc,
                                 ShaderStages, };
use vk::descriptor::descriptor_set::{UnsafeDescriptorSetLayout, };
use vk::descriptor::pipeline_layout::{PipelineLayoutDesc, PipelineLayoutDescPcRange, };
use vk::device::Device;
use lcore::*;

use lcore::ss::{CompilerDescriptorDescTyKind, CompilerDescriptorImageDims, };

pub type PersistentDescriptorSetBuilder<R> = vk::descriptor::descriptor_set::PersistentDescriptorSetBuilder<StaticPipelineLayoutDesc, R>;
pub type PersistentDescriptorSetBuilderArray<R> = vk::descriptor::descriptor_set::PersistentDescriptorSetBuilderArray<StaticPipelineLayoutDesc, R>;

// Note regarding the metadata: many of the slices used here are used inplace of `Option`.
// If the array doesn't have any elements, then the array can be interpreted as `None`.
// Otherwise, the array should only have a single element.

type CompilerVkFormat = u32;

type CompilerDescriptorImageArray = (bool, &'static [u32]);
// vk::format::Format is `#[repr(u32)]`.
type CompilerDescriptorImageDesc = (bool,
                                    u32 /*CompilerDescriptorImageDims*/,
                                    &'static [CompilerVkFormat],
                                    bool,
                                    CompilerDescriptorImageArray
);
type CompilerDescriptorBufferDesc = (&'static [bool], bool);


type CompilerDescriptorDescTy = (u32 /*CompilerDescriptorDescTyKind*/,
                                 // If self.0 is CombinedImageSampler
                                 &'static [CompilerDescriptorImageDesc],
                                 // If self.0 is Image
                                 &'static [CompilerDescriptorImageDesc],
                                 // If self.0 is TexelBuffer
                                 &'static [(bool, &'static [CompilerVkFormat])],
                                 // If self.0 is InputAttachment
                                 &'static [(bool, CompilerDescriptorImageArray)],
                                 // If self.0 is Buffer
                                 &'static [CompilerDescriptorBufferDesc],
);

/// vertex, tessellation_control, tessellation_evalutation, geometry, fragment, compute
type CompilerShaderStages = (bool, bool, bool, bool, bool, bool);
/// ty, array_count, stages, readonly
type CompilerDescriptorDesc = (CompilerDescriptorDescTy, u32, CompilerShaderStages, bool);
type CompilerDescriptorBindingsDesc = &'static [&'static [CompilerDescriptorDesc]];
type CompilerDescriptorSetBindingsDesc = &'static [CompilerDescriptorBindingsDesc];

type CompilerRange = (u32, u32);
type CompilerOptionalName = &'static [&'static str];

type CompilerShaderInterfaceDefEntry = (CompilerRange,
                                        CompilerVkFormat,
                                        CompilerOptionalName);
type CompilerShaderInterfaceDef = &'static [CompilerShaderInterfaceDefEntry];

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
    dimensions: CompilerDescriptorImageDims::from(d.1).into_vk(),
    format: (d.2).get(0)
      .map(|format| unsafe {
        transmute_copy(format)
      }),
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
  let kind = CompilerDescriptorDescTyKind::from(d.0);
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
        format: (d.3[0].1)
          .get(0)
          .map(|format| unsafe {
            transmute_copy(format)
          }),
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
    ty: convert_descriptor_desc_ty(&d.0),
    array_count: d.1,
    stages: convert_shader_stages(&d.2),
    readonly: d.3,
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StaticShaderInterfaceDef(CompilerShaderInterfaceDef);

impl StaticShaderInterfaceDef {
  crate fn new(v: CompilerShaderInterfaceDef) -> Self {
    StaticShaderInterfaceDef(v)
  }
}

unsafe impl vk::pipeline::shader::ShaderInterfaceDef for StaticShaderInterfaceDef {
  type Iter = StaticShaderInterfaceDefEntryIter;
  fn elements(&self) -> Self::Iter {
    StaticShaderInterfaceDefEntryIter(self.0.iter())
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
          format: unsafe {
            transmute(entry.1)
          },
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

extern "rust-intrinsic" {
  /// These functions don't actually use the provided function,
  /// other than to force the function to have the correct parameterization.
  fn __legionella_compute_pipeline_layout_desc<F, Args, Ret>(_: &F)
    -> CompilerDescriptorSetBindingsDesc
    where F: Fn<Args, Output = Ret>;
  fn __legionella_compute_pipeline_required_capabilities<F, Ret, Args>(_: &F)
    -> &'static [u32]
    where F: Fn<Args, Output = Ret>;
  fn __legionella_compute_pipeline_required_extensions<F, Ret, Args>(_: &F)
    -> &'static [&'static str]
    where F: Fn<Args, Output = Ret>;

  crate fn __legionella_compute_descriptor_set_binding<Fm>()
    -> &'static (u32, u32);

  // Note we don't put predicates on the generic params (which would otherwise
  // want Fn<(), Output = ()>). This is done so the geometry and tessellation
  // stages can use () if they aren't used.
  crate fn __legionella_graphics_pipeline_layout_desc<Vs, Gs, Tcs, Tes, Fs>(
    vertex: &Vs,
    geometry: &Gs,
    tess_control: &Tcs,
    tess_eval: &Tes,
    frag: &Fs,
  )
    -> CompilerDescriptorSetBindingsDesc
    where Vs: Fn<(), Output = ()>,
          // optional:
          //Gs: Fn<(), Output = ()>,
          //Tcs: Fn<(), Output = ()>,
          //Tes: Fn<(), Output = ()>,
          Fs: Fn<(), Output = ()>;

  crate fn __legionella_graphics_pipeline_required_capabilities<Vs, Gs, Tcs, Tes, Fs>(
    vertex: &Vs,
    geometry: &Gs,
    tess_control: &Tcs,
    tess_eval: &Tes,
    frag: &Fs,
  )
    -> &'static [u32]
    where Vs: Fn<(), Output = ()>,
          // optional:
          //Gs: Fn<(), Output = ()>,
          //Tcs: Fn<(), Output = ()>,
          //Tes: Fn<(), Output = ()>,
          Fs: Fn<(), Output = ()>;

  crate fn __legionella_graphics_pipeline_required_extensions<Vs, Gs, Tcs, Tes, Fs>(
    vertex: &Vs,
    geometry: &Gs,
    tess_control: &Tcs,
    tess_eval: &Tes,
    frag: &Fs,
  )
    -> &'static [&'static str]
    where Vs: Fn<(), Output = ()>,
          // optional:
          //Gs: Fn<(), Output = ()>,
          //Tcs: Fn<(), Output = ()>,
          //Tes: Fn<(), Output = ()>,
          Fs: Fn<(), Output = ()>;
}

pub fn compute_pipeline_layout_desc<F>(kernel: &F) -> StaticPipelineLayoutDesc
  where F: Fn<(), Output = ()>,
{
  let desc = unsafe {
    __legionella_compute_pipeline_layout_desc(kernel)
  };
  StaticPipelineLayoutDesc(desc)
}
pub fn compute_pipeline_required_capabilities<F>(kernel: &F)
  -> Capabilities<'static, [Capability]>
  where F: Fn<(), Output = ()>,
{
  let raw = unsafe {
    __legionella_compute_pipeline_required_capabilities(kernel)
  };

  let slice = unsafe {
    from_raw_parts(raw.as_ptr() as *const Capability,
                   raw.len())
  };

  Capabilities::new(slice)
}
pub fn compute_pipeline_required_extensions<F>(kernel: &F)
  -> &'static [&'static str]
  where F: Fn<(), Output = ()>,
{
  unsafe {
    __legionella_compute_pipeline_required_extensions(kernel)
  }
}

pub fn graphics_pipeline_layout_desc<Vs, Gs, Tcs, Tes, Fs>(
  vertex: &Vs,
  geometry: Option<&Gs>,
  tess: Option<(&Tcs, &Tes)>,
  frag: &Fs,
)
  -> StaticPipelineLayoutDesc
  where Vs: Fn<(), Output = ()>,
        Gs: Fn<(), Output = ()>,
        Tcs: Fn<(), Output = ()>,
        Tes: Fn<(), Output = ()>,
        Fs: Fn<(), Output = ()>
{
  let desc = unsafe {
    match (geometry, tess) {
      (None, None) => {
        __legionella_graphics_pipeline_layout_desc(vertex, &(),
                                                   &(), &(),
                                                   frag)
      },
      (Some(geometry), None) => {
        __legionella_graphics_pipeline_layout_desc(vertex, geometry,
                                                   &(), &(),
                                                   frag)
      },
      (None, Some((control, eval))) => {
        __legionella_graphics_pipeline_layout_desc(vertex, &(),
                                                   control, eval,
                                                   frag)
      },
      (Some(geometry), Some((control, eval))) => {
        __legionella_graphics_pipeline_layout_desc(vertex, geometry,
                                                   control, eval,
                                                   frag)
      },
    }
  };

  StaticPipelineLayoutDesc(desc)
}
pub fn graphics_pipeline_required_capabilities<Vs, Gs, Tcs, Tes, Fs>(
  vertex: &Vs,
  geometry: Option<&Gs>,
  tess: Option<(&Tcs, &Tes)>,
  frag: &Fs,
)
  -> Capabilities<'static, [Capability]>
  where Vs: Fn<(), Output = ()>,
        Gs: Fn<(), Output = ()>,
        Tcs: Fn<(), Output = ()>,
        Tes: Fn<(), Output = ()>,
        Fs: Fn<(), Output = ()>
{
  let raw = unsafe {
    match (geometry, tess) {
      (None, None) => {
        __legionella_graphics_pipeline_required_capabilities(vertex, &(),
                                                             &(), &(),
                                                             frag)
      },
      (Some(geometry), None) => {
        __legionella_graphics_pipeline_required_capabilities(vertex, geometry,
                                                             &(), &(),
                                                             frag)
      },
      (None, Some((control, eval))) => {
        __legionella_graphics_pipeline_required_capabilities(vertex, &(),
                                                             control, eval,
                                                             frag)
      },
      (Some(geometry), Some((control, eval))) => {
        __legionella_graphics_pipeline_required_capabilities(vertex, geometry,
                                                             control, eval,
                                                             frag)
      },
    }
  };

  let slice = unsafe {
    from_raw_parts(raw.as_ptr() as *const Capability,
                   raw.len())
  };

  Capabilities::new(slice)
}
pub fn graphics_pipeline_required_extensions<Vs, Gs, Tcs, Tes, Fs>(
  vertex: &Vs,
  geometry: Option<&Gs>,
  tess: Option<(&Tcs, &Tes)>,
  frag: &Fs,
)
  -> &'static [&'static str]
  where Vs: Fn<(), Output = ()>,
        Gs: Fn<(), Output = ()>,
        Tcs: Fn<(), Output = ()>,
        Tes: Fn<(), Output = ()>,
        Fs: Fn<(), Output = ()>
{
  unsafe {
    match (geometry, tess) {
      (None, None) => {
        __legionella_graphics_pipeline_required_extensions(vertex, &(),
                                                           &(), &(),
                                                           frag)
      },
      (Some(geometry), None) => {
        __legionella_graphics_pipeline_required_extensions(vertex, geometry,
                                                           &(), &(),
                                                           frag)
      },
      (None, Some((control, eval))) => {
        __legionella_graphics_pipeline_required_extensions(vertex, &(),
                                                           control, eval,
                                                           frag)
      },
      (Some(geometry), Some((control, eval))) => {
        __legionella_graphics_pipeline_required_extensions(vertex, geometry,
                                                           control, eval,
                                                           frag)
      },
    }
  }
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct StaticPipelineLayoutDesc(CompilerDescriptorSetBindingsDesc);

impl StaticPipelineLayoutDesc {
  crate fn new(v: CompilerDescriptorSetBindingsDesc) -> Self {
    StaticPipelineLayoutDesc(v)
  }
  pub fn create_set_layouts(&self, device: &Arc<Device>)
    -> Result<Vec<UnsafeDescriptorSetLayout>, OomError>
  {
    let mut out = Vec::with_capacity(self.num_sets());
    for set in 0..self.num_sets() {
      let bindings = self.num_bindings_in_set(set)
        .unwrap_or_default();

      let descriptors = (0..bindings)
        .map(|binding| {
          self.descriptor(set, binding)
        });

      let layout = UnsafeDescriptorSetLayout::new(device.clone(),
                                                  descriptors)?;
      out.push(layout);
    }

    Ok(out)
  }
  pub fn debug(&self) {
    println!("len = {}", self.0.len());
    if let Some(set) = self.0.get(0) {
      println!("set len = {}", set.len());
      for (idx, set_binding) in set.iter().enumerate() {
        if let Some(set_binding) = set_binding.get(0) {
          match CompilerDescriptorDescTyKind::from((set_binding.0).0) {
            CompilerDescriptorDescTyKind::Buffer => {
              println!("set binding #{} desc: Buffer", idx);
              assert_eq!(((set_binding.0).5).len(), 1, "missing buffer desc");
              println!("set binding #{} buffer desc: dynamic = {:?}",
                       idx, (set_binding.0).5[0].0.get(0));
              println!("set binding #{} buffer desc: storage = {}",
                       idx, (set_binding.0).5[0].1);
            },
            _ => unimplemented!("idx {}", (set_binding.0).0),
          }
        } else {
          println!("set binding #{}: None", idx)
        }
      }
    }
  }
}

unsafe impl PipelineLayoutDesc for StaticPipelineLayoutDesc {
  fn num_sets(&self) -> usize {
    self.0.len()
  }
  fn num_bindings_in_set(&self, set: usize) -> Option<usize> {
    self.0.get(set)
      .map(|bindings| bindings.len() )
  }
  fn descriptor(&self, set: usize, binding: usize) -> Option<DescriptorDesc> {
    self.0.get(set)
      .and_then(|bindings| bindings.get(binding) )
      .and_then(|bindings_opt| bindings_opt.get(0) )
      .map(|binding| {
        convert_descriptor_desc(binding)
      })
  }

  // we currently don't allow any push constants
  fn num_push_constants_ranges(&self) -> usize { 0 }
  fn push_constants_range(&self, _num: usize) -> Option<PipelineLayoutDescPcRange> {
    None
  }
}
