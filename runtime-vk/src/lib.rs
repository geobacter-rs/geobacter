
//! Crate for Vulkan accelerators.

extern crate runtime_core as rt;
extern crate vulkano as vk;

use rt::{AcceleratorId, Accelerator, };

pub struct VkAccel {
  id: AcceleratorId,

  target_desc: Arc<rt::AcceleratorTargetDesc>,

  dev: Arc<vk::Device>,
}

impl VkAccel {
  pub fn new(id: AcceleratorId, dev: Arc<vk::Device>) -> Result<Self, Box<Error>> {
    let mut out = VkAccel {
      id,
      dev,

      target_desc: Arc::default(),
    };
    out.init_target_desc()?;

    Ok(out)
  }
  pub fn device(&self) -> &Arc<vk::Device> {
    &self.dev
  }
  pub fn instance(&self) -> &Arc<vk::Instance> {
    self.dev.instance()
  }
}
impl Accelerator for VkAccel {
  fn id(&self) -> AcceleratorId { self.id }

  fn accel_target_desc(&self) -> &Arc<AcceleratorTargetDesc> {
    self.target_desc
  }

  fn set_codegen(&self, comms: CodegenComms) -> Option<CodegenComms> {
    let mut lock = self.codegen.write().unwrap();
    let ret = lock.take();
    *lock = Some(unsafe { comms.sync_comms() });

    ret.map(|v| v.clone_into() )
  }
  fn get_codegen(&self) -> Option<CodegenComms> {
    let lock = self.codegen.read().unwrap();
    (*&lock).as_ref().map(|v| v.clone_into() )
  }
}

// private methods
impl VkAccel {
  fn init_target_desc(&mut self) -> Result<(), Box<Error>> {
    let desc = Arc::make_mut(&mut self.target_desc);

    desc.allow_indirect_function_calls = false;
    let target = &mut desc.target;

    target.llvm_target = "spir64-unknown-unknown";

    target.target_endian = "little".into();
    target.target_pointer_width = "64".into();
    target.arch = "spir64".into();
    target.data_layout = "e-i64:64-v16:16-v24:32-v32:32-v48:64-\
                          v96:128-v192:256-v256:256-v512:512-\
                          v1024:1024-n8:16:32:64".into();
    target.options.codegen_backend = "llvm".into();
    target.options.panic_strategy = PanicStrategy::Abort;
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
    {
      let addr_spaces = &mut target.options.addr_spaces;
      addr_spaces.clear();

      let private = AddrSpaceKind::Alloca;
      let private_idx = AddrSpaceIdx(0);

      let global = AddrSpaceKind::ReadWrite;
      let global_idx = AddrSpaceIdx(1);

      let region = AddrSpaceKind::from_str("region").unwrap();
      let region_idx = AddrSpaceIdx(2);

      let local = AddrSpaceKind::from_str("local").unwrap();
      let local_idx = AddrSpaceIdx(3);

      let constant = AddrSpaceKind::ReadOnly;
      let constant_idx = AddrSpaceIdx(4);

      let flat = AddrSpaceKind::Flat;
      let flat_idx = AddrSpaceIdx(5);

      let props = AddrSpaceProps {
        index: flat_idx,
        shared_with: vec![private.clone(),
                          region.clone(),
                          local.clone(),
                          constant.clone(),
                          global.clone(), ]
          .into_iter()
          .collect(),
      };
      addr_spaces.insert(flat.clone(), props);

      let insert_as = |addr_spaces: &mut BTreeMap<_, _>,
                       kind,
                       idx| {
        let props = AddrSpaceProps {
          index: idx,
          shared_with: vec![flat.clone()]
            .into_iter()
            .collect(),
        };
        addr_spaces.insert(kind, props);
      };
      insert_as(addr_spaces, global.clone(), global_idx);
      insert_as(addr_spaces, region.clone(), region_idx);
      insert_as(addr_spaces, local.clone(), local_idx);
      insert_as(addr_spaces, constant.clone(), constant_idx);
      insert_as(addr_spaces, private.clone(), private_idx);
    }

    Ok(())
  }
}
