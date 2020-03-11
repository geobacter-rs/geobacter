#![feature(core_intrinsics)]

extern crate ndarray as nd;
extern crate ndarray_parallel as ndp;
extern crate env_logger;
extern crate rand;

extern crate geobacter_runtime_core as rt_core;
#[macro_use]
extern crate geobacter_runtime_amd as rt_amd;
extern crate geobacter_amd_std as amdgpu_std;

use std::mem::{size_of, };
use std::time::Instant;

use alloc_wg::{vec::Vec, };

use ndp::prelude::*;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng, };

use rt_core::context::{Context, };
use rt_amd::HsaAmdGpuAccel;
use rt_amd::alloc::*;
use rt_amd::module::{Invoc, ArgsPool, };
use rt_amd::signal::*;

use amdgpu_std::{dispatch_packet, };

pub type Elem = f32;
const COUNT_MUL: usize = 64;
const COUNT: usize = 1024 * 1024 * COUNT_MUL;
const ITERATIONS: usize = 16;

const WG_SIZE: usize = 256;

/// This is the kernel that is run on the GPU
pub fn vector_foreach(args: &Args) {
  if let Some(tensor) = args.tensor_view() {
    let value = args.value;

    let idx = dispatch_packet().global_id_x();

    let dest = &mut tensor[idx as usize];

    calc(dest, value);
  }
}

fn calc(dest: &mut Elem, value: Elem) {
  let mut i = 0u16;
  while i < ITERATIONS as u16 {
    *dest += 1.0 as Elem;
    *dest *= value;

    i += 1;
  }
}

#[repr(C)] // Ensure we have a universally understood layout
#[derive(GeobacterDeps)]
pub struct Args {
  copy: DeviceSignal,
  tensor: *mut [Elem],
  pub value: Elem,
}

impl Args {
  pub fn tensor_view(&self) -> Option<&mut [Elem]> {
    unsafe {
      self.tensor.as_mut()
    }
  }
}

pub fn time<F, R>(what: &str, f: F) -> R
  where F: FnOnce() -> R,
{
  let start = Instant::now();
  let r = f();
  let elapsed = start.elapsed();
  println!("{} took {}ms ({}μs)", what,
           elapsed.as_millis(), elapsed.as_micros());

  r
}

pub fn main() {
  env_logger::init();
  let ctxt = time("create context", || {
    Context::new()
      .expect("create context")
  });

  let accels = HsaAmdGpuAccel::all_devices(&ctxt)
    .expect("HsaAmdGpuAccel::all_devices");
  if accels.len() < 1 {
    panic!("no accelerator devices???");
  }

  println!("allocating {} MB of host memory",
           COUNT * size_of::<Elem>() / 1024 / 1024);

  let lap_alloc = accels.first().unwrap().fine_lap_node_alloc(0);
  let mut original_values: Vec<Elem, _> = time("alloc original_values", || {
    Vec::with_capacity_in(COUNT, lap_alloc.clone())
  });
  unsafe {
    // no initialization:
    original_values.set_len(COUNT);
  }
  let mut values: LapVec<Elem> = time("alloc host output slice", || {
    LapVec::with_capacity_in(COUNT, lap_alloc.clone())
  });
  unsafe { values.set_len(COUNT); }

  let mut rng = SmallRng::from_entropy();

  // run the kernel 20 times for good measure.
  const RUNS: usize = 20;

  for iteration in 0..RUNS {
    println!("Testing iteration {}/{}..", iteration, RUNS);

    for value in original_values.iter_mut() {
      *value = rng.gen();
    }

    for accel in accels.iter() {
      println!("Testing device {}", accel.agent().name().unwrap());

      let mut invoc: Invoc<_, _> =
        Invoc::new(&accel, vector_foreach)
          .expect("Invoc::new");
      invoc.compile_async();
      unsafe {
        invoc.no_acquire_fence();
        invoc.device_release_fence();
      }

      let mut device_values_ptr = time("alloc device slice", || unsafe {
        accel.alloc_device_local_slice::<Elem>(COUNT)
          .expect("HsaAmdGpuAccel::alloc_device_local")
      });

      println!("host ptr: 0x{:p}, agent ptr: 0x{:p}",
               original_values.as_ptr(), device_values_ptr);

      let async_copy_signal = accel.new_device_signal(1)
        .expect("HsaAmdGpuAccel::new_device_signal: async_copy_signal");
      let kernel_signal = accel.new_host_signal(1)
        .expect("HsaAmdGpuAccel::new_host_signal: kernel_signal");
      let results_signal = accel.new_host_signal(1)
        .expect("HsaAmdGpuAccel::new_host_signal: results_signal");

      time("grant gpu access: `original_values` and `values`", || {
        original_values.add_access(&*accel)
          .expect("grant_agents_access");
        values.add_access(&*accel)
          .expect("grant_agents_access");
      });

      unsafe {
        accel.unchecked_async_copy_into(&original_values,
                                        &mut device_values_ptr,
                                        &(), &async_copy_signal)
          .expect("HsaAmdGpuAccel::async_copy_into");
      }

      let queue = accel.create_single_queue2(None, 0, 0)
        .expect("HsaAmdGpuAccel::create_single_queue");

      let args_pool = time("alloc kernargs pool", || {
        ArgsPool::new::<Args>(&accel, 1)
          .expect("ArgsPool::new")
      });

      const VALUE: Elem = 4.0 as _;
      let args = Args {
        copy: async_copy_signal,
        tensor: device_values_ptr.as_ptr(),
        value: VALUE,
      };

      invoc.workgroup_dims((WG_SIZE, ));
      invoc.grid_dims((COUNT, ));

      println!("dispatching...");
      let wait = time("dispatching", || unsafe {
        invoc.unchecked_call_async(args, &queue,
                                   kernel_signal,
                                   &args_pool)
          .expect("Invoc::call_async")
      });

      // specifically wait (without enqueuing another async copy) here
      // so we can time just the dispatch.
      time("dispatch wait", move || {
        wait.wait_for_zero(false)
          .expect("wait for zero failed");
      });

      // now copy the results back to the locked memory:
      unsafe {
        accel.unchecked_async_copy_from(&device_values_ptr,
                                        &mut values,
                                        &(), &results_signal)
          .expect("HsaAmdGpuAccel::async_copy_from");
      }
      time("gpu -> cpu async copy", || {
        results_signal.wait_for_zero(true)
          .expect("unexpected signal status");
      });

      let values = nd::aview1(&values);
      let original_values = nd::aview1(&original_values);

      // check results:
      time("checking results", || {
        nd::Zip::from(&values)
          .and(&original_values)
          .par_apply(|&lhs, &rhs| {
            let mut rhs = rhs;
            calc(&mut rhs, VALUE);
            assert_eq!(lhs, rhs);
          });
      });
    }
    println!();
  }
}
