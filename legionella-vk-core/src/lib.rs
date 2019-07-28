#![feature(core_intrinsics)]

extern crate legionella_shared_defs as shared_defs;
extern crate vulkano as vk;

use std::iter::*;

use shared_defs::platform::vk::*;

pub mod ss;

macro_rules! const_array {
  ($ty:ty, $($elem:expr,)*) => {{
    const C: &'static [$ty] = &[
      $($elem,)*
    ];
    C
  }};
}

macro_rules! mk_struct_if {
  (false => $name:ident) => {
    // empty
  };
  ( => $name:ident) => {
    mk_struct_if!(true => $name);
  };
  (true => $name:ident) => {
    #[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
    pub struct $name;
  };
}

macro_rules! mk_enum {
  (pub enum $enum_name:ident {
    $($name:ident = {
      id: $id:expr,
      implies: [$($implies:ident,)*],
      $(ext: [$($vk_ext:ident,)*],)?
      $(spirv_ext: [$($spirv_ext:ident,)*],)?
      $(marker_type: $condition:ident,)?
    },)*
  }) => {

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u32)]
pub enum $enum_name {
  $($name = $id,)*
}

$(
mk_struct_if!($($condition)? => $name);
)*

$(impl From<$name> for $enum_name {
  fn from(_: $name) -> $enum_name {
    $enum_name::$name
  }
})*

impl ::std::str::FromStr for $enum_name {
  type Err = ();
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      $(stringify!($name) => Ok($enum_name::$name),)*
      _ => Err(()),
    }
  }
}

impl $enum_name {
  fn __implicit_deps(&self) -> &'static [Capability] {
    match self {
      $(&$enum_name::$name => {
         const C: &'static [Capability] = &[
           $(Capability::$implies,)*
         ];
         C
      },)*
    }
  }

  pub fn required_extensions(&self) -> &'static [&'static str] {
    match self {
      $(&$enum_name::$name => {
        const_array![&'static str, $($(stringify!($vk_ext),)*)?]
      },)*
    }
  }
  pub fn required_spirv_extensions(&self) -> &'static [&'static str] {
    match self {
      $(&$enum_name::$name => {
        const_array![&'static str, $($(stringify!($spirv_ext),)*)?]
      },)*
    }
  }
}

  }
}

macro_rules! mk_cap_enum {
  (pub enum Capability {
    $($name:ident = {
      id: $id:expr,
      implies: [$($implies:ident,)*],
      $(ext: [$($vk_ext:ident,)*],)?
      $(spirv_ext: [$($spirv_ext:ident,)*],)?
      $(features: [$($feature:ident,)*],)?
    },)*
  }) => {
    mk_enum! {
      pub enum Capability {
        $($name = {
          id: $id,
          implies: [$($implies,)*],
          $(ext: [$($vk_ext,)*],)?
        },)*
      }
    }

    impl Capability {
      pub fn merge_required_features(&self, features: &mut vk::device::Features) {
        match self {
          $(&Capability::$name => {
            $($(features.$feature = true;)*)?
          },)*
        }
      }
    }

  };
}

