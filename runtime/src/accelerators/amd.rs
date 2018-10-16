
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::str::FromStr;
use std::sync::{RwLock, Weak, Arc, };

use rustc_target::spec::{PanicStrategy, AddrSpaceKind,
                         AddrSpaceIdx, AddrSpaceProps, };

use {Accelerator, AcceleratorId, AcceleratorTargetDesc, DeviceLibsBuilder, };
use accelerators::DeviceLibsBuild;
use codegen::worker::{CodegenComms, CodegenUnsafeSyncComms};
use utils::{run_cmd, CreateIfNotExists, };

use hsa_rt::agent::{Agent, Isa, DeviceType};

pub struct AmdGpuAccel {
  id: AcceleratorId,

  host: Weak<Accelerator>,

  hsa_agent: Agent,
  hsa_isa:   Isa,

  codegen: RwLock<Option<CodegenUnsafeSyncComms>>,
}
impl AmdGpuAccel {
  pub fn new(id: AcceleratorId,
             host: Weak<Accelerator>,
             agent: Agent)
    -> Result<Self, Box<Error>>
  {
    if agent.device_type()? != DeviceType::Gpu {
      return Err(format!("accelerator isn't a GPU").into());
    }
    if agent.vendor_name()? != "AMD" {
      return Err(format!("accelerator isn't an AMD").into());
    }
    let isas = agent.isas()?;
    if isas.len() == 0 {
      return Err("agent has no isas".into());
    }
    let isa = isas[0].clone();

    Ok(AmdGpuAccel {
      id,

      host,

      hsa_agent: agent,
      hsa_isa: isa,

      codegen: Default::default(),
    })
  }
}
impl Accelerator for AmdGpuAccel {
  fn id(&self) -> AcceleratorId { self.id }

  fn host_accel(&self) -> Option<Arc<Accelerator>> {
    self.host.upgrade()
  }

  fn agent(&self) -> &Agent {
    &self.hsa_agent
  }

