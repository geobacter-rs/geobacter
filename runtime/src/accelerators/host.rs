
use std::sync::atomic::{AtomicUsize, Ordering, };

use hsa_rt::{self, agent::Agent, agent::Isa, };

use {Accelerator, AcceleratorId, };

pub struct HostAccel {
  id: AtomicUsize,

  hsa_ctx: hsa_rt::ApiContext,
  hsa_agent: Agent,
  hsa_isa: Isa,

  target_triple: String,
  host_arch: String,
  host_cpu: String,
}
impl HostAccel {
  /// This isn't ideal, but for now, make the user provide the host cpu.
  pub fn new(agent: Agent, isa: Isa,
             host_arch: String, host_cpu: String) -> Self {
    HostAccel {
      id: AtomicUsize::new(AcceleratorId::default().index()),

      hsa_ctx: hsa_rt::ApiContext::new(),
      hsa_agent: agent,
      hsa_isa: isa,

      target_triple: format!("{}-unknown-linux-gnu", host_arch),
      host_arch,
      host_cpu,
    }
  }
}

impl Accelerator for HostAccel {
  fn id_opt(&self) -> Option<AcceleratorId> {
    let id = self.id.load(Ordering::Acquire);
    let id = AcceleratorId::new(id);
    id.check_invalid()
  }
  fn set_id(&self, id: AcceleratorId) {
    self.id.store(id.index(), Ordering::Release);
  }

  fn target_triple(&self) -> String {
    self.target_triple.clone()
  }
  fn target_arch(&self) -> String {
    self.host_arch.clone()
  }
  fn target_cpu(&self) -> String {
    self.host_cpu.clone()
  }

  fn allow_indirect_function_calls(&self) -> bool { true }
}
