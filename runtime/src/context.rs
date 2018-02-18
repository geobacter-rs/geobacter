
use std::collections::{HashSet};
use std::sync::{Arc, Weak, RwLock};

use hsa_core::kernel_info::KernelId;
use hsa_rt;

use module::{ModuleData, ErasedModule};
use Accelerator;

pub struct ContextData {
  accelerators: Vec<Weak<Accelerator>>,
  modules: HashSet<KernelId, Weak<ErasedModule>>,
}

#[derive(Clone)]
pub struct Context(Arc<RwLock<ContextData>>);

