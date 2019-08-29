
//! Crate for Vulkan accelerators.

#![feature(rustc_private)]

extern crate geobacter_core as gcore;
extern crate geobacter_std as gstd;
extern crate geobacter_runtime_core as grt_core;
extern crate rustc_target;

#[macro_use]
extern crate serde_derive;
extern crate vulkano as vk;

use std::error::Error;
use std::ffi::CString;
use std::sync::{Arc, RwLock, };

use grt_core::{AcceleratorId, Accelerator, AcceleratorTargetDesc,
               PlatformTargetDesc, };
use grt_core::codegen::{PlatformKernelDesc, CodegenDesc};
use grt_core::codegen::worker::{CodegenComms, CodegenUnsafeSyncComms, };
use grt_core::context::Context;

use rustc_target::spec::{PanicStrategy, abi::Abi, };
use crate::module::SpirvModule;

pub mod codegen;
pub mod module;

mod serde_utils;

#[derive(Debug)]
pub struct VkAccel {
  id: AcceleratorId,

  target_desc: Arc<AcceleratorTargetDesc>,

  host_codegen: CodegenUnsafeSyncComms,

  dev: Arc<vk::device::Device>,
  queue: Arc<vk::device::Queue>,

  self_codegen: RwLock<Option<CodegenUnsafeSyncComms>>,
}

impl VkAccel {
  pub fn new_raw(id: AcceleratorId,
                 host: CodegenComms,
                 dev: Arc<vk::device::Device>,
                 queue: Arc<vk::device::Queue>)
    -> Result<Self, Box<Error>>
  {
    use vk::device::RawDeviceExtensions;
    let mut plat = VkTargetDesc {
      features: dev.enabled_features().clone(),
      extensions: Default::default(),
    };
    let extensions = RawDeviceExtensions::from(dev.loaded_extensions());
    for extension in extensions.iter() {
      let extension = extension.to_str()?.to_string();
      plat.extensions.insert(extension);
    }

    let target_desc = AcceleratorTargetDesc::new(plat);

    let mut out = VkAccel {
      id,
      dev,
      queue,

      target_desc: Arc::new(),
      host_codegen: unsafe { host.sync_comms() },
      self_codegen: RwLock::new(None),
    };
    out.init_target_desc()?;

    Ok(out)
  }
  pub fn new(ctx: &Context,
             dev: Arc<vk::device::Device>,
             queue: Arc<vk::device::Queue>)
    -> Result<Self, Box<Error>>
  {
    let id = ctx.take_accel_id()?;
    let host = ctx.host_codegen()?;
    Self::new_raw(id, host, dev, queue)
  }
  pub fn instance(&self) -> &Arc<vk::instance::Instance> {
    self.dev.instance()
  }
  pub fn device(&self) -> &Arc<vk::device::Device> {
    &self.dev
  }
}
impl Accelerator for VkAccel {
  fn id(&self) -> AcceleratorId { self.id.clone() }


  fn platform(&self) -> Platform {
    unimplemented!()
  }

  fn accel_target_desc(&self) -> &Arc<AcceleratorTargetDesc> {
    &self.target_desc
  }

  fn set_accel_target_desc(&mut self, desc: Arc<AcceleratorTargetDesc>) {
    self.target_desc = desc;
  }

  fn create_target_codegen(self: &mut Arc<Self>, ctxt: &Context)
    -> Result<Arc<Any>, Box<Error>>
    where Self: Sized
  {
    unimplemented!()
  }

  fn set_target_codegen(&mut self, codegen_comms: &Arc<Any>) {
    unimplemented!()
  }

  fn load_kernel(&self, _this: &Arc<Accelerator>, codegen: &[u8],
                 roots: &[CodegenDesc<Box<PlatformKernelDesc>>]) -> Result<Arc<PlatformModuleData>, Box<Error>> {
    let shader = codegen.desc.interface;
    let pipeline_desc = codegen.desc.pipeline;
    let exe_bin = codegen.outputs.get(&OutputType::Exe).unwrap();

    //let dev = accel.device();

    let spirv = unsafe {
      vk::pipeline::shader::ShaderModule::new(dev.clone(),
                                              exe_bin)?
    };
    // vulkano puts the ShaderModule in an Arc. Extract it here:
    let spirv = Arc::try_unwrap(spirv)
      .ok()
      .expect("spirv module is already shared? Impossible!");

    println!("symbol: {}", codegen.symbol);

    let kernel = SpirvModule {
      entry: CString::new(codegen.symbol.clone())
        .expect("str -> CString"),
      exe_model: desc.exe_model,
      shader,
      spirv,
      pipeline_desc,
    };
  }
}

// private methods
impl VkAccel {
  fn init_target_desc(&mut self) -> Result<(), Box<Error>> {
    let desc = Arc::make_mut(&mut self.target_desc);

    desc.allow_indirect_function_calls = false;

    desc.kernel_abi = Abi::SpirKernel;
    let target = &mut desc.target;

    target.llvm_target = "spir64-unknown-unknown".into();

    target.target_endian = "little".into();
    target.target_pointer_width = "64".into();
    target.arch = "spir64".into();
    target.data_layout = "e-p:64:64:64-i1:8:8-i8:8:8-i16:16:16-i32:32:32-\
                          i64:64:64-f32:32:32-f64:64:64-v16:16:16-v24:32:32-\
                          v32:32:32-v48:64:64-v64:64:64-v96:128:128-\
                          v128:128:128-v192:256:256-v256:256:256-\
                          v512:512:512-v1024:1024:1024".into();

    target.options.codegen_backend = "llvm".into();
    target.options.panic_strategy = PanicStrategy::Abort;
    target.options.custom_unwind_resume = false;
    target.options.trap_unreachable = true;
    target.options.position_independent_executables = true;
    target.options.dynamic_linking = false;
    target.options.executables = true;
    target.options.requires_lto = false;
    target.options.atomic_cas = true;
    target.options.default_codegen_units = Some(1);
    target.options.i128_lowering = true;
    target.options.obj_is_bitcode = true;
    target.options.simd_types_indirect = false;
    target.options.is_builtin = false;
    // Note: this is fudged; LLVM doesn't optimize address space
    // casts.
    target.options.addr_spaces = Default::default();

    Ok(())
  }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct VkTargetDesc {
  #[serde(flatten, with = "serde_utils::VkFeatures")]
  pub features: vk::device::Features,
  pub extensions: BTreeSet<String>,
}
impl PlatformTargetDesc for VkTargetDesc {

}

