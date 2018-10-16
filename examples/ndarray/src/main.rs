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

use std::time::Instant;

use rand::rngs::SmallRng;
use rand::{Rng, FromEntropy, };

use hsa_rt::{ext::HostLockedAgentMemory, };
use hsa_rt::agent::{DeviceType, Profiles, DefaultFloatRoundingModes, };
use hsa_rt::code_object::CodeObjectReaderRef;
use hsa_rt::executable::{CommonExecutable, Executable, };
use hsa_rt::queue::{QueueType, DispatchPacket, };
use hsa_rt::signal::Signal;

use runtime::context::{Context, OutputType, };

pub fn vector_fill_chunked(into: &mut [f64],
                           value: f64) {
  for chunk in into.chunks_mut(64) {
    for chunk in chunk.chunks_mut(8) {
      for into in chunk.iter_mut() {
        *into += value;
      }
    }
  }
}
pub fn vector_fill_single(mut into: nd::ArrayViewMut1<f64>,
                          value: f64) {
  for into in into.iter_mut() {
    *into += value;
  }
}
pub fn vector_fill(into: nd::ArrayViewMut1<f64>,
                   value: f64) {
  vector_fill_single(into, value)
}

pub fn matrix_matrix_mul(alpha: f64,
                         a: nd::ArrayView2<f64>,
                         b: nd::ArrayView2<f64>,
                         beta: f64,
                         mut c: nd::ArrayViewMut2<f64>)
{
  nd::linalg::general_mat_mul(alpha, &a, &b, beta, &mut c);
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

  let id = hsa_core::kernel::kernel_id_for(&matrix_matrix_mul);
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

  //const COUNT: usize = 512 * 1024 * 1024 / size_of::<f64>();
  //info!("allocating {} MB of agent memory", COUNT * size_of::<f64>() / 1024 / 1024);

  const SIDE: usize = 1000;
  const DIM: (usize, usize) = (SIDE, SIDE);
  let alpha = 1.0f64;
  let beta = 0.0f64;

  let mut a = nd::Array::zeros(DIM);
  let mut b = nd::Array::zeros(DIM);

  let c = nd::Array::zeros(DIM);

  //let mut values = vec![0.0; COUNT];

  let mut rng = SmallRng::from_entropy();
  for value in a.iter_mut() {
    *value = rng.gen();
  }
  for value in b.iter_mut() {
    *value = rng.gen();
  }

  let start = Instant::now();
  //let mut values = values.lock_memory(Some(agent).into_iter())
  //  .expect("lock host memory for agent");
  let a = a.lock_memory(Some(agent).into_iter())
    .expect("lock host memory for agent");
  let b = b.lock_memory(Some(agent).into_iter())
    .expect("lock host memory for agent");
  let mut c = c.lock_memory(Some(agent).into_iter())
    .expect("lock host memory for agent");
  let elapsed = start.elapsed();
  info!("mem lock took {}us", elapsed.as_micros());
  //info!("agent_ptr {:p}", values.agent_ptr());

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

  struct MatrixMatrixMulArgs<'a> {
    alpha: f64,
    a: nd::ArrayView2<'a, f64>,
    b: nd::ArrayView2<'a, f64>,
    beta: f64,
    c: nd::ArrayViewMut2<'a, f64>,
  }

  struct Args<'a> {
    into: nd::ArrayViewMut1<'a, f64>,
    value: f64,
  }

  {
    /*let args: &mut Args = unsafe {
      let args = kernel_arg_region.allocate::<Args>(1)
        .expect("allocate kernel arg");
      ::std::mem::transmute(args)
    };
    args.into = nd::aview_mut1(values.as_agent_mut());
    args.value = 4.0;*/
    let args: &mut MatrixMatrixMulArgs = unsafe {
      let args = kernel_arg_region.allocate::<MatrixMatrixMulArgs>(1)
        .expect("allocate kernel arg");
      ::std::mem::transmute(args)
    };
    args.alpha = alpha;
    args.a = a.agent_view();
    args.b = b.agent_view();
    args.beta = beta;
    args.c = c.agent_view_mut();

    let dispatch = DispatchPacket {
      workgroup_size: (1, ),
      grid_size: (1, ),
      group_segment_size: 4096,
      private_segment_size: 472,
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
    info!("dispatch took {}ms", elapsed.as_millis());
  }
}
