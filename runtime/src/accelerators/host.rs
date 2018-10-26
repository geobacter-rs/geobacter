
use std::error::Error;
use std::fmt;
use std::sync::{RwLock, };

use hsa_rt::{agent::Agent, agent::DeviceType, };

use {Accelerator, AcceleratorId, AcceleratorTargetDesc, };
use codegen::worker::{CodegenComms, CodegenUnsafeSyncComms, };

pub struct HostAccel {
  id: AcceleratorId,

  hsa_agent: Agent,

  codegen: RwLock<Option<CodegenUnsafeSyncComms>>,
}
impl HostAccel {
  pub fn new(id: AcceleratorId, agent: Agent) -> Result<Self, Box<Error>> {
    if agent.device_type()? != DeviceType::Cpu {
      return Err(format!("host accelerator isn't a CPU").into());
    }

    Ok(HostAccel {
      id,

      hsa_agent: agent,

      codegen: Default::default(),
    })
  }
}

impl Accelerator for HostAccel {
  fn id(&self) -> AcceleratorId {
    self.id
  }

  fn agent(&self) -> &Agent { &self.hsa_agent }

  fn accel_target_desc(&self) -> Result<AcceleratorTargetDesc, Box<Error>> {
    let mut desc = AcceleratorTargetDesc::default();
    desc.target.options.cpu = "native".into();
    desc.target.options.obj_is_bitcode = true;
    desc.target.options.default_codegen_units = Some(1);
    Ok(desc)
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

impl fmt::Debug for HostAccel {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "HostAccel {{ id: {:?}, agent: {:?}, }}",
           self.id, self.hsa_agent)
  }
}
