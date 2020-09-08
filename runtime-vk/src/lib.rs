//! Crate for Vulkan accelerators.

#![feature(rustc_private)]
#![feature(intrinsics)]
#![feature(geobacter)]

extern crate rustc_ast;
extern crate rustc_data_structures;
extern crate rustc_geobacter;
extern crate rustc_hir;
extern crate rustc_index;
extern crate rustc_middle;
extern crate rustc_span;
extern crate rustc_target;

use std::any::Any;
use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::ffi::CString;
use std::fmt;
use std::geobacter::kernel::OptionalKernelFn;
use std::geobacter::platform::Platform;
use std::num::NonZeroU32;
use std::sync::Arc;

use grt_core::{AcceleratorId, Accelerator, AcceleratorTargetDesc, PlatformTargetDesc, Device};
use grt_core::codegen::{CodegenDriver, KernelDesc};
use grt_core::codegen::products::PCodegenResults;
use grt_core::context::*;

use rustc_target::spec::{*, abi::Abi};

use serde::*;

use crate::codegen::*;
use crate::module::*;

pub use crate::error::Error;

pub mod codegen;
pub mod error;
pub mod module;

mod serde_utils;

pub struct VkAccel {
  id: AcceleratorId,
  ctx: Context,
  dev: Arc<vk::device::Device>,

  target_desc: Arc<AcceleratorTargetDesc>,

  self_codegen: Option<Arc<CodegenDriver<VkPlatformCodegen>>>,
}

impl VkAccel {
  fn new_raw(id: AcceleratorId, ctx: &Context, dev: Arc<vk::device::Device>)
    -> Result<Arc<Self>, Error>
  {
    let features = dev.enabled_features();
    if !features.variable_pointers {
      return Err(Error::MissingRequiredFeature);
    }

    let target_desc = AcceleratorTargetDesc::new(VkTargetDesc);
    let mut out = VkAccel {
      id,
      ctx: ctx.clone(),
      dev,
      target_desc: Arc::new(target_desc),
      self_codegen: None,
    };
    out.init_target_desc()?;

    let mut out = Arc::new(out);

    ctx.initialize_accel(&mut out)?;

    Ok(out)
  }
  #[inline(always)]
  pub fn new(ctx: &Context, dev: Arc<vk::device::Device>) -> Result<Arc<Self>, Error> {
    let id = ctx.take_accel_id();
    Self::new_raw(id, ctx, dev)
  }
  #[inline(always)]
  pub fn instance(&self) -> &Arc<vk::instance::Instance> {
    self.dev.instance()
  }
  #[inline(always)]
  pub fn device(&self) -> &Arc<vk::device::Device> {
    &self.dev
  }
}

impl Accelerator for VkAccel {
  #[inline(always)]
  fn id(&self) -> AcceleratorId { self.id.clone() }

  /// Since the Platform enum contains the specific shader/kernel execution model,
  /// we can't return a specific platform here.
  #[inline(always)]
  fn platform(&self) -> Option<Platform> { None }

  #[inline(always)]
  fn accel_target_desc(&self) -> &Arc<AcceleratorTargetDesc> {
    &self.target_desc
  }

  fn set_accel_target_desc(&mut self, desc: Arc<AcceleratorTargetDesc>) {
    self.target_desc = desc;
  }

  fn create_target_codegen(self: &mut Arc<Self>, ctxt: &Context)
    -> Result<Arc<dyn Any + Send + Sync + 'static>, Box<dyn StdError + Send + Sync + 'static>>
    where Self: Sized,
  {
    let cg = CodegenDriver::new(ctxt,
                                self.accel_target_desc().clone(),
                                Default::default())?;
    let cg_sync = Arc::new(cg);
    Arc::get_mut(self)
      .expect("there should only be a single ref at this point")
      .self_codegen = Some(cg_sync.clone());

    cg_sync.add_accel(self);

    Ok(cg_sync)
  }

  fn set_target_codegen(self: &mut Arc<Self>,
                        codegen_comms: Arc<dyn Any + Send + Sync + 'static>)
    where Self: Sized,
  {
    let cg = codegen_comms
      .downcast()
      .expect("unexpected codegen type?");

    Arc::get_mut(self)
      .expect("there should only be a single ref at this point")
      .self_codegen = Some(cg);

    self.codegen().add_accel(self);
  }
}

impl Device for VkAccel {
  type Error = Error;
  type Codegen = codegen::VkPlatformCodegen;
  type TargetDesc = VkTargetDesc;
  type ModuleData = module::SpirvModule;

  fn codegen(&self) -> &Arc<CodegenDriver<Self::Codegen>> {
    self.self_codegen
      .as_ref()
      .expect("we are uninitialized?")
  }

