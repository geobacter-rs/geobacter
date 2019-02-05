#![feature(rustc_private, duration_as_u128)]
#![feature(intrinsics)]
#![feature(custom_attribute)]
#![feature(core_intrinsics)]
#![feature(allocator_api)]

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
extern crate tempdir;

use std::alloc::{Global, Alloc, Layout, };
use std::env::current_dir;
use std::io::{Write, };
use std::mem::{size_of, };
use std::net::{Shutdown, };
use std::os::unix::net::{UnixListener, UnixStream, };
use std::process::{Command, Stdio, Child, };
use std::slice::from_raw_parts;
use std::sync::{Arc, };

use hsa_core::unit::{Vec2, Vec3, Vec4, VecFloat, };

use hsa_rt::ext::amd::*;
use hsa_rt::agent::{DeviceType, };
use hsa_rt::signal::{Signal, WaitState, ConditionOrdering, };
use hsa_rt::queue::{FenceScope, KernelMultiQueue, };

use runtime::Accelerator;
use runtime::context::{Context, };
use runtime::module::{Invoc, ArgsStorage, };

use legionella_std::{dispatch_packet, mem::*, };

pub type Elem = Vec4<u8>;
// note the output resolution will be half of this due to
// supersampling. Currently we let ffmpeg do it, but in the future
// another kernel could do it here.
const X_SIZE: usize = 2560 * 4;
//const Y_SIZE: usize = 1440 * 4;
const Y_SIZE: usize = X_SIZE;
const PIXEL_LEN: usize = X_SIZE * Y_SIZE;

const X_CENTER_OFFSET: usize = X_SIZE * 3 / 4;
//const Y_CENTER_OFFSET: usize = Y_SIZE * 2 / 5;
const Y_CENTER_OFFSET: usize = X_CENTER_OFFSET;

const MAX_FRAMES: usize = 60;
const QUEUE_LEN: usize = 6; // power of 2

const TAU: f32 = 0.0025;
const START_SCALE: f32 = 1.0;

const WORKITEM_SIZE: usize = 16;
const OUT_NAME: &'static str = "out9.mkv";

fn work(args: Args) {
  let Args {
    mut image,
    scale,
  } = args;

  let dispatch = dispatch_packet();

  let id_x = dispatch.global_id_x();
  let id_y = dispatch.global_id_y();

  let offset = Vec2::new([
    X_CENTER_OFFSET as f32,
    Y_CENTER_OFFSET as f32,
  ]);

  if let Some(dest) = image.get_mut((id_y, id_x)) {
    let image_size = Vec2::new([
      X_SIZE as f32,
      Y_SIZE as f32,
    ]);

    let id: Vec2<f32> = Vec2::new([id_x as f32, id_y as f32]);
    let id = id - offset;
    let id = id / scale;
    let id = id + offset;
    let norm = (id + 0.5f32) / image_size;
    let c = (norm - 0.5f32) * 2.0f32 - Vec2::new([1.0f32, 0.0]);

    let mut z: Vec2<f32> = Vec2::splat(0.0);
    let mut i = 0.0f32;

    loop {
      z = Vec2::new([
        z.x() * z.x() - z.y() * z.y() + c.x(),
        2.0 * z.x() * z.y() + c.y(),
      ]);

      if z.length_sqrd() > 40.0 {
        break;
      }

      i += 0.005;
      if i >= 1.0 { break; }
    }

    // use a simple cosine palette to determine color:
    // http://iquilezles.org/www/articles/palettes/palettes.htm
    let d = Vec3::new([0.3, 0.3, 0.5]);
    let e = Vec3::new([-0.2, -0.3, -0.5]);
    let f = Vec3::new([2.1, 2.0, 3.0]);
    let g = Vec3::new([0.0, 0.1, 0.0]);


    let i: Vec3<f32> = Vec3::splat(i);
    let t = (f * i + g) * 6.28318;
    let t = Vec3::new([
      t.x().cos(),
      t.y().cos(),
      t.z().cos(),
    ]);
    let mut write = d + e * t;

    write *= u8::max_value() as f32;

    // XXX casts are currently ugly.
    let w0 = write.x() as u8;
    let w1 = write.y() as u8;
    let w2 = write.z() as u8;
    let w3 = u8::max_value();

    *dest = Vec4::new([w0, w1, w2, w3]);
  }
}

