
use {Accelerator, AcceleratorId};

use std::sync::atomic::{AtomicUsize, Ordering};
use indexvec::Idx;

use hsa_rt;
use hsa_rt::agent::{Agent, Isa};

use error::Error;

#[derive(Debug)]
pub struct AmdGpuAccel {
  id: AcceleratorId,
  gpu_name: String,

  hsa_ctx:   hsa_rt::ApiContext,
  hsa_agent: Agent,
  hsa_isa:   Isa,
}
impl AmdGpuAccel {
  pub fn new(id: AcceleratorId,
             agent: Agent, isa: Isa,
             gpu_name: String) -> Result<Self, Error> {
    AmdGpuAccel {
      id,

      gpu_name,
      hsa_ctx: hsa_rt::ApiContext::new(),
      hsa_agent: agent,
      hsa_isa: isa,
    }
  }
}
impl Accelerator for AmdGpuAccel {
  fn id(&self) -> AcceleratorId { self.id }

  fn target_triple(&self) -> String {
    "amdgcn-amd-amdhsa-amdgiz".into()
  }
  fn target_arch(&self) -> String {
    "amdgpu".into()
  }
  fn target_cpu(&self) -> String { self.gpu_name.clone() }
  fn target_datalayout(&self) -> String {
    "e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-\
     i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-\
     v512:512-v1024:1024-v2048:2048-n32:64-S32-A5".into()
  }

  fn allow_indirect_function_calls(&self) -> bool { false }
}