mk_cap_enum! {

pub enum Capability {
  Matrix = {
    id: 0,
    implies: [],
    ext: [],
  },
  Shader = {
    id: 1,
    implies: [Matrix, ],
    ext: [],
  },
  Geometry = {
    id: 2,
    implies: [Shader, ],
    ext: [],
    features: [geometry_shader, ],
  },
  Tessellation = {
    id: 3,
    implies: [Shader, ],
    ext: [],
    features: [tessellation_shader, ],
  },
  Addresses = {
    id: 4,
    implies: [],
    ext: [VK_EXT_buffer_device_address, ],
    features: [buffer_device_address, ],
  },
  Linkage = {
    id: 5,
    implies: [],
    ext: [],
  },
  Kernel = {
    id: 6,
    implies: [],
    ext: [],
  },
  Vector16 = {
    id: 7,
    implies: [Kernel, ],
    ext: [],
  },
  Float16Buffer = {
    id: 8,
    implies: [Kernel, ],
    ext: [],
  },
  Float16 = {
    id: 9,
    implies: [],
    ext: [],
  },
  Float64 = {
    id: 10,
    implies: [],
    ext: [],
    features: [shader_f3264, ],
  },
  Int64 = {
    id: 11,
    implies: [],
    ext: [],
    features: [shader_int64, ],
  },
  Int64Atomics = {
    id: 12,
    implies: [Int64, ],
    ext: [VK_KHR_shader_atomic_int64, ],
  },
  ImageBasic = {
    id: 13,
    implies: [Kernel, ],
    ext: [],
  },
  ImageReadWrite = {
    id: 14,
    implies: [ImageBasic, ],
    ext: [],
  },
  ImageMipmap = {
    id: 15,
    implies: [ImageBasic, ],
    ext: [],
  },
  // skip 16
  Pipes = {
    id: 17,
    implies: [Kernel, ],
    ext: [],
  },
  Groups = {
    id: 18,
    implies: [],
    ext: [],
  },
  DeviceEnqueue = {
    id: 19,
    implies: [Kernel, ],
    ext: [],
  },
  LiteralSampler = {
    id: 20,
    implies: [Kernel, ],
    ext: [],
  },
  AtomicStorage = {
    id: 21,
    implies: [Shader, ],
    ext: [],
  },
  Int16 = {
    id: 22,
    implies: [],
    ext: [],
    features: [shader_int16, ],
  },
  TessellationPointSize = {
    id: 23,
    implies: [Tessellation, ],
    ext: [],
    features: [shader_tessellation_and_geometry_point_size, ],
  },
  GeometryPointSize = {
    id: 24,
    implies: [Geometry, ],
    ext: [],
    features: [shader_tessellation_and_geometry_point_size, ],
  },
  ImageGatherExtended = {
    id: 25,
    implies: [Shader, ],
    ext: [],
    features: [shader_image_gather_extended, ],
  },
  // skip 26
  StorageImageMultisample = {
    id: 27,
    implies: [Shader, ],
    ext: [],
    features: [shader_storage_image_multisample, ],
  },
  UniformBufferArrayDynamicIndexing = {
    id: 28,
    implies: [Shader, ],
    ext: [],
    features: [shader_uniform_buffer_array_dynamic_indexing, ],
  },
  SampledImageArrayDynamicIndexing = {
    id: 29,
    implies: [Shader, ],
    ext: [],
    features: [shader_sampled_image_array_dynamic_indexing, ],
  },
  StorageBufferArrayDynamicIndexing = {
    id: 30,
    implies: [Shader, ],
    ext: [],
    features: [shader_storage_buffer_array_dynamic_indexing, ],
  },
  GenericPointer = {
    id: 38,
    implies: [Addresses, ],
    ext: [],
  },
  Int8 = {
    id: 39,
    implies: [],
  },
  InputAttachment = {
    id: 40,
    implies: [Shader, ],
    ext: [],
  },
  Sampled1D = {
    id: 43,
    implies: [],
    ext: [],
  },
  Image1D = {
    id: 44,
    implies: [Sampled1D, ],
    ext: [],
  },
  StorageImageExtendedFormats = {
    id: 49,
    implies: [Shader, ],
    ext: [],
  },
  DerivativeControl = {
    id: 51,
    implies: [Shader, ],
    ext: [],
  },
  TransformFeedback = {
    id: 53,
    implies: [Shader, ],
    ext: [],
  },
  GroupNonUniform = {
    id: 61,
    implies: [],
  },
  VariablePointersStorageBuffer = {
    id: 4441,
    implies: [Shader, ],
    spirv_ext: [SPV_KHR_variable_pointers, ],
  },
  VariablePointers = {
    id: 4442,
    implies: [VariablePointersStorageBuffer, ],
    spirv_ext: [SPV_KHR_variable_pointers, ],
  },
}

}

impl Capability {
  pub fn implicitly_declares(&self) -> &'static [Capability] {
    self.__implicit_deps()
  }
}