#[repr(packed)]
pub struct Args<'a> {
  image: nd::ArrayViewMut2<'a, Elem>,
  scale: f32,
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

  let mut invoc = Invoc::new(ctxt.clone(), work)
    .expect("Invoc::new");

  invoc.workgroup_dims((WORKITEM_SIZE, WORKITEM_SIZE, ))
    .expect("Invoc::workgroup_dims");
  invoc.grid_dims((X_SIZE, Y_SIZE, ))
    .expect("Invoc::grid_dims");
  //invoc.begin_fence = Some(FenceScope::System);
  invoc.end_fence = Some(FenceScope::System);

  let kernel_props = invoc.precompile(&accel)
    .expect("kernel compilation");

  // XXX assumes all Cpus are local.
  let host_accels = ctxt
    .filter_accels(|accel| {
      match accel.agent().device_type() {
        Ok(DeviceType::Cpu) => true,
        _ => false,
      }
    })
    .expect("Context::filter_accels");
  let host_pools: Vec<_> = host_accels
    .iter()
    .map(|accel| {
      let pool = accel.agent().amd_memory_pools()
        .expect("get host pools")
        .into_iter()
        .filter(|pool| pool.alloc_allowed() )
        .filter(|pool| pool.segment().ok() == Some(Segment::Global) )
        .find(|pool| {
          if let Ok(Some(flags)) = pool.global_flags() {
            flags.fine_grained()
          } else {
            false
          }
        })
        .expect("host accels has no global segment memory pools");

      (accel, pool)
    })
    .collect();
  assert_eq!(host_pools.len(), host_accels.len());

  let agent = accel.agent();

  let frame_bytes = PIXEL_LEN * size_of::<Elem>();

  let group_size = Some(kernel_props.group_segment_size());
  let private_size = Some(kernel_props.private_segment_size());

  let host_pixels = unsafe {
    // allocate the host frame data w/ page alignment.
    let host_frame_layout =
      Layout::from_size_align(frame_bytes, 4096)
        .unwrap();
    let host_frame_ptr = Global.alloc(host_frame_layout)
      .expect("allocate frame data");
    Vec::from_raw_parts(host_frame_ptr.as_ptr() as *mut Elem,
                        PIXEL_LEN, PIXEL_LEN)
  };
  let host_pixels = nd::Array2::from_shape_vec((X_SIZE, Y_SIZE),
                                               host_pixels)
    .unwrap();
  let mut pixels = host_pixels.lock_memory(&[agent])
    .expect("lock host memory to GPU");

  let queue = agent.new_kernel_multi_queue(QUEUE_LEN, group_size,
                                           private_size)
    .expect("Agent::new_kernel_multi_queue");

  let socket_dir = tempdir::TempDir::new("fractal-ffmpeg-socket")
    .expect("create tempdir for ffmpeg socket");
  let socket_addr = socket_dir.path()
    .join("sock.unix");

  let socket_listener = UnixListener::bind(&socket_addr)
    .expect("UnixListener::bind");

  let mut ffmpeg = {
    let mut cmd = Command::new("ffmpeg");
    cmd.current_dir(current_dir().unwrap())
      // input options
      .arg("-f").arg("rawvideo")
      .arg("-framerate").arg("4")
      .arg("-pixel_format").arg("rgba")
      .arg("-video_size").arg(format!("{}x{}", X_SIZE, Y_SIZE))
      .arg("-i").arg(format!("unix://{}", socket_addr.display()))

      // supersample
      .arg("-filter:v").arg(format!("scale={}:{}", X_SIZE / 2, Y_SIZE / 2))

      // encode options
      .arg("-vcodec").arg("libx264")
      .arg("-preset").arg("veryfast")
      .arg("-tune").arg("animation")
      .arg("-profile:v").arg("high444")
      .arg("-level").arg("6.2")
      .arg("-crf").arg("17")

      // output options
      .arg("-f").arg("matroska").arg("-y")
      .arg(OUT_NAME);

    cmd.stdin(Stdio::piped());

    info!("ffmpeg invocation: {:?}", cmd);

    cmd.spawn()
      .expect("failed to spawn ffmpeg")
  };

  let mut ffmpeg_write_sock = loop {
    match socket_listener.accept() {
      Ok((stream, addr)) => {
        info!("(ffmpeg socket:) accepting connection from {:?}", addr);
        break stream
      },
      Err(err) => {
        error!("(ffmpeg socket:) error accepting incoming connection: {:?}, retrying", err);
      }
    }
  };

  let mut buffer: Vec<Elem> = Vec::with_capacity(PIXEL_LEN);
  let mut args_storage = invoc.allocate_kernarg_storage(&accel)
    .expect("allocate kernel argument storage");

  let kernel_completion = Signal::new(1, &[])
    .expect("create kernel_completion signal");

  let mut frame_index = 0;

  'outer: loop {
    debug!("rendering frame #{}", frame_index);

    render_frame(&accel, &mut invoc, &mut pixels, &queue, &mut ffmpeg,
                 &mut ffmpeg_write_sock, &mut buffer, &mut args_storage,
                 &kernel_completion, frame_index);

    if frame_index >= MAX_FRAMES { break; }

    frame_index += 1;
    unsafe { buffer.set_len(PIXEL_LEN); }
    let src = pixels.host_view();
    let src = src.as_slice_memory_order()
      .expect("pixels should be in memory order");
    buffer.copy_from_slice(src);
  }

  // close ffmpeg's stdin. This should cause it to finish encoding
  // and exit.
  ffmpeg.stdin.take();
  ffmpeg_write_sock.shutdown(Shutdown::Both)
    .expect("unix socket shutdown");

  info!("waiting for ffmpeg to finish");
  let ffmpeg_status = ffmpeg.wait()
    .expect("wait on ffmpeg to finish");

  assert!(ffmpeg_status.success(),
          "ffmpeg failed with code: {:?}",
          ffmpeg_status.code());
}

