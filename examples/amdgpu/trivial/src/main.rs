
extern crate grt_amd as geobacter_runtime_amd;

use std::mem::{size_of, };
use std::ops::*;
use std::time::Instant;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng, };

use grt_core::context::{Context, };
use grt_amd::{HsaAmdGpuAccel, GeobacterDeps, };
use grt_amd::alloc::*;
use grt_amd::module::*;
use grt_amd::signal::*;

pub type Elem = f32;
const COUNT_MUL: usize = 64;
const COUNT: usize = 1024 * 1024 * COUNT_MUL;
const ITERATIONS: usize = 16;

const WG_SIZE: usize = 256;

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
  queue: DeviceSingleQueue,
  completion: GlobalSignal,
}
impl Completion for Args {
  type CompletionSignal = GlobalSignal;
  fn completion(&self) -> &GlobalSignal { &self.completion }
}
impl Kernel for Args {
  type Grid = Dim1D<Range<u32>>;
  const WORKGROUP: <Self::Grid as GridDims>::Workgroup = Dim1D {
    x: ..WG_SIZE as _,
  };
  type Queue = DeviceSingleQueue;

  fn queue(&self) -> &Self::Queue {
    &self.queue
  }

  /// This is the kernel that is run on the GPU
  fn kernel(&self, vp: KVectorParams<Self>)
    where Self: Sized,
  {
    if let Some(tensor) = self.tensor_view() {
      let idx = vp.gl_id();
      if let Some(dest) = tensor.get_mut(idx as usize) {
        calc(dest, self.value);
      }
    }
  }
}

impl Args {
  pub fn tensor_view(&self) -> Option<&mut [Elem]> {
    unsafe {
      self.tensor.as_mut()
    }
  }
}
unsafe impl Send for Args { }
unsafe impl Sync for Args { }

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
  let mut original_values: LapVec<Elem> = time("alloc original_values", || {
    LapVec::with_capacity_in(COUNT, lap_alloc.clone())
  });
  original_values.resize_with(COUNT, Default::default);
  let mut values: LapVec<Elem> = time("alloc host output slice", || {
    LapVec::with_capacity_in(COUNT, lap_alloc.clone())
  });
  values.resize_with(COUNT, Default::default);

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

      let mut invoc: FuncModule<Args> =
        FuncModule::new(&accel);
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
      let kernel_signal = GlobalSignal::new(1)
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
        completion: kernel_signal,
        queue,
      };

      let grid = Dim1D {
        x: 0u32..COUNT as _,
      };

      invoc.compile().expect("codegen failed");

      let mut invoc = invoc.into_invoc(&args_pool);

      println!("dispatching...");
      let wait = time("dispatching", || unsafe {
        invoc.unchecked_call_async(&grid, args)
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
