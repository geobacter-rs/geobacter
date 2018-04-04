
use {Accelerator, AcceleratorId};

use std::sync::atomic::{AtomicUsize, Ordering};
use indexvec::Idx;

#[derive(Debug)]
pub struct AmdRadeon {
  id: AtomicUsize,
}
impl AmdRadeon {
  pub fn new() -> Self {
    AmdRadeon {
      id: AtomicUsize::new(AcceleratorId::default().index()),
    }
  }
}
impl Accelerator for AmdRadeon {
  fn id_opt(&self) -> Option<AcceleratorId> {
    let id = self.id.load(Ordering::Acquire);
    let id = AcceleratorId::new(id);
    id.check_invalid()
  }
  fn set_id(&self, id: AcceleratorId) {
    self.id.store(id.index(), Ordering::Release);
  }

  fn llvm_target(&self) -> String {
    "amdgcn-amd-hcc-amdgiz".into()
  }
  fn target_arch(&self) -> String {
    "amdgpu".into()
  }
  fn target_cpu(&self) -> String {
    // XXX
    "gfx803".into()
  }
}