#[derive(Debug, Hash, Eq, PartialEq)]
pub struct Capabilities<'a, T>(&'a T)
  where &'a T: IntoIterator<Item = &'a Capability>,
        T: ?Sized;

impl<'a, T> Capabilities<'a, T>
  where &'a T: IntoIterator<Item = &'a Capability>,
        T: ?Sized
{
  pub fn new(caps: &'a T) -> Self {
    Capabilities(caps)
  }
  pub fn iter(&self) -> <&'a T as IntoIterator>::IntoIter {
    self.0.into_iter()
  }

  pub fn merge_required_features(&self, features: &mut vk::device::Features) {
    for cap in self.iter() {
      cap.merge_required_features(features);
    }
  }
}

impl<'a, T> Copy for Capabilities<'a, T>
  where &'a T: IntoIterator<Item = &'a Capability>,
        T: ?Sized
{ }
impl<'a, T> Clone for Capabilities<'a, T>
  where &'a T: IntoIterator<Item = &'a Capability>,
        T: ?Sized
{
  fn clone(&self) -> Self { *self }
}

mk_enum! {

pub enum ExecutionModel {
  Vertex = {
    id: 0,
    implies: [Shader, ],
    ext: [],
  },
  TessellationControl = {
    id: 1,
    implies: [Tessellation, ],
    ext: [],
  },
  TessellationEval = {
    id: 2,
    implies: [Tessellation, ],
    ext: [],
  },
  Geometry = {
    id: 3,
    implies: [Geometry, ],
    ext: [],
    marker_type: false,
  },
  Fragment = {
    id: 4,
    implies: [Shader, ],
    ext: [],
  },
  GLCompute = {
    id: 5,
    implies: [Shader, ],
    ext: [],
  },
  Kernel = {
    id: 6,
    implies: [Kernel, ],
    ext: [],
    marker_type: false,
  },
}

}

impl ExecutionModel {
  pub fn required_capabilities(&self) -> &'static [Capability] {
    self.__implicit_deps()
  }
}
impl Into<ExeModel> for ExecutionModel {
  fn into(self) -> ExeModel {
    match self {
      ExecutionModel::Kernel => ExeModel::Kernel,
      ExecutionModel::Vertex => ExeModel::Vertex,
      ExecutionModel::TessellationControl => ExeModel::TessellationControl,
      ExecutionModel::TessellationEval => ExeModel::TessellationEval,
      ExecutionModel::Geometry => ExeModel::Geometry,
      ExecutionModel::Fragment => ExeModel::Fragment,
      ExecutionModel::GLCompute => ExeModel::GLCompute,
    }
  }
}
impl From<ExeModel> for ExecutionModel {
  fn from(v: ExeModel) -> Self {
    match v {
      ExeModel::Kernel => ExecutionModel::Kernel,
      ExeModel::Vertex => ExecutionModel::Vertex,
      ExeModel::TessellationControl => ExecutionModel::TessellationControl,
      ExeModel::TessellationEval => ExecutionModel::TessellationEval,
      ExeModel::Geometry => ExecutionModel::Geometry,
      ExeModel::Fragment => ExecutionModel::Fragment,
      ExeModel::GLCompute => ExecutionModel::GLCompute,
    }
  }
}

macro_rules! mk_builtin_enum {
  (pub enum $enum_name:ident {
    $($name:ident = {
      id: $id:expr,
      requires: [$($requires:ident,)*],
      $(ext: [$($ext:ident,)*],)?
      exe_model: [$(($model:ident, $storage_class:ident),)*],
      $(marker_type: $condition:expr,)?
    },)*
  }) => {
    mk_enum! {
      pub enum $enum_name {
        $($name = {
          id: $id,
          implies: [$($requires,)*],
          $(ext: [$($ext,)*],)?
          $(marker_type: $condition,)?
        },)*
      }
    }

    impl $enum_name {
      pub fn required_capabilities(&self) -> &'static [Capability] {
        self.__implicit_deps()
      }

      pub fn exe_model_storage_class(&self) -> &'static [(ExecutionModel, StorageClass)] {
        match self {
          $(&$enum_name::$name => {
            const_array![(ExecutionModel, StorageClass), $((ExecutionModel::$model,
                                                            StorageClass::$storage_class),)*]
          },)*
        }
      }
    }
  };
}