  fn accel_target_desc(&self) -> Result<AcceleratorTargetDesc, Box<Error>> {
    let mut desc = AcceleratorTargetDesc::default();
    desc.allow_indirect_function_calls = false;
    {
      let mut target = &mut desc.target;

      // we get the triple and gpu "cpu" from the name of the isa:
      let triple = self.hsa_isa.name()?;
      const PREFIX: &'static str = "amdgcn-amd-amdhsa--";
      if triple.starts_with(PREFIX) {
        target.llvm_target = "amdgcn-amd-amdhsa-amdgiz".into();
        target.options.cpu = triple[PREFIX.len()..].into();
      } else {
        let idx = triple.rfind('-')
          .expect("expected at least one hyphen in the AMDGPU ISA name");
        assert_ne!(idx, triple.len(),
                   "AMDGPU ISA target triple has no cpu model, or something else weird");
        target.llvm_target = triple[..idx + 1].into();
        target.options.cpu = triple[idx + 1..].into();
      }

      target.options.features = "+dpp,+s-memrealtime".into();
      if self.hsa_isa.fast_f16()? {
        target.options.features.push_str(",+16-bit-insts");
      }

      target.target_endian = "little".into();
      target.target_pointer_width = "64".into();
      target.arch = "amdgpu".into();
      target.data_layout = "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-\
                          p4:64:64-p5:32:32-p6:32:32-i64:64-v16:16-\
                          v24:32-v32:32-v48:64-v96:128-v192:256-\
                          v256:256-v512:512-v1024:1024-v2048:2048-\
                          n32:64-S32-A5".into();
      target.options.codegen_backend = "llvm".into();
      target.options.panic_strategy = PanicStrategy::Abort;
      target.options.trap_unreachable = true;
      target.options.position_independent_executables = true;
      target.options.dynamic_linking = true;
      target.options.executables = true;
      target.options.requires_lto = false;
      target.options.atomic_cas = true;
      target.options.default_codegen_units = Some(1);
      target.options.i128_lowering = true;
      //target.options.obj_is_bitcode = true;
      {
        let addr_spaces = &mut target.options.addr_spaces;
        addr_spaces.clear();

        let flat = AddrSpaceKind::Flat;
        let flat_idx = AddrSpaceIdx(0);

        let global = AddrSpaceKind::ReadWrite;
        let global_idx = AddrSpaceIdx(1);

        let region = AddrSpaceKind::from_str("region").unwrap();
        let region_idx = AddrSpaceIdx(2);

        let local = AddrSpaceKind::from_str("local").unwrap();
        let local_idx = AddrSpaceIdx(3);

        let constant = AddrSpaceKind::ReadOnly;
        let constant_idx = AddrSpaceIdx(4);

        let private = AddrSpaceKind::Alloca;
        let private_idx = AddrSpaceIdx(5);

        let constant_32b = AddrSpaceKind::from_str("32bit constant").unwrap();
        let constant_32b_idx = AddrSpaceIdx(6);

        let props = AddrSpaceProps {
          index: flat_idx,
          shared_with: vec![private.clone(),
                            region.clone(),
                            local.clone(),
                            constant.clone(),
                            global.clone(),
                            constant_32b.clone(), ]
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
        insert_as(addr_spaces, global.clone(), global_idx);
        insert_as(addr_spaces, region.clone(), region_idx);
        insert_as(addr_spaces, local.clone(), local_idx);
        //insert_as(addr_spaces, constant.clone(), constant_idx);
        insert_as(addr_spaces, private.clone(), private_idx);
        insert_as(addr_spaces, constant_32b.clone(), constant_32b_idx);
      }
    }

    Ok(desc)
  }

  fn device_libs_builder(&self) -> Option<Box<dyn DeviceLibsBuilder>> {
    Some(Box::new(AmdGpuDeviceLibsBuilder))
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

const DEVICE_LIBS_URL: &'static str = "https://github.com/RadeonOpenCompute/ROCm-Device-Libs.git";
const DEVICE_LIBS_BRANCH: &'static str = "master";

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct AmdGpuDeviceLibsBuilder;
impl DeviceLibsBuilder for AmdGpuDeviceLibsBuilder {
  fn run_build(&self, device_libs: &mut DeviceLibsBuild) -> Result<(), Box<Error>> {
    use std::process::Command;
    use utils::git::checkout_repo;

    let device_libs_src = device_libs.build_path()
      .join("ROCm-Device-Libs");
    let device_libs_build = device_libs.build_path()
      .join("ROCm-Device-Libs-build");

    info!("rocm device libs src dir  : {}", device_libs_src.display());
    info!("rocm device libs build dir: {}", device_libs_build.display());

    checkout_repo(&device_libs_src, DEVICE_LIBS_URL,
                  DEVICE_LIBS_BRANCH, true)?;

    device_libs_build.create_if_not_exists()?;

    let rust_build_root = super::RustBuildRoot::default();
    let llvm_root = rust_build_root.llvm_root();

    let clang = llvm_root.join("bin/clang");
    let clangxx = llvm_root.join("bin/clang++");

    let ar = llvm_root.join("bin/llvm-ar");
    let ranlib = llvm_root.join("bin/llvm-ranlib");

    let cc = format!("-DCMAKE_C_COMPILER={}", clang.display());
    let cxx = format!("-DCMAKE_CXX_COMPILER={}", clangxx.display());

    let ar = format!("-DCMAKE_AR={}", ar.display());
    let ranlib = format!("-DCMAKE_RANLIB={}", ranlib.display());

    let target_flags = {
      let target = &device_libs.target_desc().target.llvm_target;
      let arch = &device_libs.target_desc().target.options.cpu;
      format!("-target {} -march {}", target, arch)
    };
    let c_flags = format!("-DCMAKE_C_FLAGS={}", target_flags);
    let cxx_flags = format!("-DCMAKE_CXX_FLAGS={}", target_flags);

    let llvm_dir = format!("-DLLVM_DIR={}", llvm_root.display());

    let mut cmake = Command::new("cmake");
    cmake.current_dir(&device_libs_build)
      .arg(&device_libs_src)
      .arg("-G")
      .arg("Unix Makefiles")
      .arg("-DCMAKE_BUILD_TYPE=Release")
      .arg(cc).arg(c_flags)
      .arg(cxx).arg(cxx_flags)
      .arg(ar)
      .arg(ranlib)
      .arg(llvm_dir)
      .arg("-DROCM_DEVICELIB_INCLUDE_TESTS:BOOL=Off");

    run_cmd(cmake)?;

    let mut make = Command::new("make");
    make.current_dir(&device_libs_build)
      .arg("-j")
      .arg(format!("{}", ::num_cpus::get()));

    run_cmd(make)?;

    unimplemented!();

    //Ok(())
  }
}

impl fmt::Debug for AmdGpuAccel {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "AmdGpuAccel {{ id: {:?}, agent: {:?}, isa: {:?}, }}",
           self.id, self.hsa_agent, self.hsa_isa)
  }
}
