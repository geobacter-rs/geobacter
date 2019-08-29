
#![feature(rustc_private, duration_as_u128)]
#![feature(intrinsics)]
#![feature(custom_attribute)]
#![feature(core_intrinsics)]

extern crate ndarray as nd;
extern crate geobacter_core;
extern crate hsa_rt;
extern crate hsa_rt_sys;
extern crate runtime;
#[macro_use]
extern crate log;
extern crate env_logger;
extern crate rand;
extern crate geobacter_std;
extern crate rustc_driver;
extern crate lodepng;

use std::mem::{size_of, };
use std::time::Instant;

use hsa_rt::{ext::HostLockedAgentMemory, };
use hsa_rt::agent::{DeviceType, };

use runtime::context::{Context, };
use runtime::module::Invoc;

use geobacter_std::{dispatch_packet, };

use lodepng::{encode32_file, RGBA, };

pub type Elem = Vec4<u8>;
// 4096 x 4096
const X_SIZE: usize = 1024 * 4;
const Y_SIZE: usize = X_SIZE;

const WORKITEM_SIZE: usize = 8;

fn work(args: Args) {
  let Args {
    image,
    scale,
  } = args;

  let mut args = UnpackedArgs {
    image,
  };

  struct UnpackedArgs<'a> {
    image: nd::ArrayViewMut2<'a, Elem>,
  }
  impl<'a> UnpackedArgs<'a> {
    pub fn image_size(&self) -> Vec2<f32> {
      let x = self.image.len_of(nd::Axis(0)) as f32;
      let y = self.image.len_of(nd::Axis(1)) as f32;

      Vec2::new([x, y])
    }
    pub fn write_pixel(&mut self, ix: (usize, usize),
                       px: Elem) {
      if let Some(dest) = self.image.get_mut(ix) {
        *dest = px;
      }
    }
  }
  let dispatch = dispatch_packet();
  let id_x = dispatch.global_id_x();
  let id_y = dispatch.global_id_y();
  let id: Vec2<f32> = Vec2::new([id_x as f32, id_y as f32]);
  let norm = (id + 0.5f32) * scale / args.image_size();
  let c = (norm - 0.5f32) * 2.0f32 - Vec2::new([1.0f32, 0.0]);

  let mut z: Vec2<f32> = Vec2::splat(0.0);
  let mut i = 0.0f32;

  loop {
    z = Vec2::new([
      z.x() * z.x() - z.y() * z.y() + c.x(),
      2.0 * z.x() * z.y() + c.y(),
    ]);

    if z.length() > (1 << 16) as f32 {
      break;
    }

    i += 0.005;
    if i >= 1.0 { break; }
  }

  let zx = if z.x() > 1.0 {
    1.0
  } else {
    z.x()
  };
  let zy = if z.y() > 1.0 {
    1.0
  } else {
    z.y()
  };

  let mut write: Vec4<f32> = Vec4::new([i, zx, zy, 1.0f32]);
  write *= u8::max_value() as f32;

  // XXX casts are currently ugly.
  let w0 = write.w() as u8;
  let w1 = write.x() as u8;
  let w2 = write.y() as u8;
  let w3 = write.z() as u8;

  let write: Vec4<u8> = Vec4::new([w0, w1, w2, w3]);

  args.write_pixel((id_x, id_y), write);
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
    .unwrap_or_else(|| {
      error!("error finding suitable GPU, falling back to CPU");
      ctxt.primary_host_accel().unwrap()
    });

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

  info!("allocating {} MB of host memory",
        X_SIZE * Y_SIZE * size_of::<Elem>() / 1024 / 1024);

  let frames: nd::Array<Elem, _> =
    nd::Array::default((X_SIZE, Y_SIZE, ));

  let start = Instant::now();
  let mut values = frames
    .lock_memory(vec![accel.agent()].into_iter())
    .expect("lock host memory for agent");
  let elapsed = start.elapsed();
  info!("mem lock took {}ms", elapsed.as_millis());

  invoc.workgroup_dims((WORKITEM_SIZE, WORKITEM_SIZE, ))
    .expect("Invoc::workgroup_dims");
  invoc.grid_dims((X_SIZE, Y_SIZE, ))
    .expect("Invoc::grid_dims");

  let args = Args {
    image: values.agent_view_mut(),
    scale: 1.0,
  };

  info!("dispatching...");
  let start = Instant::now();
  invoc.call_accel_sync(&accel, (args, ))
    .expect("Invoc::call_accel_sync");
  let elapsed = start.elapsed();
  info!("dispatch took {}ms", elapsed.as_millis());


  // write the output image to `out.png`:
  let out_filename = "out.png";
  let values = values.unlock();
  let buffer = values.as_slice_memory_order()
    .expect("non-continuous memory order");
  let buffer: &[RGBA] = unsafe {
    ::std::slice::from_raw_parts(buffer.as_ptr() as *const _,
                                 buffer.len())
  };
  encode32_file(out_filename, buffer, X_SIZE, Y_SIZE)
    .expect("write output png");
}
