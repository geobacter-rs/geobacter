
use std::error::Error;
use std::sync::{Arc, RwLock, };

use rustc_target::spec::{AddrSpaceIdx, AddrSpaceKind, AddrSpaceProps,
                         PanicStrategy, abi::Abi, };

use vk;

use {Accelerator, AcceleratorId, AcceleratorTargetDesc, };
use codegen::worker::{CodegenComms, CodegenUnsafeSyncComms, };
use context::Context;

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
    let mut out = VkAccel {
      id,
      dev,
      queue,

      target_desc: Arc::default(),
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
}
impl Accelerator for VkAccel {
  fn id(&self) -> AcceleratorId { self.id.clone() }

  fn host_codegen(&self) -> CodegenComms {
    self.host_codegen.clone_unsync()
  }

  fn device(&self) -> &Arc<vk::device::Device> {
    &self.dev
  }

  fn accel_target_desc(&self) -> &Arc<AcceleratorTargetDesc> {
    &self.target_desc
  }

  fn set_codegen(&self, comms: CodegenComms) -> Option<CodegenComms> {
    let mut lock = self.self_codegen.write().unwrap();
    let ret = lock.take();
    *lock = Some(unsafe { comms.sync_comms() });

    ret.map(|v| v.clone_unsync() )
  }
  fn get_codegen(&self) -> Option<CodegenComms> {
    let lock = self.self_codegen.read().unwrap();
    (*&lock).as_ref().map(|v| v.clone_unsync() )
  }
}

// private methods
impl VkAccel {
  fn init_target_desc(&mut self) -> Result<(), Box<Error>> {
    use std::str::FromStr;
    use vk::device::RawDeviceExtensions;
    let desc = Arc::make_mut(&mut self.target_desc);

    desc.allow_indirect_function_calls = false;
    desc.features = self.dev.enabled_features().clone();
    let extensions = RawDeviceExtensions::from(self.dev.loaded_extensions());
    for extension in extensions.iter() {
      let extension = extension.to_str()?.to_string();
      desc.extensions.insert(extension);
    }
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
    target.options.addr_spaces = Default::default();

    Ok(())
  }
}
