

extern crate grt_amd as geobacter_runtime_amd;

use std::mem::{size_of, };
use std::time::Instant;
use std::rc::Rc;

use gstd_amd::*;
use grt_amd::{*, alloc::*, module::*, signal::*, };

use packed_simd::*;

use lodepng::{encode32_file, RGBA, };

pub type Elem = u8x4;
const X_SIZE: usize = 1024 * 32;
const Y_SIZE: usize = X_SIZE;

const WORKITEM_SIZE: usize = 16;

fn work(args: &Args) {
  let dispatch = dispatch_packet();
  let id_x = dispatch.global_id_x();
  let id_y = dispatch.global_id_y();
  let id = f32x2::new(id_x as f32, id_y as f32);
  let norm = (id + 0.5f32) * args.scale / args.image_size();
  let c = (norm - 0.5f32) * 2.0f32 - f32x2::new(1.0f32, 0.0);

  let c = (c.extract(0), c.extract(1));

  let mut z = (0.0f32, 0.0f32);
  let mut i = 0.0f32;

  loop {
    z = (
      z.0 * z.0 - z.1 * z.1 + c.0,
      2.0 * z.0 * z.1 + c.1,
    );

    if (z.0 * z.0 + z.1 * z.1) > (1 << 16) as f32 {
      break;
    }

    i += 0.005;
    if i >= 1.0 { break; }
  }

  let zx = if z.0 > 1.0 {
    1.0
  } else {
    z.0
  };
  let zy = if z.1 > 1.0 {
    1.0
  } else {
    z.1
  };

  let mut write = f32x4::new(i, zx, zy, 1.0f32);
  write *= u8::max_value() as f32;

  args.write_pixel((id_x as _, id_y as _), write.cast());
}

#[derive(GeobacterDeps)]
pub struct Args {
  image: *mut [Elem],
  scale: f32,
}
impl Args {
  pub fn image_size(&self) -> f32x2 {
    let x = X_SIZE as f32;
    let y = Y_SIZE as f32;

    f32x2::new(x, y)
  }
  pub fn write_pixel(&self, ix: (usize, usize),
                     px: Elem) {
    let ptr = unsafe {
      &mut *self.image
    };
    if let Some(dest) = ptr.get_mut(ix.1 * X_SIZE + ix.0) {
      *dest = px;
    }
  }
}

pub fn main() {
  env_logger::init();
  let ctxt = Context::new().expect("create context");

  let dev = HsaAmdGpuAccel::first_device(&ctxt)
    .expect("no device");

  let alloc = unsafe {
    dev.coarse_lap_node_alloc(0)
  };

  let workitems = dev
    .isa_info()
    .workgroup_max_size as usize;
  assert!(workitems >= WORKITEM_SIZE,
          "not enough workitems per workgroup for this example");
  println!("output size: {}x{}", X_SIZE, Y_SIZE);
  println!("using workgroup size of {}", WORKITEM_SIZE);

  let mut invoc = Invoc::new(&dev, work)
    .expect("Invoc::new");

  println!("allocating {} MB of host memory",
           X_SIZE * Y_SIZE * size_of::<Elem>() / 1024 / 1024);

  let mut frames = LapVec::new_in(alloc.clone());
  frames.resize(X_SIZE * Y_SIZE, Elem::default());
  let mut frames = frames.into_boxed_slice();
  frames.add_access(&dev).unwrap();

  invoc.workgroup_dims((WORKITEM_SIZE, WORKITEM_SIZE, ));
  invoc.grid_dims((X_SIZE, Y_SIZE, ));

  let kernel_signal = GlobalSignal::new(1).unwrap();

  let group_size = invoc.group_size().expect("codegen failure");
  let private_size = invoc.private_size().unwrap();

  let queue = dev.create_single_queue2(None, group_size, private_size)
    .expect("HsaAmdGpuAccel::create_single_queue");

  let args_pool = ArgsPool::new::<Args>(&dev, 1)
      .expect("ArgsPool::new");
  let args_pool = Rc::new(args_pool);

  let args = Args {
    image: &mut frames[..],
    scale: 1.0,
  };

  println!("dispatching...");
  let start = Instant::now();
  unsafe {
    let _wait = invoc
      .unchecked_call_async(args, &queue, kernel_signal, args_pool)
      .expect("Invoc::unchecked_call_async");
  }
  let elapsed = start.elapsed();
  println!("dispatch took {}ms", elapsed.as_millis());

  // write the output image to `out.png`:
  let out_filename = "out.png";
  let buffer: &[RGBA] = unsafe {
    ::std::slice::from_raw_parts(frames.as_ptr() as *const _,
                                 frames.len())
  };
  encode32_file(out_filename, buffer, X_SIZE, Y_SIZE)
    .expect("write output png");
}