fn render_frame<F>(accel: &Arc<Accelerator>,
                   invoc: &mut Invoc<F, (Args, ), (usize, usize), (usize, usize)>,
                   pixels: &mut HostLockedAgentPtr<nd::Array2<Elem>>,
                   queue: &KernelMultiQueue,
                   ffmpeg: &mut Child,
                   ffmpeg_write_sock: &mut UnixStream,
                   mut buffer: &mut Vec<Elem>,
                   mut args_storage: &mut ArgsStorage,
                   kernel_completion: &Signal,
                   frame_index: usize)
  where F: for<'a> Fn(Args<'a>)
{
  let mut call = None;
  if frame_index < MAX_FRAMES {
    debug!("queuing call_async for this frame");
    kernel_completion.store_screlease(1);

    let image = pixels.agent_view_mut()
      .into_shape((Y_SIZE, X_SIZE, ))
      .unwrap();
    let scale = 1.0 / (START_SCALE + frame_index as f32 * TAU);
    let args = Args {
      image,
      scale,
    };
    let call_ = invoc.call_async((args, ), accel,
                                 queue, kernel_completion,
                                 args_storage);
    assert!(call_.is_ok(), "kernel enqueue error: {:?}", call_.err().unwrap());

    call = call_.ok();
  }

  // check that ffmpeg is still running
  match ffmpeg.try_wait() {
    Ok(Some(status)) => {
      // ffmpeg crashed/errored
      panic!("ffmpeg is not running: exit {:?}", status);
    },
    Ok(None) => {},
    Err(err) => {
      error!("error on Child::try_wait: {:?}", err);
    }
  }

  if frame_index > 0 {
    debug!("sending frame #{} to ffmpeg", frame_index - 1);
    let bytes = unsafe {
      from_raw_parts(buffer.as_ptr() as *const u8,
                     PIXEL_LEN * size_of::<Elem>())
    };
    let written = ffmpeg_write_sock.write(bytes)
      .expect("ffmpeg write failure");
    assert_eq!(written, bytes.len(),
               "OS isn't writing the whole frame as required");

    if frame_index == MAX_FRAMES {
      debug!("got last frame");
      return;
    }
  }

  if let Some(call) = call {
    debug!("waiting frame #{} (kernel) to finish", frame_index);
    call.wait(true);
  }
}
