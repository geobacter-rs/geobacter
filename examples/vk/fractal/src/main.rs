#![feature(rustc_private)]
#![feature(intrinsics)]
#![feature(custom_attribute)]
#![feature(core_intrinsics)]
#![feature(coerce_unsized)]

extern crate ndarray as nd;
extern crate geobacter_core;
extern crate runtime_core as rt;
#[macro_use]
extern crate log;
extern crate env_logger;
extern crate rand;
extern crate geobacter_std as gstd;
extern crate tempdir;
#[macro_use]
extern crate vulkano as vk;

use std::env::current_dir;
use std::ffi::CString;
use std::io::{Write, };
use std::mem::{size_of, };
use std::net::{Shutdown, };
use std::option::Option::None;
use std::os::unix::net::{UnixListener, };
use std::ops::{Deref, DerefMut, CoerceUnsized, };
use std::process::{Command, Stdio, };
use std::slice::from_raw_parts;
use std::sync::{Arc, };
use std::time::Instant;
use std::vec::Vec;

use vk::buffer::BufferUsage;
use vk::buffer::cpu_access::{CpuAccessibleBuffer, };
use vk::buffer::device_local::{DeviceLocalBuffer, };
use vk::command_buffer::{CommandBuffer, AutoCommandBufferBuilder, };
use vk::descriptor::descriptor_set::{PersistentDescriptorSet, DescriptorSet, };
use vk::descriptor::pipeline_layout::PipelineLayout;
use vk::device::{Device, Features, };
use vk::instance::{layers_list, Instance, InstanceExtensions, PhysicalDevice, debug::DebugCallback, debug::MessageTypes, RawInstanceExtensions};
use vk::pipeline::ComputePipeline;
use vk::sync::GpuFuture;

use rt::accelerators::VkAccel;
use rt::context::Context;
use rt::module::KernelDesc;

use gstd::kernel::global_invocation_id;
use gstd::mem::buffer::{BufferBinding, UniformBinding, AddBinding, };
use rt::Accelerator;

pub type Elem = Vec4<u8>;
// note the output resolution will be half of this due to
// supersampling. Currently we let ffmpeg do it, but in the future
// another kernel could do it here.
const X_SIZE: usize = 2560;
const Y_SIZE: usize = 1440;
//const Y_SIZE: usize = X_SIZE;
const PIXEL_LEN: usize = X_SIZE * Y_SIZE;

const X_CENTER_OFFSET: usize = X_SIZE * 3 / 4;
const Y_CENTER_OFFSET: usize = Y_SIZE * 2 / 5;
//const Y_CENTER_OFFSET: usize = X_CENTER_OFFSET;

const MAX_FRAMES: usize = 60;

const TAU: f32 = 0.0025;
const START_SCALE: f32 = 1.0;

#[repr(C)]
#[derive(Copy, Clone)]
struct Scale {
  scale: Vec2<f32>,
}

#[geobacter(set = "0", binding = "1",
            storage_class = "Uniform")]
static mut SCALE: Scale = Scale {
  scale: Vec2::new_c([0.0; 2]),
};
#[geobacter(set = "0", binding = "1")]
fn scale_binding() -> UniformBinding<Scale> {
  UniformBinding::new(&scale_binding)
}

/// SPIR-V (practically) requires all `Uniform` and `StorageBuffer` globals
/// be a structure decorated with `Block`; thus, at least until we can automate
/// this, one must wrap these in a structure.
#[derive(Clone, Copy)]
struct Pixels([Elem; PIXEL_LEN]);
impl Deref for Pixels {
  type Target = [Elem; PIXEL_LEN];
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}
impl DerefMut for Pixels {
  fn deref_mut(&mut self) -> &mut [Elem; PIXEL_LEN] {
    &mut self.0
  }
}
impl CoerceUnsized<[Elem]> for Pixels { }

