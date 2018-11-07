#![feature(rustc_private, duration_as_u128)]
#![feature(intrinsics)]
#![feature(custom_attribute)]
#![feature(core_intrinsics)]

extern crate ndarray as nd;
extern crate hsa_core;
extern crate hsa_rt;
extern crate hsa_rt_sys;
extern crate runtime;
#[macro_use]
extern crate log;
extern crate env_logger;
extern crate rand;
extern crate legionella_std;
extern crate rustc_driver;

use std::mem::{size_of, };
use std::time::Instant;

use rand::rngs::SmallRng;
use rand::{Rng, FromEntropy, };

use hsa_rt::{ext::HostLockedAgentMemory, };
use hsa_rt::agent::{DeviceType, };

use runtime::context::{Context, };
use runtime::module::Invoc;

use legionella_std::{workitem::WorkItemAxis,
                     workitem::WorkGroupAxis,
                     workitem::AxisDimX, };

pub type Elem = f32;
const COUNT: usize = 1024 * 1024;
const ITERATIONS: usize = 16;

pub fn vector_foreach(args: Args) {
  let Args {
    mut into,
    value,
  } = args;

  let wg = AxisDimX.workgroup_id();
  let wi = AxisDimX.workitem_id();

  let idx = (wg, wi);

  if let Some(dest) = into.get_mut(idx) {
    for _ in 0..ITERATIONS {
      *dest += 1.0;
      *dest *= value;
    }
  }
}

#[repr(packed)]
pub struct Args<'a> {
  into: nd::ArrayViewMut2<'a, Elem>,
  value: Elem,
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
    .unwrap_or_else(|| {
      error!("error finding suitable GPU, falling back to CPU");
      ctxt.primary_host_accel().unwrap()
    });

  let workitems = accel.isa()
    .map(|isa| {
      isa.workgroup_max_size()
    })
    .unwrap_or(Ok(1))
    .expect("get max workgroup size");
  let workitems = workitems as usize;
  info!("using workgroup size of {}", workitems);

  let mut invoc = Invoc::new(ctxt.clone(), vector_foreach)
    .expect("Invoc::new");

  info!("allocating {} MB of host memory", COUNT * size_of::<Elem>() / 1024 / 1024);

  let mut values = nd::Array::zeros((COUNT, ));

  let mut rng = SmallRng::from_entropy();
  for value in values.iter_mut() {
    *value = rng.gen();
  }
  let values = values
    .into_shape((COUNT / workitems, workitems, ))
    .expect("reshape");
  let original_values = values.clone();

  let start = Instant::now();
  let mut values = values.lock_memory_globally()
    .expect("lock host memory for agent");
  let elapsed = start.elapsed();
  info!("mem lock took {}ms", elapsed.as_millis());

  const VALUE: Elem = 4.0;
  let args = Args {
    into: values.agent_view_mut(),
    value: VALUE,
  };

  invoc.workgroup_dims((workitems, ))
    .expect("Invoc::workgroup_dims");
  invoc.grid_dims((COUNT, ))
    .expect("Invoc::grid_dims");

  info!("dispatching...");
  let start = Instant::now();
  invoc.call_accel_sync(&accel, (args, ))
    .expect("Invoc::call_accel_sync");
  let elapsed = start.elapsed();
  info!("dispatch took {}ms", elapsed.as_millis());

  // check results:
  let values = values.unlock();
  for (idx, &value) in values.indexed_iter() {
    let &original_value = original_values.get(idx).unwrap();
    let mut expected_value = original_value;
    for _ in 0..ITERATIONS {
      expected_value += 1.0;
      expected_value *= VALUE;
    }
    assert_eq!(expected_value, value, "mismatch at index {:?}", idx);
  }
}
