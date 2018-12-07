
extern crate runtime_core as rt_core;

use std::os::raw::{c_int, };

use rt_core::{Accelerator, AcceleratorId, };

pub struct CudaAccel {

  device_id: c_int,
}