#[geobacter(set = "0", binding = "0",
            storage_class = "StorageBuffer")]
static mut PIXELS: Pixels = Pixels([Elem::new_u8_c([0u8; 4]); PIXEL_LEN]);
#[geobacter(set = "0", binding = "0")]
fn pixels_binding() -> BufferBinding<Pixels> {
  BufferBinding::new(&pixels_binding)
}

#[geobacter(capabilities(Shader, Kernel))]
#[geobacter(local_size(x = 8, y = 8, z = 1))]
fn kernel() {
  let gid = global_invocation_id();
  let id_x = gid.x();
  let id_y = gid.y();
  let id = id_y * X_SIZE + id_x;

  let scale = unsafe { SCALE.scale };

  unsafe { ::std::intrinsics::assume(id < PIXEL_LEN) };

  if let Some(dest) = unsafe { PIXELS.get_mut(id) } {
    work(dest, scale, id_x, id_y);
  }
}

fn work(dest: &mut Elem, scale: Vec2<f32>, id_x: usize, id_y: usize) {
  let offset = Vec2::new([
    X_CENTER_OFFSET as f32,
    Y_CENTER_OFFSET as f32,
  ]);

  let image_size = Vec2::new([
    X_SIZE as f32,
    Y_SIZE as f32,
  ]);

  let id: Vec2<f32> = Vec2::new([id_x as f32, id_y as f32]);
  let id = id - offset;
  let id = id * scale;
  let id = id + offset;
  let norm = (id + 0.5f32) / image_size;
  let c = (norm - 0.5f32) * 2.0f32 - Vec2::new([1.0f32, 0.0]);

  let mut z: Vec2<f32> = Vec2::splat(0.0);
  let mut i = 0;

  loop {
    z = Vec2::new([
      z.x() * z.x() - z.y() * z.y(),
      z.y() * z.x() + z.x() * z.y(),
    ]) + c;

    if z.length_sqrd() > 16.0 {
      break;
    }

    i += 1;
    if i >= 200 { break; }
  }

  // use a simple cosine palette to determine color:
  // http://iquilezles.org/www/articles/palettes/palettes.htm
  let d = Vec3::new([0.3, 0.3, 0.5]);
  let e = Vec3::new([-0.2, -0.3, -0.5]);
  let f = Vec3::new([2.1, 2.0, 3.0]);
  let g = Vec3::new([0.0, 0.1, 0.0]);

  let i = (i as f32) * 0.005;
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

pub fn main() {
  println!("----- NEW RUN -----");
  let mut layers = vec![];
  for layer in layers_list().unwrap() {
    layers.push(layer.name().to_string());
  }

  env_logger::init();
  let ctxt = Context::new()
    .expect("create context");

  let kernel_desc = KernelDesc::new(ctxt.clone(),
                                    kernel);

  println!("capabilities: {:?}", kernel_desc.capabilities);

  let pipeline = kernel_desc.pipeline_desc;
  info!("pipeline desc: {:#?}", pipeline);
  let mut raw_exts = kernel_desc.raw_device_extensions();
  raw_exts.insert(CString::new("VK_KHR_storage_buffer_storage_class").unwrap());
  raw_exts.insert(CString::new("VK_KHR_variable_pointers").unwrap());

  let mut features = Features::none();
  features.robust_buffer_access = true;

  let app_info = app_info_from_cargo_toml!();
  let mut exts = InstanceExtensions::supported_by_core()
    .expect("InstanceExtensions::supported_by_core");
  exts.ext_debug_report = true;
  let mut exts: RawInstanceExtensions = From::from(&exts);
  exts.insert(CString::new("VK_EXT_debug_utils").unwrap());
  let instance = Instance::new(Some(app_info).as_ref(),
                               exts,
                               layers.iter().map(|s| &s[..] ))
    .expect("Instance::new");

  let mut debug_message_types = MessageTypes::none();
  debug_message_types.debug = true;
  debug_message_types.error = true;
  debug_message_types.information = true;
  debug_message_types.performance_warning = true;
  debug_message_types.warning = true;
  let _debug_callback =
    DebugCallback::new(&instance,
                       debug_message_types,
                       |msg| {
                         let ty = if msg.ty.error {
                           "error"
                         } else if msg.ty.warning {
                           "warning"
                         } else if msg.ty.performance_warning {
                           "performance_warning"
                         } else if msg.ty.information {
                           "information"
                         } else if msg.ty.debug {
                           "debug"
                         } else {
                           unimplemented!()
                         };

                         println!("{} {}: {}", msg.layer_prefix,
                                  ty, msg.description);
                       });

  for phy in PhysicalDevice::enumerate(&instance) {
    println!("Physical Device Name: `{}`", phy.name());

    let socket_dir = tempdir::TempDir::new("fractal-ffmpeg-socket")
      .expect("create tempdir for ffmpeg socket");
    let socket_addr = socket_dir.path()
      .join("sock.unix");

    let socket_listener = UnixListener::bind(&socket_addr)
      .expect("UnixListener::bind");

    let out = format!("out-{}.mkv", phy.name());

    let mut ffmpeg = {
      let mut cmd = Command::new("ffmpeg");
      cmd.current_dir(current_dir().unwrap())
        .arg("-loglevel").arg("panic")
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
        .arg(&out);

      cmd.stdin(Stdio::piped());

      info!("ffmpeg invocation: {:?}", cmd);

      cmd.spawn()
        .expect("failed to spawn ffmpeg")
    };

    println!("animation output: {}", out);

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

    // initialization
    let q_fam = phy.queue_families()
      .find(|fam| {
        fam.supports_compute()
      });
    if q_fam.is_none() {
      println!("Couldn't find compute queue family; skipping");
      continue;
    }
    let q_fam = q_fam.unwrap();
    let (device, mut queues) = Device::new(phy,
                                           &features,
                                           raw_exts.clone(),
                                           Some((q_fam, 1.0)))
      .expect("create vk device");
    let queue = queues.next()
      .expect("no compute queue?");

    let vk_accel = VkAccel::new(&ctxt,
                                device.clone(),
                                queue.clone())
      .expect("failed to create Geobacter accelerator");
    let vk_accel = Arc::new(vk_accel);
    let accel = vk_accel.clone() as Arc<Accelerator>;

    ctxt.add_accel(vk_accel.clone())
      .expect("failed to add accel to context");

    // finished core initialization

    // create the cpu accessable buffer and the device side buffer the kernel
    // will write to.

    let usage = BufferUsage {
      transfer_destination: true,
      transfer_source: true,
      storage_buffer: true,
      .. BufferUsage::none()
    };
    let device_buffer: Arc<DeviceLocalBuffer<[Elem]>> =
      DeviceLocalBuffer::array(device.clone(), PIXEL_LEN,
                               usage, Some(q_fam))
      .expect("DeviceLocalBuffer::array");

    let usage = BufferUsage {
      transfer_destination: true,
      storage_buffer: true,
      .. BufferUsage::none()
    };
    let cpu_buffer = unsafe {
      CpuAccessibleBuffer::uninitialized_array(device.clone(),
                                              PIXEL_LEN,
                                               usage)
        .expect("failed to create buffer")
    };

    let usage = BufferUsage {
      transfer_destination: true,
      uniform_buffer: true,
      .. BufferUsage::none()
    };
    let scale_uniform = DeviceLocalBuffer::new(device.clone(),
                                               usage, Some(q_fam))
      .expect("create scale storage uniform");

    let pipeline = PipelineLayout::new(device.clone(),
                                       pipeline)
      .expect("PipelineLayout::new");
    let pipeline_layout = Arc::new(pipeline);

    let compiled = kernel_desc.compile(&accel)
      .expect("failed to compile kernel");

    let pipeline = ComputePipeline::new(device.clone(),
                                        &compiled,
                                        &())
      .expect("create compute pipeline");
    let pipeline = Arc::new(pipeline);

    let desc_set_builder =
      PersistentDescriptorSet::start(pipeline_layout.clone(),
                                     0);

    let desc_set_builder = desc_set_builder
      .add_binding(&scale_binding(), scale_uniform.clone())
      .expect("scale set/binding mismatch (this *shouldn't* happen if \
              this code compiled)")
      .add_slice_binding(&pixels_binding(), device_buffer.clone())
      .expect("pixels set/binding mismatch (this *shouldn't* happen if \
              this code compiled)");

    let desc_set = desc_set_builder.build()
      .expect("failed to build desc set");
    let desc_set = Arc::new(desc_set) as Arc<dyn DescriptorSet + Send + Sync>;

    let mut frame_data: Vec<_> = vec![];
    frame_data.resize(PIXEL_LEN, Elem::new_u8_c([0u8; 4]));

    for frame_index in 0..MAX_FRAMES {
      let scale = 1.0 / (START_SCALE + frame_index as f32 * TAU);

      print!("current frame: {} ... ", frame_index - 1);

      let desc_sets = vec![desc_set.clone()];

      let copy_cmd_buf =
        AutoCommandBufferBuilder::primary_one_time_submit(device.clone(),
                                                          queue.family())
          .expect("failed to create auto cmd buf")
          .copy_buffer(device_buffer.clone(), cpu_buffer.clone())
          .expect("failed to build cmd buf dispatch")
          .build()
          .expect("build cmd buf");

      let update_scale =
        AutoCommandBufferBuilder::primary_one_time_submit(device.clone(),
                                                          queue.family())
          .expect("failed to create auto cmd buf")
          .update_buffer(scale_uniform.clone(), Scale {
            scale: Vec2::splat(scale),
          })
          .expect("failed to build cmd buf update_buffer")
          .dispatch([(X_SIZE / 8) as _, (Y_SIZE / 8) as _, 1],
                    pipeline.clone(), desc_sets, ())
          .expect("failed to build cmd buf dispatch")
          .build()
          .expect("build cmd buf");

      let kf = update_scale
        .execute(queue.clone())
        .expect("execute update scale uniform");

      {
        let r = cpu_buffer.write().expect("pixels buffer read");
        frame_data.copy_from_slice(&*r);
      }

      /*let mut read: Vec<Elem> = vec![];
          read.resize(PIXEL_LEN, Elem::new_u8_c([0u8; 4]));
          for id_y in 0..Y_SIZE {
            for id_x in 0..X_SIZE {
              let dest = read.get_mut(id_y * X_SIZE + id_x).unwrap();
              work(dest, Vec2::splat(scale), id_x, id_y);
            }
          }*/

      // the work on the cpu pixel buffer is complete,
      let kf = kf.then_execute_same_queue(copy_cmd_buf)
        .expect("enqueue device -> cpu buffer copy")
        .then_signal_fence_and_flush()
        .expect("then_signal_fence_and_flush: copy_buffer");

      debug!("sending frame #{} to ffmpeg", frame_index - 1);
      let bytes = unsafe {
        from_raw_parts(frame_data.as_ptr() as *const u8,
                       PIXEL_LEN * size_of::<Elem>())
      };
      let written = ffmpeg_write_sock.write(bytes)
        .expect("ffmpeg write failure");
      assert_eq!(written, bytes.len(),
                 "OS isn't writing the whole frame as required");

      let start = Instant::now();
      kf.wait(None)
        .expect("GpuFuture::wait: copy_buffer");
      let elapsed = start.elapsed().as_micros();
      println!("frame took {}us", elapsed);
    }

    // XXX technically the last computed frame is thrown away.

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

    println!("finished work on {}", phy.name());
  }
}