  fn load_kernel(self: &Arc<Self>, codegen: &PCodegenResults<Self::Codegen>)
    -> Result<Arc<Self::ModuleData>, Error>
  {
    let exe_bin = codegen.exe_ref().unwrap();

    let spirv = unsafe {
      vk::pipeline::shader::ShaderModule::new(self.device().clone(),
                                              exe_bin)?
    };
    // vulkano puts the ShaderModule in an Arc. Extract it here:
    let spirv = Arc::try_unwrap(spirv).ok()
      .expect("spirv module is already shared? Impossible!");

    assert_eq!(codegen.entries.len(), 1);
    let entry = &codegen.entries[0];

    let mut kernel = SpirvModule {
      entries: Default::default(),
      pipeline_desc: codegen.root()
        .platform
        .pipeline,
    };
    kernel.entries.push(Arc::new(module::SpirvEntryPoint {
      name: CString::new(entry.symbol.clone())
        .expect("unexpected null char"),
      spirv,
      exe_model: entry.platform.exe_model,
      shader: entry.platform.interface,
    }));

    Ok(Arc::new(kernel))
  }
}
impl fmt::Debug for VkAccel {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.debug_struct("VkAccel")
      .field("id", &self.id)
      .field("device", &self.dev)
      .field("target_desc", &self.target_desc)
      .finish()
  }
}

// private methods
impl VkAccel {
  pub fn compile_compute_testing<F>(self: &Arc<Self>, k: F,
                                    workgroup_size: Option<(NonZeroU32, NonZeroU32, NonZeroU32)>)
    -> Result<Arc<module::SpirvModule>, Error>
    where F: Fn() + Sized,
  {
    use std::geobacter::platform::*;

    let pipeline = StaticPipelineLayoutDesc::layout_for::<F>();
    let instance = k.kernel_instance();
    let context_data = ModuleContextData::get(&k)
      .get_cache_data(&self.ctx);
    let desc = VkEntryDesc {
      exe_model: spirv::ExeModel::GLCompute,
      workgroup_size,
      pipeline,
      interface: Default::default(),
    };

    let desc = KernelDesc {
      instance,
      spec_params: Default::default(),
      platform_desc: desc,
    };

    Ok(context_data.compile(self, desc, self.codegen(), cfg!(test))?)
  }

  /// XXX neither `vert` or `frag` can be used as a compute entry point, but
  /// nothing will error if you do this.
  pub fn compile_shader_testing<V, F>(self: &Arc<Self>, vert: V, frag: F)
    -> Result<Arc<module::SpirvModule>, Error>
    where V: Fn() + Sized,
          F: Fn() + Sized,
  {
    use std::geobacter::platform::*;

    let pipeline = StaticPipelineLayoutDesc::layout_for2::<V, F>();

    let instance = vert.kernel_instance();
    let context_data = ModuleContextData::get(&vert)
      .get_cache_data(&self.ctx);
    let desc = VkEntryDesc {
      exe_model: spirv::ExeModel::Vertex,
      workgroup_size: None,
      pipeline,
      interface: Default::default(),
    };

    let desc = KernelDesc {
      instance,
      spec_params: Default::default(),
      platform_desc: desc,
    };

    let vertex = context_data.compile(self, desc, self.codegen(), cfg!(test))?;

    let instance = frag.kernel_instance();
    let context_data = ModuleContextData::get(&frag)
      .get_cache_data(&self.ctx);
    let desc = VkEntryDesc {
      exe_model: spirv::ExeModel::Fragment,
      workgroup_size: None,
      pipeline,
      interface: Default::default(),
    };

    let desc = KernelDesc {
      instance,
      spec_params: Default::default(),
      platform_desc: desc,
    };

    let fragment = context_data.compile(self, desc, self.codegen(), cfg!(test))?;
    let mut module = SpirvModule {
      entries: Default::default(),
      pipeline_desc: pipeline,
    };
    module.entries.push(vertex.entries[0].clone());
    module.entries.push(fragment.entries[0].clone());

    Ok(Arc::new(module))
  }

