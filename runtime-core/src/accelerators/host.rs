
use std::error::Error;
use std::fmt;
use std::ops::{Range, };
use std::sync::{RwLock, Arc, };

use hsa_rt::{agent::Agent, agent::DeviceType, };
use hsa_rt::mem::region::{Region, };
use hsa_rt::queue::MultiQueue;

use {Accelerator, AcceleratorId, AcceleratorTargetDesc, };
use codegen::worker::{CodegenComms, CodegenUnsafeSyncComms, };

pub struct HostAccel {
  id: AcceleratorId,

  hsa_agent: Agent,
  kernarg_region: Region,

  codegen: RwLock<Option<CodegenUnsafeSyncComms>>,
  queues: RwLock<Arc<Vec<Arc<MultiQueue>>>>,

  target_desc: Arc<AcceleratorTargetDesc>,
}
impl HostAccel {
  pub fn new(id: AcceleratorId, agent: Agent) -> Result<Self, Box<Error>> {
    if agent.device_type()? != DeviceType::Cpu {
      return Err(format!("host accelerator isn't a CPU").into());
    }

    let kernarg_region = agent.all_regions()?
      .into_iter()
      .filter(|region| {
        region.runtime_alloc_allowed()
          .unwrap_or(false)
      })
      .find(|region| {
        match region.global_flags() {
          Ok(Some(flags)) => flags.kernel_arg() && flags.fine_grained(),
          _ => false,
        }
      })
      .ok_or_else(|| "no kernel argument region" )?;

    let mut desc = AcceleratorTargetDesc::default();
    desc.target.options.cpu = "native".into();
    desc.target.options.obj_is_bitcode = true;
    desc.target.options.default_codegen_units = Some(1);

    Ok(HostAccel {
      id,

      hsa_agent: agent,
      kernarg_region,

      codegen: Default::default(),
      queues: RwLock::new(Arc::new(vec![])),

      target_desc: Arc::new(desc),
    })
  }
}

impl Accelerator for HostAccel {
  fn id(&self) -> AcceleratorId {
    self.id
  }

  fn agent(&self) -> &Agent { &self.hsa_agent }
  fn kernargs_region(&self) -> &Region { &self.kernarg_region }

  fn accel_target_desc(&self) -> Result<Arc<AcceleratorTargetDesc>, Box<Error>> {
    Ok(self.target_desc.clone())
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
