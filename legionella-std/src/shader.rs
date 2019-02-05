
/// Types and functions for shader functions

use std::ops::{Deref, };
use std::slice::{from_raw_parts, };

use hsa_core::kernel::{KernelId, kernel_id_for, opt_kernel_id_for,
                       OptionalFn, };
use hsa_core::unit::*;
use spirv_help::*;

use lcore::{Capabilities, Capability, };
use vk_help::{StaticPipelineLayoutDesc,
              __legionella_graphics_pipeline_layout_desc,
              __legionella_graphics_pipeline_required_capabilities,
              __legionella_graphics_pipeline_required_extensions,
};

use super::{exe_model, ExeModel, };

  // TODO expand these

#[legionella(capabilities(any(Geometry, Tessellation, RayTracingNV, Fragment)),
             spirv_builtin = "PrimitiveId",
             storage_class = "Input")]
static PRIMITIVE_ID: u32 = 0;

#[legionella(capabilities = "Vertex",
             spirv_builtin = "VertexIndex",
             storage_class = "Input")]
static VERTEX_INDEX: u32 = 0;

#[legionella(capabilities(all(not(Kernel), Shader)),
             spirv_builtin = "ViewIndex",
             storage_class = "Input")]
static VIEW_INDEX: u32 = 0;

#[legionella(capabilities(all(not(Kernel), Shader)),
             exe_model(any(Geometry, TessellationControl,
                           TessellationEval)),
             spirv_builtin = "Position",
             storage_class = "Input")]
static POSITION_IN: Vec4<f32> = Vec4::new_c([0.0, 0.0, 0.0, 1.0]);

// this isn't unsafe; the `Output` storage class is per work item.
pub struct Position;
impl Position {
  #[legionella(exe_model(any(Vertex, Geometry, TessellationControl,
                             TessellationEval)))]
  pub fn write(&self, v: Vec4<f32>) {
    unsafe { POSITION_OUT = v; }
  }
}
impl Deref for Position {
  type Target = Vec4<f32>;

  #[legionella(exe_model(any(Geometry, TessellationControl,
                             TessellationEval)))]
  fn deref(&self) -> &Vec4<f32> {
    unsafe { &POSITION_IN }
  }
}

#[legionella(capabilities = "Shader",
             exe_model(any(Vertex, Geometry, TessellationControl,
                           TessellationEval)),
             spirv_builtin = "Position",
             storage_class = "Output")]
static mut POSITION_OUT: Vec4<f32> = Vec4::new_c([0.0, 0.0, 0.0, 1.0]);

pub fn position() -> &'static Position {
  const P: &'static Position = &Position;
  P
}

macro_rules! g {
  ($name:ident) => {
    g(unsafe { &$name }) as _
  };
}
macro_rules! gv4 {
  ($name:ident) => {{
    let v = g(unsafe { &$name });
    Vec4::new([v.w() as _, v.x() as _, v.y() as _, v.z() as _, ].into())
  }};
}

pub fn primitive_id() -> usize { g!(PRIMITIVE_ID) }
pub fn vertex_index() -> usize { g!(VERTEX_INDEX) }
pub fn view_index() -> usize { g!(VIEW_INDEX) }

extern "rust-intrinsic" {
  /// These cause our driver to run the compile time checked for the
  /// provided function.
  fn __legionella_check_vertex_shader<F>(f: &F)
    where F: Fn<(), Output=()>;
  fn __legionella_check_geometry_shader<F>(f: &F)
    where F: OptionalFn<(), Output=()>;
  fn __legionella_check_tessellation_control_shader<F>(f: &F)
    where F: OptionalFn<(), Output=()>;
  fn __legionella_check_tessellation_eval_shader<F>(f: &F)
    where F: OptionalFn<(), Output=()>;
  fn __legionella_check_fragment_shader<F>(f: &F)
    where F: Fn<(), Output=()>;
}

macro_rules! required_shader_id {
  ($pub_name:ident $intrinsic_name:ident) => {

pub fn $pub_name<F>(f: &F) -> KernelId
  where F: Fn<(), Output=()>,
{
  unsafe {
    $intrinsic_name(f);
  }

  kernel_id_for(f)
}

  }
}
macro_rules! optional_shader_id {
  ($pub_name:ident $intrinsic_name:ident) => {

pub fn $pub_name<F>(f: &F) -> Option<KernelId>
  where F: OptionalFn<(), Output=()>,
{
  if f.is_none() { return None; }

  unsafe {
    $intrinsic_name(f);
  }

  opt_kernel_id_for(f)
}

  }
}

required_shader_id!(vertex_shader_id __legionella_check_vertex_shader);
optional_shader_id!(geometry_shader_id __legionella_check_geometry_shader);
optional_shader_id!(tessellation_control_shader_id __legionella_check_tessellation_control_shader);
optional_shader_id!(tessellation_eval_shader_id __legionella_check_tessellation_eval_shader);
required_shader_id!(fragment_shader_id __legionella_check_fragment_shader);