  fn init_target_desc(&mut self) -> Result<(), Error> {
    use std::str::FromStr;

    let desc = Arc::get_mut(&mut self.target_desc).unwrap();

    desc.allow_indirect_function_calls = false;

    desc.kernel_abi = Abi::SpirKernel;
    desc.target.target_endian = desc.host_target
      .target_endian
      .clone();

    let target = &mut desc.target;
    target.llvm_target = "spirv64-vulkan-unknown".into();
    target.target_pointer_width = "64".into();
    target.arch = "spirv64".into();
    target.options.cpu = "".into();
    target.data_layout = "e-m:e-p:64:64".into();

    {
      let features = self.dev.enabled_features();
      let mut push = |s: &str| {
        if target.options.features.len() != 0 {
          target.options.features.push(',');
        }
        target.options.features
          .push_str(s);
      };
      push("+shader");

      if features.variable_pointers {
        push("+variable-pointers");
      } else if features.variable_pointers_storage_buffer {
        push("+variable-pointers-storage-buffer");
      }
      if features.shader_int8 {
        push("+i8");
      }
      if features.shader_int16 {
        push("+i16");
      }
      if features.shader_int64 {
        push("+i64");
      }
      if features.shader_float16 {
        push("+f16");
      }
      if features.shader_f3264 {
        push("+f64");
      }
      if features.shader_buffer_int64_atomics {
        push("+i64-atomics");
      }
      if features.storage_uniform_8bit {
        push("+storage-uniform-i8-access");
      } else if features.storage_buffer_8bit {
        push("+storage-buffer-i8-access");
      }
      if features.storage_uniform_16bit {
        push("+storage-uniform-i16-access");
      } else if features.storage_buffer_16bit {
        push("+storage-buffer-i16-access");
      }
      if features.storage_input_output_16bit {
        push("+storage-input-output-i16-access");
      }
      if features.buffer_device_address {
        push("+physical-storage-buffer-addresses");
      }
    }
    println!("{}", target.options.features);

    target.options.panic_strategy = PanicStrategy::Abort;
    target.options.trap_unreachable = false;
    target.options.position_independent_executables = true;
    target.options.dynamic_linking = false;
    target.options.executables = true;
    target.options.requires_lto = false;
    target.options.atomic_cas = true;
    target.options.default_codegen_units = Some(1);
    target.options.simd_types_indirect = false;
    target.options.is_builtin = false;

    {
      let addr_spaces = &mut target.options.addr_spaces;
      addr_spaces.clear();

      let flat = AddrSpaceKind::Flat;
      let flat_idx = AddrSpaceIdx(4);

      let global = AddrSpaceKind::ReadWrite;
      let global_idx = AddrSpaceIdx(4);

      let region = AddrSpaceKind::from_str("region").unwrap();
      let region_idx = AddrSpaceIdx(1);

      let local = AddrSpaceKind::from_str("local").unwrap();
      let local_idx = AddrSpaceIdx(3);

      let constant = AddrSpaceKind::ReadOnly;
      let constant_idx = AddrSpaceIdx(4);

      let private = AddrSpaceKind::Alloca;
      let private_idx = AddrSpaceIdx(0);

      let input = AddrSpaceKind::from_str("input").unwrap();
      let input_idx = AddrSpaceIdx(5);

      let output = AddrSpaceKind::from_str("output").unwrap();
      let output_idx = AddrSpaceIdx(6);

      let uniform = AddrSpaceKind::from_str("uniform").unwrap();
      let uniform_idx = AddrSpaceIdx(7);

      let buffer = AddrSpaceKind::from_str("buffer").unwrap();
      let buffer_idx = AddrSpaceIdx(8);

      let phys_buffer = AddrSpaceKind::from_str("phys-buffer").unwrap();
      let phys_buffer_idx = AddrSpaceIdx(9);

      let props = AddrSpaceProps {
        index: flat_idx,
        shared_with: vec![private.clone(),
                          region.clone(),
                          local.clone(),
                          constant.clone(),
                          global.clone(),
                          input.clone(),
                          output.clone(),
                          uniform.clone(),
                          buffer.clone(),
                          phys_buffer.clone(),
        ]
          .into_iter()
          .collect(),
      };
      addr_spaces.insert(flat.clone(), props);

      let insert_as = |addr_spaces: &mut BTreeMap<_, _>, kind,
                       idx| {
        let props = AddrSpaceProps {
          index: idx,
          shared_with: vec![flat.clone()]
            .into_iter()
            .collect(),
        };
        addr_spaces.insert(kind, props);
      };
      insert_as(addr_spaces, global, global_idx);
      insert_as(addr_spaces, region, region_idx);
      insert_as(addr_spaces, local, local_idx);
      insert_as(addr_spaces, constant, constant_idx);
      insert_as(addr_spaces, private, private_idx);
      insert_as(addr_spaces, input, input_idx);
      insert_as(addr_spaces, output, output_idx);
      insert_as(addr_spaces, uniform, uniform_idx);
      insert_as(addr_spaces, buffer, buffer_idx);
      insert_as(addr_spaces, phys_buffer, phys_buffer_idx);
    }

    Ok(())
  }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Hash)]
pub struct VkTargetDesc;

impl PlatformTargetDesc for VkTargetDesc {
  fn as_any_hash(&self) -> &dyn any_key::AnyHash {
    self
  }
}

