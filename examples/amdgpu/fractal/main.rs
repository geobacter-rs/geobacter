

extern crate grt_amd as geobacter_runtime_amd;

use std::mem::{size_of, };
use std::ops::*;
use std::time::Instant;

use grt_amd::{*, alloc::*, module::*, signal::*, };

use packed_simd::*;

use lodepng::{encode32_file, RGBA, };

pub type Elem = u8x4;
const X_SIZE: usize = 1024 * 4;
const Y_SIZE: usize = X_SIZE;

const WORKITEM_SIZE: usize = 16;

#[derive(GeobacterDeps)]
pub struct Args {
  image: *mut [Elem],
  queue: DeviceSingleQueue,
  completion: GlobalSignal,
}
impl Completion for Args {
  type CompletionSignal = GlobalSignal;
  fn completion(&self) -> &GlobalSignal { &self.completion }
}
impl Kernel for Args {
  type Grid = Dim2D<RangeTo<u32>>;
  const WORKGROUP: <Self::Grid as GridDims>::Workgroup = Dim2D {
    x: ..WORKITEM_SIZE as _,
    y: ..WORKITEM_SIZE as _,
  };
  type Queue = DeviceSingleQueue;

  fn queue(&self) -> &Self::Queue {
    &self.queue
  }

  /// This is the kernel that is run on the GPU
  fn kernel(&self, vp: KVectorParams<Self>) where Self: Sized {
    let grid = vp.grid_id();
    let id = f64x2::new(grid.x as f64, grid.y as f64);
    let norm = (id + 0.5f64) / self.image_size();
    let c = (norm - 0.5f64) * 2.0f64 - f64x2::new(1.0f64, 0.0);

    let c = (c.extract(0), c.extract(1));

    let mut z = (0.0f64, 0.0f64);
    let mut i = 0.0f64;

    loop {
      z = (
        z.0 * z.0 - z.1 * z.1 + c.0,
        2.0 * z.0 * z.1 + c.1,
      );

      if (z.0 * z.0 + z.1 * z.1) > 4.0 {
        break;
      }

      i += 0.005;
      if i >= 1.0 { break; }
    }

    let mut write = f64x4::new(i, i, i, 1.0f64);
    write *= u8::max_value() as f64;

    self.write_pixel((grid.x as _, grid.y as _), write.cast());
  }
}
unsafe impl Send for Args { }
unsafe impl Sync for Args { }

impl Args {
  pub fn image_size(&self) -> f64x2 {
    let x = X_SIZE as f64;
    let y = Y_SIZE as f64;

    f64x2::new(x, y)
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

  let alloc = dev.fine_lap_node_alloc(0);

  let workitems = dev
    .isa_info()
    .workgroup_max_size as usize;
  assert!(workitems >= WORKITEM_SIZE,
          "not enough workitems per workgroup for this example");
  println!("output size: {}x{}", X_SIZE, Y_SIZE);
  println!("using workgroup size of {}", WORKITEM_SIZE);

  let mut invoc = FuncModule::<Args>::new(&dev);

  println!("allocating {} MB of host memory",
           X_SIZE * Y_SIZE * size_of::<Elem>() / 1024 / 1024);

  let mut frames = LapVec::new_in(alloc.clone());
  frames.resize(X_SIZE * Y_SIZE, Elem::default());
  let mut frames = frames.into_boxed_slice();
  frames.add_access(&dev).unwrap();

  let grid = Dim2D {
    x: ..X_SIZE as _,
    y: ..Y_SIZE as _,
  };

  let kernel_signal = GlobalSignal::new(1).unwrap();

  let group_size = invoc.group_size().expect("codegen failure");
  let private_size = invoc.private_size().unwrap();

  let queue = dev.create_single_queue2(None, group_size, private_size)
    .expect("HsaAmdGpuAccel::create_single_queue");

  let args_pool = ArgsPool::new::<Args>(&dev, 1)
      .expect("ArgsPool::new");

  invoc.compile().expect("codegen failed");

  let mut invoc = invoc.into_invoc(&args_pool);

  let args = Args {
    image: &mut frames[..],
    queue,
    completion: kernel_signal,
  };

  println!("dispatching...");
  let start = Instant::now();
  unsafe {
    let _wait = invoc
      .unchecked_call_async(&grid, args)
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
