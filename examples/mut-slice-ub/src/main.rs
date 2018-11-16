#![feature(allocator_api)]

extern crate hsa_rt;
extern crate runtime;
#[macro_use]
extern crate log;
extern crate env_logger;
extern crate legionella_std;

use std::alloc::{Global, Alloc, Layout, };
use std::mem::{size_of, };

use hsa_rt::agent::{DeviceType, };
use hsa_rt::queue::{FenceScope, };

use runtime::context::{Context, };
use runtime::module::{Invoc, };

use legionella_std::{dispatch_packet, mem::*, mem::slice::*, };

const WORKITEM_SIZE: usize = 8;
const X_SIZE: usize = 1000;

/// This is our kernel.
fn obviously_undefined_behaviour(mut out: SliceMut<u64>) {
  let dispatch = dispatch_packet();
  out[0] = dispatch.global_id_x() as u64;
}

pub fn main() {
  env_logger::init();
  let ctxt = Context::new()
    .expect("create context");

  let accel = ctxt
    .find_accel(|accel| {
      match accel.agent().device_type() {
        Ok(DeviceType::Gpu) => true,
        _ => false,
      }
    })
    .expect("lock failure")
    .expect("no suitable accelerators");

  let workitems = accel.isa()
    .map(|isa| {
      isa.workgroup_max_size()
    })
    .unwrap_or(Ok(1))
    .expect("get max workgroup size") as usize;
  assert!(workitems >= WORKITEM_SIZE,
          "not enough workitems per workgroup for this example");
  info!("using workgroup size of {}", WORKITEM_SIZE);

  let mut invoc = Invoc::new(ctxt.clone(), obviously_undefined_behaviour)
    .expect("Invoc::new");

  invoc.workgroup_dims((WORKITEM_SIZE, ))
    .expect("Invoc::workgroup_dims");
  invoc.grid_dims((X_SIZE, ))
    .expect("Invoc::grid_dims");
  invoc.begin_fence = Some(FenceScope::System);
  invoc.end_fence = Some(FenceScope::System);

  let kernel_props = invoc.precompile(&accel)
    .expect("kernel compilation");

  let agent = accel.agent();

  let data_bytes = X_SIZE * size_of::<u64>();

  let group_size = Some(kernel_props.group_segment_size());
  let private_size = Some(kernel_props.private_segment_size());

  let host_data = unsafe {
    // allocate the host frame data w/ page alignment. This isn't
    // *required*, but I'm betting nicer for the driver.
    // XXX hardcoded page size.
    let layout =
      Layout::from_size_align(data_bytes, 4096)
        .unwrap();
    let data = Global.alloc(layout)
      .expect("allocate kernel data");
    Vec::from_raw_parts(data.as_ptr() as *mut u64,
                        X_SIZE, X_SIZE)
  };
  let mut data = host_data.lock_memory(&[agent])
    .expect("lock host memory to GPU");

  let queue = agent.new_kernel_multi_queue(4, group_size, private_size)
    .expect("Agent::new_kernel_multi_queue");

  invoc.call_accel_sync(&accel, &queue, (data.as_slice_mut(), ))
    .expect("Invoc::call_accel_sync");

  println!("the winning global id is: {}", data.as_slice()[0]);
}