mk_builtin_enum! {

pub enum Builtin {
  Position = {
    id: 0,
    requires: [Shader, ],
    ext: [],
    exe_model: [(Vertex, Output),
                (TessellationControl, Input),
                (TessellationControl, Output),
                (TessellationEval, Input),
                (TessellationEval, Output),
                (Geometry, Input),
                (Geometry, Output),
                ],
  },
  PrimitiveId = {
    id: 7,
    requires: [Geometry, Tessellation, ],
    ext: [],
    exe_model: [(TessellationControl, Input),
                (TessellationEval, Output),
                (Geometry, Input),
                (Geometry, Output),
                (Fragment, Input),
                ],
  },
  NumWorkgroups = {
    id: 24,
    requires: [],
    exe_model: [(Kernel, Input), ],
  },
  WorkgroupSize = {
    id: 25,
    requires: [],
    exe_model: [(Kernel, Input), ],
  },
  WorkgroupId = {
    id: 26,
    requires: [],
    exe_model: [(Kernel, Input), ],
  },
  LocalInvocationId = {
    id: 27,
    requires: [],
    exe_model: [(Kernel, Input), ],
  },
  GlobalInvocationId = {
    id: 28,
    requires: [],
    exe_model: [(Kernel, Input), ],
  },
  LocalInvocationIndex = {
    id: 29,
    requires: [],
    exe_model: [(Kernel, Input), ],
  },
  SubgroupSize = {
    id: 36,
    requires: [Kernel, GroupNonUniform, ],
    exe_model: [(Vertex, Input),
                (TessellationControl, Input),
                (TessellationEval, Input),
                (Geometry, Input),
                (Fragment, Input),
                (Kernel, Input),
               ],
  },
  NumSubgroups = {
    id: 38,
    requires: [Kernel, GroupNonUniform, ],
    exe_model: [(Kernel, Input),],
  },

  VertexIndex = {
    id: 42,
    requires: [Shader, ],
    ext: [],
    exe_model: [(Vertex, Input), ],
  },

}

}

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub enum ExecutionMode {
  LocalSize {
    x: u32,
    y: u32,
    z: u32,
  },
  Xfb,
}
impl ExecutionMode {
  pub fn required_capabilities(&self) -> &'static [Capability] {
    match self {
      ExecutionMode::LocalSize { .. } => {
        const_array![Capability, Capability::Kernel, ]
      },
      ExecutionMode::Xfb => {
        const_array![Capability, Capability::TransformFeedback, ]
      },
    }
  }
  pub fn required_extensions(&self) -> &'static [&'static str] {
    const C: &'static [&'static str] = &[];
    C
  }
}

mk_enum! {

pub enum StorageClass {
  UniformConstant = {
    id: 0,
    implies: [],
  },
  Input = {
    id: 1,
    implies: [],
  },
  Uniform = {
    id: 2,
    implies: [Shader, ],
  },
  Output = {
    id: 3,
    implies: [Shader, ],
  },
  Workgroup = {
    id: 4,
    implies: [],
  },
  CrossWorkgroup = {
    id: 5,
    implies: [],
  },
  Private = {
    id: 6,
    implies: [Shader, ],
  },
  Function = {
    id: 7,
    implies: [],
  },
  Generic = {
    id: 8,
    implies: [GenericPointer, ],
  },
  PushConstant = {
    id: 9,
    implies: [Shader, ],
  },
  AtomicCounter = {
    id: 10,
    implies: [AtomicStorage, ],
  },
  Image = {
    id: 11,
    implies: [],
  },
  StorageBuffer = {
    id: 12,
    implies: [Shader, ],
    spirv_ext: [SPV_KHR_storage_buffer_storage_class, ],
  },
}

}

impl StorageClass {
  pub fn required_capabilities(&self) -> &'static [Capability] {
    self.__implicit_deps()
  }
}
