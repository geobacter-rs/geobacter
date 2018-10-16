#![feature(rustc_private, duration_as_u128)]

extern crate ndarray as nd;
extern crate hsa_core;
extern crate hsa_rt;
extern crate hsa_rt_sys;
extern crate runtime;
#[macro_use]
extern crate log;
extern crate env_logger;
extern crate rand;
extern crate rustc_driver;

use std::io::{stdout, Write, };
use std::slice::from_raw_parts_mut;
use std::mem::size_of;
use std::time::Instant;

use rand::rngs::SmallRng;
use rand::{Rng, FromEntropy, };

use hsa_rt::{ApiContext, ffi, };
use hsa_rt::agent::{DeviceType, Profiles, DefaultFloatRoundingModes, };
use hsa_rt::code_object::CodeObjectReaderRef;
use hsa_rt::executable::{CommonExecutable, Executable, };
use hsa_rt::queue::{QueueType, DispatchPacket, };
use hsa_rt::signal::Signal;
use hsa_rt::mem::region::Region;

use runtime::context::{Context, OutputType, };

pub fn vector_fill(into: &mut [f64],
                   value: f64) {
  for chunk in into.chunks_mut(64) {
    for chunk in chunk.chunks_mut(8) {
      for into in chunk.iter_mut() {
        *into += value;
      }
    }
  }
}
pub fn mat_mat_mul(alpha: f32,
                   a: &nd::ArrayView2<f32>,
                   b: &nd::ArrayView2<f32>,
                   beta: f32,
                   c: &mut nd::ArrayViewMut2<f32>) {
  nd::linalg::general_mat_mul(alpha, a, b, beta, c)
}

fn debug_region(region: &Region) {
  println!("\tRegion ID(0x{:x}):", region.id());
  println!("\t\tSegment: {:?}", region.segment().expect("can't get region segment"));
  println!("\t\tGlobal Flags: {:?}",
           region.global_flags().expect("can't get region global flags"));
  println!("\t\tSize: {}", region.size().expect("can't get region size"));
  println!("\t\tAlloc Max Size: {}", region.alloc_max_size().unwrap());
  println!("\t\tAlloc Max Private Workgroup Size: {}",
           match region.alloc_max_private_workgroup_size() {
             Ok(size) => format!("{}", size),
             Err(_) => "N/A".to_string(),
           });
  println!("\t\tRuntime Alloc Allowed: {}",
           region.runtime_alloc_allowed().unwrap());
  println!("\t\tRuntime Alloc Granule: {}",
           region.runtime_alloc_granule().unwrap());
  println!("\t\tRuntime Alloc Alignment: {}",
           region.runtime_alloc_alignment().unwrap());
}

const SYMBOL_NAME: &'static str = "_ZN7ndarray11vector_fill17h261b425b617eccaeE";

pub fn main() {
  env_logger::init();
  //rustc_driver::env_logger::init();
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

  let id = hsa_core::kernel::kernel_id_for(&vector_fill);
  let outputs = ctxt.manually_compile(&accel, id)
    .expect("codegen failed");

  let asm = outputs.get(&OutputType::Assembly)
    .expect("no asm output?");

  //let mut out = stdout();
  //out.write_all(asm.as_ref()).expect("write failed");

  let exe_bin = outputs.get(&OutputType::Exe)
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

  let alloc_region = regions
    .iter()
    .filter(|region| {
      region.runtime_alloc_allowed()
        .unwrap_or(false)
    })
    .find(|region| {
      match region.global_flags() {
        Ok(Some(flags)) => flags.fine_grained(),
        _ => false,
      }
    })
    .expect("no fine alloc region");

  info!("allocating agent memory");

  const COUNT: usize = 1024 / size_of::<f64>();

  let agent_values = unsafe {
    let ptr = alloc_region.allocate::<f64>(COUNT)
      .expect("not enough agent memory");

    from_raw_parts_mut(ptr, COUNT * size_of::<f64>())
  };

  let mut values = vec![0.0; COUNT];
  info!("allocated agent memory");

  let mut rng = SmallRng::from_entropy();
  for value in values.iter_mut() {
    *value = rng.gen();
  }

  let start = Instant::now();
  assert_eq!(unsafe {
    ffi::hsa_memory_copy(agent_values.as_mut_ptr() as *mut _,
                         values.as_ptr() as *const _,
                         values.len())
  },
             0);
  let elapsed = start.elapsed();
  let micros = elapsed.as_micros();
  println!("mem copy took {}us", micros);

  let symbols = exe.agent_symbols(agent)
    .expect("agent symbols");
  let kernel_symbol = symbols.into_iter()
    .find(|symbol| {
      match symbol.name() {
        Ok(ref n) if n == SYMBOL_NAME => true,
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
    into: &'a mut [f64],
    value: f64,
  }
  assert_eq!(size_of::<Args>(), 24);

  {
    let args: &mut Args = unsafe {
      let args = kernel_arg_region.allocate::<Args>(1)
        .expect("allocate kernel arg");
      ::std::mem::transmute(args)
    };
    args.into = agent_values;
    args.value = 4.0;

    let dispatch = DispatchPacket {
      workgroup_size: (1, ),
      grid_size: (1, ),
      group_segment_size: 4096,
      private_segment_size: 128,
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
    let millis = elapsed.as_millis();
    println!("dispatch took {}ms", millis);

  }

  unsafe {
    alloc_region.deallocate(agent_values.as_mut_ptr())
      .expect("free values");
  }
}
