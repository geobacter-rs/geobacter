use vk;

use lstd::vk_help::{StaticPipelineLayoutDesc, StaticShaderInterfaceDef, };

#[derive(Debug)]
pub struct SpirvModule {
  entry: CString,
  exe_model: ExecutionModel,
  /// If Some(..), this function is a shader; if None, it is a kernel.
  shader: Option<CodegenShaderInterface>,
  spirv: vk::pipeline::shader::ShaderModule,
  pipeline_desc: StaticPipelineLayoutDesc,
}
impl SpirvModule {
  pub fn descriptor_set(&self) -> &StaticPipelineLayoutDesc {
    &self.pipeline_desc
  }
  pub fn entry(self: Arc<Self>) -> SpirvEntry {
    match self.exe_model {
      ExecutionModel::Kernel |
      ExecutionModel::GLCompute => {
        SpirvEntry::Kernel(SpirvComputeKernel {
          module: self,
        })
      },
      _ => {
        SpirvEntry::Shader(SpirvGraphicsShader {
          module: self,
        })
      },
    }
  }
  pub fn entry_ref<'a>(self: &'a Arc<Self>) -> SpirvEntryRef<'a> {
    if self.shader.is_some() {
      SpirvEntryRef::Shader(SpirvGraphicsShaderRef {
        module: &*self,
      })
    } else {
      SpirvEntryRef::Kernel(SpirvComputeKernelRef {
        module: &*self,
      })
    }
  }

  pub fn graphics_ty(&self) -> vk::pipeline::shader::GraphicsShaderType {
    use vk::pipeline::shader::GraphicsShaderType;
    match self.exe_model {
      ExecutionModel::Vertex => GraphicsShaderType::Vertex,
      ExecutionModel::TessellationControl => GraphicsShaderType::TessellationControl,
      ExecutionModel::TessellationEval => GraphicsShaderType::TessellationEvaluation,
      ExecutionModel::Geometry => unimplemented!("TODO geometry primitive modes"),
      ExecutionModel::Fragment => GraphicsShaderType::Fragment,
      _ => panic!("not a graphics shader exe model"),
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub struct SpirvComputeKernelRef<'a> {
  module: &'a SpirvModule,
}
#[derive(Clone, Copy, Debug)]
pub struct SpirvGraphicsShaderRef<'a> {
  module: &'a SpirvModule,
}
#[derive(Clone, Copy, Debug)]
pub enum SpirvEntryRef<'a> {
  Kernel(SpirvComputeKernelRef<'a>),
  Shader(SpirvGraphicsShaderRef<'a>),
}
#[derive(Clone, Debug)]
pub struct SpirvComputeKernel {
  module: Arc<SpirvModule>,
}
#[derive(Clone, Debug)]
pub struct SpirvGraphicsShader {
  module: Arc<SpirvModule>,
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
    &self.module.spirv
  }
  fn name(&self) -> &CStr {
    &self.module.entry
  }
  fn layout(&self) -> &Self::PipelineLayout { &self.module.pipeline_desc }
}
unsafe impl<'a> vk::pipeline::shader::EntryPointAbstract for SpirvGraphicsShaderRef<'a> {
  type PipelineLayout = StaticPipelineLayoutDesc;
  type SpecializationConstants = ();

  fn module(&self) -> &vk::pipeline::shader::ShaderModule {
    &self.module.spirv
  }
  fn name(&self) -> &CStr {
    &self.module.entry
  }
  fn layout(&self) -> &Self::PipelineLayout { &self.module.pipeline_desc }
}
unsafe impl<'a> vk::pipeline::shader::GraphicsEntryPointAbstract for SpirvGraphicsShaderRef<'a> {
  type InputDefinition = StaticShaderInterfaceDef;
  type OutputDefinition = StaticShaderInterfaceDef;

  fn input(&self) -> &Self::InputDefinition { &self.module.shader.as_ref().unwrap().0 }
  fn output(&self) -> &Self::OutputDefinition { &self.module.shader.as_ref().unwrap().1 }
  fn ty(&self) -> vk::pipeline::shader::GraphicsShaderType {
    self.module.graphics_ty()
  }
}
unsafe impl vk::pipeline::shader::EntryPointAbstract for SpirvComputeKernel {
  type PipelineLayout = StaticPipelineLayoutDesc;
  type SpecializationConstants = ();

  fn module(&self) -> &vk::pipeline::shader::ShaderModule {
    &self.module.spirv
  }
  fn name(&self) -> &CStr {
    &self.module.entry
  }
  fn layout(&self) -> &Self::PipelineLayout { &self.module.pipeline_desc }
}
unsafe impl vk::pipeline::shader::EntryPointAbstract for SpirvGraphicsShader {
  type PipelineLayout = StaticPipelineLayoutDesc;
  type SpecializationConstants = ();

  fn module(&self) -> &vk::pipeline::shader::ShaderModule {
    &self.module.spirv
  }
  fn name(&self) -> &CStr {
    &self.module.entry
  }
  fn layout(&self) -> &Self::PipelineLayout { &self.module.pipeline_desc }
}
unsafe impl vk::pipeline::shader::GraphicsEntryPointAbstract for SpirvGraphicsShader {
  type InputDefinition = StaticShaderInterfaceDef;
  type OutputDefinition = StaticShaderInterfaceDef;

  fn input(&self) -> &Self::InputDefinition { &self.module.shader.as_ref().unwrap().0 }
  fn output(&self) -> &Self::OutputDefinition { &self.module.shader.as_ref().unwrap().1 }
  fn ty(&self) -> vk::pipeline::shader::GraphicsShaderType {
    self.module.graphics_ty()
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
  pub fn kernel_entry(self) -> Option<SpirvComputeKernel> {
    match self {
      SpirvEntry::Kernel(k) => Some(k),
      _ => None,
    }
  }
  pub fn shader_entry(self) -> Option<SpirvGraphicsShader> {
    match self {
      SpirvEntry::Shader(s) => Some(s),
      _ => None,
    }
  }

  pub fn as_ref(&self) -> SpirvEntryRef {
    match self {
      &SpirvEntry::Kernel(ref k) =>
        SpirvEntryRef::Kernel(SpirvComputeKernelRef {
          module: &*k.module,
        }),
      &SpirvEntry::Shader(ref s) =>
        SpirvEntryRef::Shader(SpirvGraphicsShaderRef {
          module: &*s.module,
        }),
    }
  }
}
