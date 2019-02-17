
use std::error::Error;
use std::ops::{Deref, };
use std::sync::{Arc, };

use lcore::*;
use lstd::kernel::{KernelDesc as StdKernelDesc, kernel_desc, };

use {Accelerator, };
use codegen::worker::CodegenDesc;
use context::{Context, ModuleContextData,
              SpirvComputeKernel, };

#[derive(Clone, Eq, PartialEq)]
pub struct KernelDesc {
  desc: StdKernelDesc,
  context: Context,
  context_data: ModuleContextData,
}

impl KernelDesc {
  pub fn new<F>(context: Context, f: F) -> Self
    where F: Fn<(), Output = ()>,
  {
    KernelDesc {
      desc: kernel_desc(&f),
      context,
      context_data: ModuleContextData::get(&f),
    }
  }

  pub fn compile(&self, accel: &Arc<Accelerator>)
    -> Result<SpirvComputeKernel, Box<Error>>
  {
    let cache_data = self.context_data
      .get_cache_data(&self.context);

    let desc = CodegenDesc {
      id: self.desc.id,
      exe_model: ExecutionModel::GLCompute,
      pipeline: self.desc.pipeline_desc,
      interface: None,
      capabilities: self.desc.capabilities,
      extensions: self.desc.extensions,
    };

    let entry = cache_data
      .compile(&self.context,
               accel, desc)?
      .entry()
      .kernel_entry()
      .unwrap();

    Ok(entry)
  }
}
impl Deref for KernelDesc {
  type Target = StdKernelDesc;
  fn deref(&self) -> &StdKernelDesc {
    &self.desc
  }
}
