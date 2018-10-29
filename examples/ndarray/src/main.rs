#![feature(rustc_private, duration_as_u128)]
#![feature(intrinsics)]
#![feature(custom_attribute)]

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

use std::mem::size_of;
use std::time::Instant;

use rand::rngs::SmallRng;
use rand::{Rng, FromEntropy, };

use hsa_rt::{ext::HostLockedAgentMemory, };
use hsa_rt::agent::{DeviceType, Profiles, DefaultFloatRoundingModes, };
use hsa_rt::code_object::CodeObjectReaderRef;
use hsa_rt::executable::{CommonExecutable, Executable, };
use hsa_rt::queue::{QueueType, DispatchPacket, FenceScope, };
use hsa_rt::signal::Signal;

use runtime::context::{Context, OutputType, };

use legionella_std::{dispatch_packet,
                     workitem::WorkItemAxis,
                     workitem::WorkGroupAxis,
                     workitem::AxisDimX};

pub type Elem = f32;
const COUNT: usize = 4 * WORKGROUP_SIZE * 1024 * 1024;
const ITERATIONS: usize = 16;
const WORKGROUP_X_SIZE: usize = 16;
const WORKGROUP_Y_SIZE: usize = 1;
const WORKGROUP_SIZE: usize = WORKGROUP_X_SIZE * WORKGROUP_Y_SIZE;

pub fn vector_foreach(mut into: nd::ArrayViewMut3<Elem>,
                      value: Elem) {
  let mut into = into
    .subview_mut(nd::Axis(0), AxisDimX.workgroup_id());
  let mut into = into
    .subview_mut(nd::Axis(0), AxisDimX.workitem_id());
  for idx in 0..WORKGROUP_Y_SIZE {
    if let Some(dest) = into.get_mut(idx) {
      for _ in 0..ITERATIONS {
        *dest += 1.0;
        *dest *= value;
      }
    }
  }
}

pub fn main() {
  env_logger::init();
  rustc_driver::env_logger::init();
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

  let id = hsa_core::kernel::kernel_id_for(&vector_foreach);
  let codegen = ctxt.manually_compile(&accel, id)
    .expect("codegen failed");

  let exe_bin = codegen.outputs.get(&OutputType::Exe)
    .expect("link failed");
  let exe_reader = CodeObjectReaderRef::new(exe_bin.as_ref())
    .expect("CodeObjectReaderRef::new");

  let agent = accel.agent();

  let profiles = Profiles::base();
  let rounding_mode = DefaultFloatRoundingModes::near();
  let exe = Executable::new(profiles, rounding_mode,
                            "")
    .expect("Executable::new");

  exe.load_agent_code_object(agent, &exe_reader, "")
    .expect("Executable::load_agent_code_object");
  let exe = exe.freeze("")
    .expect("Executable::freeze");

  let regions = agent.all_regions()
    .expect("Agent::all_regions");
  let kernel_arg_region = regions
    .iter()
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
    .expect("no kernel argument region");

  info!("allocating {} MB of host memory", COUNT * size_of::<Elem>() / 1024 / 1024);

  let mut values =
    nd::Array::zeros((COUNT, ));

  let mut rng = SmallRng::from_entropy();
  for value in values.iter_mut() {
    *value = rng.gen();
  }
  let values = values
    .into_shape((COUNT / WORKGROUP_SIZE,
                 WORKGROUP_X_SIZE,
                 WORKGROUP_Y_SIZE, ))
    .expect("reshape");
  let original_values = values.clone();

  let start = Instant::now();
  let mut values = values.lock_memory(Some(agent).into_iter())
    .expect("lock host memory for agent");
  let elapsed = start.elapsed();
  info!("mem lock took {}ms", elapsed.as_millis());

  let symbols = exe.agent_symbols(agent)
    .expect("agent symbols");
  let kernel_symbol = symbols.into_iter()
    .find(|symbol| {
      match symbol.name() {
        Ok(ref n) if n == &codegen.kernel_symbol => true,
        Ok(n) => {
          info!("ignoring symbol {}", n);
          false
        },
        Err(_) => {
          warn!("unnamed symbol; skipping");
          false
        },
      }
    })
    .expect("missing kernel symbol");

  let kernel_object = kernel_symbol.kernel_object()
    .expect("Symbol::kernel_object")
    .expect("missing kernel object");

  let kernel_queue = agent
    .new_kernel_queue(8, QueueType::Single, None, None)
    .expect("Agent::new_queue");

  let completion_signal = Signal::new(1, &[])
    .expect("create completion signal");

  struct Args<'a> {
    into: nd::ArrayViewMut3<'a, Elem>,
    value: Elem,
  }

  const VALUE: Elem = 4.0;

  {
    let args: &mut Args = unsafe {
      let args = kernel_arg_region.allocate::<Args>(1)
        .expect("allocate kernel arg");
      ::std::mem::transmute(args)
    };
    let agent_values: nd::ArrayViewMut3<Elem> = values.agent_view_mut();
    args.into = agent_values;
    args.value = VALUE;

    completion_signal.store_screlease(1);

    let dispatch = DispatchPacket {
      workgroup_size: (WORKGROUP_X_SIZE, ),
      grid_size: (COUNT / WORKGROUP_Y_SIZE, ),
      group_segment_size: 4112,
      private_segment_size: (WORKGROUP_X_SIZE * 112) as u32,
      scaquire_scope: Some(FenceScope::System),
      screlease_scope: Some(FenceScope::System),
      ordered: true,
      kernel_object,
      kernel_args: args,
      completion_signal: &completion_signal,
    };
    info!("dispatching...");
    let start = Instant::now();
    let wait = kernel_queue
      .write_dispatch_packet(dispatch)
      .expect("write_dispatch_packet");
    wait.wait();
    let elapsed = start.elapsed();
    info!("dispatch {} took {}ms", 0, elapsed.as_millis());
  }

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