pub fn is_vertex_shader() -> bool { exe_model() == ExeModel::Vertex }
pub fn is_geometry_shader() -> bool { exe_model() == ExeModel::Geometry }
pub fn is_tessellation_control_shader() -> bool { exe_model() == ExeModel::TessellationControl }
pub fn is_tessellation_eval_shader() -> bool { exe_model() == ExeModel::TessellationEval }
pub fn is_fragment_shader() -> bool { exe_model() == ExeModel::Fragment }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct GraphicsDesc {
  pub vertex: KernelId,
  pub geometry: Option<KernelId>,
  pub tess: Option<(KernelId, KernelId)>,
  pub fragment: KernelId,

  pub pipeline_desc: StaticPipelineLayoutDesc,
  pub capabilities: Capabilities<'static, [Capability]>,
  pub extensions: &'static [&'static str],
}
impl GraphicsDesc {
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct GraphicsDescBuilder<Vs, Gs, Tsc, Tse, Fs>
  where Vs: Fn<(), Output = ()>,
        Gs: OptionalFn<(), Output = ()>,
        Tsc: OptionalFn<(), Output = ()>,
        Tse: OptionalFn<(), Output = ()>,
        Fs: Fn<(), Output = ()>,
{
  vertex: Vs,
  geometry: Gs,
  tess: (Tsc, Tse),
  fragment: Fs,
}
impl<Vs, Fs> GraphicsDescBuilder<Vs, (), (), (), Fs>
  where Vs: Fn<(), Output = ()>,
        Fs: Fn<(), Output = ()>,
{
  pub fn new(vertex: Vs, fragment: Fs) -> Self {
    GraphicsDescBuilder {
      vertex,
      geometry: (),
      tess: ((), ()),
      fragment,
    }
  }
}
impl<Vs, Gs, Tsc, Tse, Fs> GraphicsDescBuilder<Vs, Gs, Tsc, Tse, Fs>
  where Vs: Fn<(), Output = ()>,
        Gs: OptionalFn<(), Output = ()>,
        Tsc: OptionalFn<(), Output = ()>,
        Tse: OptionalFn<(), Output = ()>,
        Fs: Fn<(), Output = ()>,
{
  pub fn with_geometry<Gs2>(self, geometry: Gs2)
    -> GraphicsDescBuilder<Vs, Gs2, Tsc, Tse, Fs>
    where Gs2: Fn<(), Output = ()> + OptionalFn<(), Output = ()>,
  {
    let GraphicsDescBuilder {
      vertex,
      tess,
      fragment,
      ..
    } = self;

    GraphicsDescBuilder {
      vertex,
      geometry,
      tess,
      fragment,
    }
  }
  pub fn without_geometry(self)
    -> GraphicsDescBuilder<Vs, (), Tsc, Tse, Fs>
  {
    let GraphicsDescBuilder {
      vertex,
      tess,
      fragment,
      ..
    } = self;

    GraphicsDescBuilder {
      vertex,
      geometry: (),
      tess,
      fragment,
    }
  }
  pub fn with_tessellation<Tsc2, Tse2>(self, control: Tsc2, eval: Tse2)
    -> GraphicsDescBuilder<Vs, Gs, Tsc2, Tse2, Fs>
    where Tsc2: Fn<(), Output = ()> + OptionalFn<(), Output = ()>,
          Tse2: Fn<(), Output = ()> + OptionalFn<(), Output = ()>,
  {
    let GraphicsDescBuilder {
      vertex,
      geometry,
      fragment,
      ..
    } = self;

    GraphicsDescBuilder {
      vertex,
      geometry,
      tess: (control, eval),
      fragment,
    }
  }
  pub fn without_tessellation(self)
    -> GraphicsDescBuilder<Vs, Gs, (), (), Fs>
  {
    let GraphicsDescBuilder {
      vertex,
      geometry,
      fragment,
      ..
    } = self;

    GraphicsDescBuilder {
      vertex,
      geometry,
      tess: ((), ()),
      fragment,
    }
  }
}

impl<Vs, Gs, Tsc, Tse, Fs> GraphicsDescBuilder<Vs, Gs, Tsc, Tse, Fs>
  where Vs: Fn<(), Output = ()>,
        Gs: OptionalFn<(), Output = ()>,
        Tsc: OptionalFn<(), Output = ()>,
        Tse: OptionalFn<(), Output = ()>,
        Fs: Fn<(), Output = ()>,
{
  pub fn build(self) -> GraphicsDesc {
    let GraphicsDescBuilder {
      vertex,
      geometry,
      tess: (control, eval),
      fragment,
    } = self;

    let vertex_id = vertex_shader_id(&vertex);
    let geometry_id = geometry_shader_id(&geometry);
    let control_id = tessellation_control_shader_id(&control);
    let eval_id = tessellation_eval_shader_id(&eval);
    let fragment_id = fragment_shader_id(&fragment);

    let pipeline_desc = unsafe {
      __legionella_graphics_pipeline_layout_desc(&vertex,
                                                 &geometry,
                                                 &control, &eval,
                                                 &fragment)
    };
    let caps = unsafe {
      __legionella_graphics_pipeline_required_capabilities(&vertex,
                                                           &geometry,
                                                           &control, &eval,
                                                           &fragment)
    };
    let caps = unsafe {
      from_raw_parts(caps.as_ptr() as *const Capability,
                     caps.len())
    };
    let caps = Capabilities::new(caps);

    let exts = unsafe {
      __legionella_graphics_pipeline_required_extensions(&vertex,
                                                         &geometry,
                                                         &control, &eval,
                                                         &fragment)
    };

    GraphicsDesc {
      vertex: vertex_id,
      geometry: geometry_id,
      tess: control_id
        .map(move |control| {
          (control, eval_id.unwrap())
        }),
      fragment: fragment_id,

      pipeline_desc: StaticPipelineLayoutDesc::new(pipeline_desc),
      capabilities: caps,
      extensions: exts,
    }
  }
}
