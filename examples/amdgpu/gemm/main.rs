
#![feature(geobacter)]

// TODO: make the Geobacter attributes "known" to rustc.
#![feature(register_attr)]
#![register_attr(geobacter_attr)]

extern crate grt_amd as geobacter_runtime_amd;

use grt_amd::prelude::*;
use grt_amd::texture::{access::*, geometry::*};

use num_traits::AsPrimitive;

use rand::distributions::Uniform;
use rand::prelude::*;

use std::geobacter::platform::*;
use std::geobacter::spec_param as param;
use std::mem::{size_of, drop};
use std::num::NonZeroU32;
use std::ops::*;
use std::rc::Rc;
use std::time::*;

type ImageRef<'a, A> = grt_amd::texture::ImageRef<'a,
  A,
  Format<ETy, channel_order::R>,
  TwoD<u32>,
  layout::Opaque,
>;
type LinearImageRef<'a, A> = grt_amd::texture::ImageRef<'a,
  A,
  Format<ETy, channel_order::R>,
  TwoD<u32>,
  layout::Opaque,
>;

/// All row major.
#[derive(GeobacterDeps)]
struct GemmArgs<'b> {
  a: ImageRef<'b, ReadOnly>,
  b: ImageRef<'b, ReadOnly>,
  c: LinearImageRef<'b, WriteOnly>,
  queue: DeviceSingleQueue,
  completion: GlobalSignal,
}

impl<'b> Completion for GemmArgs<'b> {
  type CompletionSignal = GlobalSignal;
  #[inline(always)] fn completion(&self) -> &GlobalSignal { &self.completion }
}
impl<'b> Kernel for GemmArgs<'b> {
  type Grid = Dim2D<Range<u32>>;
  const WORKGROUP: <Self::Grid as GridDims>::Workgroup = Dim2D {
    x: ..TILE_S as _,
    y: ..RTS as _,
  };
  type Queue = DeviceSingleQueue;

  fn queue(&self) -> &Self::Queue {
    &self.queue
  }

  /// This function is run on the GPU.
  fn kernel(&self, vp: KVectorParams<Self>) {
    use std::geobacter::amdgpu::workitem::ReadFirstLane;

    let dim = dim_spec_param();

    // Our inputs are the same for every workitem in the dispatch.
    // Not doing this means the resource descriptors are loaded into SGPRs right before
    // every image load. Doing this here grants us just over 300G-ops on a Radeon VII.
    let a = unsafe { self.a.read_first_lane() };
    let b = unsafe { self.b.read_first_lane() };
    let c = unsafe { self.c.read_first_lane() };

    // These globals are in LDS (workgroup local) memory.
    // XXX this function can't be generic over ETy *only* because Rust prohibits it.
    // statics aren't allowed to close over generic parameters of the parent function.
    lds! {
      let mut lds_sa: Lds<LdsArray<ETy>> = Lds::new();
      let mut lds_sb: Lds<LdsArray<ETy>> = Lds::new();
    }

    lds_sa.with_shared(|mut sa| {
      lds_sb.with_shared(|mut sb| {
        gemm_v1(vp, a, b, c,
                &mut sa, &mut sb, dim);
      });
    });
  }
}
unsafe impl<'b> Send for GemmArgs<'b> { }
unsafe impl<'b> Sync for GemmArgs<'b> { }

/// Get the dimension specialization parameter. This should only be called on
/// the device.
fn dim_spec_param() -> NonZeroU32 {
  assert!(!platform().is_host());
  *param::get(&dim_spec_param).unwrap()
}
/// Do we need to do bounds checks, because TILE_S doesn't divide the grid evenly?
fn mod_block_k() -> bool {
  if let Some(&v) = param::get(&mod_block_k) {
    v
  } else {
    false
  }
}

const WPT: usize = 1;
const RTS: usize = TILE_S / WPT;

const TILE_S: usize = 32;
type ETy = f32;
type LdsArray<E> = [[[E; WPT]; RTS + 1]; TILE_S];
type LdsTile<E> = [[E; TILE_S + WPT]; TILE_S];

/// LdsArray and LdsTile *MUST* have the same size. This function will cause a compile
/// error if they don't match.
#[allow(dead_code)]
fn assert_lds_size() -> LdsTile<ETy> {
  let v: LdsArray<ETy> = [[[0.0f32; WPT]; RTS + 1]; TILE_S];
  unsafe {
    ::std::mem::transmute(v)
  }
}

fn gemm_v1<A1, A2>(vp: VectorParams<Dim2D<Range<u32>>>,
                   a: ImageRef<A1>, b: ImageRef<A1>,
                   c: LinearImageRef<A2>,
                   sa: &mut LdsShared<LdsArray<ETy>>,
                   sb: &mut LdsShared<LdsArray<ETy>>,
                   stride: NonZeroU32)
  where A1: access::ReadAccess,
        A2: access::WriteAccess,
{
  let stride = stride.get();
  let mod_k = mod_block_k();

  let wpt = WPT as u32;

  let mut acc = [0u32.as_(); WPT];

  let wg_size = Dim2D::from(TILE_S as u32);
  let wg = vp.wg_id();

  let Dim2D { x: wi_x, y: wi_y, } = (vp.wi() * Dim2D {
    x: 1,
    y: wpt as u16,
  }).as_::<u32>();
  let Dim2D { x: wg_x, y: wg_y, } = wg * wg_size;

  let g_x = wg_x + wi_x;
  let g_y = wg_y + wi_y;

  let mut k = 0;
  while k < stride {
    // init SMEM:
    let mut bt = [0u32.as_(); WPT];
    let mut at = [0u32.as_(); WPT];

    let mut w = 0u32;
    while w < wpt {
      let ao_y = g_y + w;
      let ao_x = k + wi_x;

      let bo_y = k + wi_y + w;
      let bo_x = g_x;

      at[w as usize] = a.load((ao_x, ao_y));
      bt[w as usize] = b.load((bo_x, bo_y));

      w += 1;
    }

    k += TILE_S as u32;

    let sa = sa.init(&vp, at);
    let sb = sb.init(&vp, bt);

    unsafe {
      let sa = &*(sa.as_ptr() as *const _ as *const LdsTile<ETy>);
      let sb = sb.unchecked_ref();

      let mut kci = 0u16;
      while kci < (TILE_S as u16) {
        let va = sa[wi_y as usize][kci as usize];
        let vbs = sb[kci as usize][vp.wi().x as usize];

        let mut w = 0u32;
        while w < wpt {
          acc[w as usize] += va * vbs[w as usize];
          w += 1;
        }

        kci += 1;
      }
    }

    drop(sa);
    unsafe { sb.unsynced_drop() }
  }

  // copy sc back to C:
  let mut w = 0u32;
  while w < wpt {
    let i_y = g_y + w;
    let i_x = g_x;
    if mod_k || (i_y < stride && i_x < stride) {
      c.store((i_x, i_y), [acc[w as usize]; 1]);
    }

    w += 1;
  }
}

pub fn time<F, R>(what: &str, f: F) -> R
  where F: FnOnce() -> R,
{
  let start = Instant::now();
  let r = f();
  let elapsed = start.elapsed();

  let nanos = elapsed.as_nanos();
  let micros = elapsed.as_micros();
  let ms = elapsed.as_millis();
  let secs = elapsed.as_secs();

  let big;
  let small;

  if ms <= 1 {
    big = (micros, "μs");
    small = (nanos, "ns");
  } else if ms > 1 && secs < 1 {
    big = (ms, "ms");
    small = (micros, "μs");
  } else {
    big = (secs as _, "s");
    small = (ms, "ms");
  }

  println!("{} took {}{} ({}{})", what,
           big.0, big.1, small.0, small.1);

  r
}
pub fn bench<F, R>(what: &str, hardness: f64, f: F) -> R
  where F: FnOnce() -> R,
{
  let start = Instant::now();
  let r = f();
  let elapsed = start.elapsed();

  let nanos = elapsed.as_nanos();
  let micros = elapsed.as_micros();
  let ms = elapsed.as_millis();
  let secs = elapsed.as_secs();

  let big;
  let small;

  if ms <= 1 {
    big = (micros, "μs");
    small = (nanos, "ns");
  } else if ms > 1 && secs < 1 {
    big = (ms, "ms");
    small = (micros, "μs");
  } else {
    big = (secs as _, "s");
    small = (ms, "ms");
  }

  println!("{} took {}{} ({}{})", what,
           big.0, big.1, small.0, small.1);

  let time = elapsed.as_secs_f64();
  let mut scale = "k";
  let mut ops = (hardness / time) / 1000.0;
  if ops >= 1000.0 {
    ops /= 1000.0;
    scale = "M";
  }
  if ops >= 1000.0 {
    ops /= 1000.0;
    scale = "G";
  }
  if ops >= 1000.0 {
    ops /= 1000.0;
    scale = "T";
  }

  println!("{} {}-ops: {}", what, scale, ops);

  r
}

pub fn main() {
  println!("TILE_S = {}, WPT = {}", TILE_S, WPT);

  env_logger::init();
  let ctxt = Context::new().expect("create context");

  let dev = HsaAmdGpuAccel::nth_device(&ctxt, 0)
    .expect("no device");

  // XXX this is the maximum image size along a single side: 14bits, the hw limit as of GFX9.
  const AXIS_SIZE_: usize = 4 * 4096;
  const AXIS_SIZE: usize = ((AXIS_SIZE_ - 1) / TILE_S) * TILE_S;
  const SIZE: usize = AXIS_SIZE * AXIS_SIZE;
  const GRID: usize = AXIS_SIZE;

  let shape = (AXIS_SIZE, AXIS_SIZE);
  let dim = NonZeroU32::new(AXIS_SIZE as u32).unwrap();
  let hardness = (2 * AXIS_SIZE * AXIS_SIZE * AXIS_SIZE) as f64;

  let mut invoc = GemmArgs::module(&dev);
  invoc.define_param(dim_spec_param, &dim);
  invoc.define_param(mod_block_k, &(GRID % TILE_S == 0));
  invoc.compile_async();

  let alloc = dev.fine_lap_node_alloc(0);

  println!("{}mb on host", (3 * SIZE * size_of::<ETy>()) / 1024 / 1024);
  println!("{}mb on device", (2 * SIZE * size_of::<ETy>()) / 1024 / 1024);

  let mut la = LapVec::with_capacity_in(SIZE, alloc.clone());
  let mut lb = LapVec::with_capacity_in(SIZE, alloc.clone());
  let mut lc: LapVec<ETy> =
    LapVec::with_capacity_in(SIZE, alloc.clone()); // for verification
  let mut nd_lc = LapVec::with_capacity_in(SIZE, alloc.clone());

  la.resize(SIZE, 0u32.as_());
  lb.resize(SIZE, 0u32.as_());
  lc.resize(SIZE, 0u32.as_());
  nd_lc.resize(SIZE, 0u32.as_());

  let setup_memory = |b: &mut LapVec<_>| {
    use nix::sys::mman::*;

    b.add_access(&dev).expect("grant GPU access to host memory");

    unsafe {
      let b_region = b.pool_ptr().unwrap();
      let r = madvise(b_region.as_ptr().as_ptr() as _,
                      b_region.len() as _,
                      MmapAdvise::MADV_HUGEPAGE);
      if let Err(err) = r {
        eprintln!("failed to madvise for hugepages: {}", err);
      }
    }
  };
  setup_memory(&mut la);
  setup_memory(&mut lb);
  setup_memory(&mut lc);
  setup_memory(&mut nd_lc);

  let mut nd_lc = nd_lc.into_boxed_slice();

  let mut rng = SmallRng::seed_from_u64(1);
  let dist = Uniform::new(0u32 as ETy, 1u32 as ETy);
  let mut rng_mat = |l: &mut LapVec<_>| {
    let mut l = nd::aview_mut1(&mut l[..]).into_shape(shape).unwrap();
    for mut l in l.axis_iter_mut(nd::Axis(0)) {
      for l in l.iter_mut() {
        *l = dist.sample(&mut rng);
      }
    }
  };
  rng_mat(&mut la);
  rng_mat(&mut lb);

  let la = la.into_boxed_slice();
  let lb = lb.into_boxed_slice();
  let mut lc = lc.into_boxed_slice();

  {
    // ndarray has nice pretty printing:
    let a = nd::aview1(&la[..]).into_shape(shape).unwrap();
    let b = nd::aview1(&lb[..]).into_shape(shape).unwrap();
    println!("A = {:?}", a);
    println!("B = {:?}", b);
  }

  let geometry = geometry::TwoD {
    width: GRID as u32,
    height: GRID as u32,
  };
  let mut a_img = dev
    .create_ro_texture(geometry)
    .expect("create a_img");
  let mut b_img = dev
    .create_ro_texture(geometry)
    .expect("create b_img");

  a_img.import_all_packed(&la)
    .expect("a_img import");
  b_img.import_all_packed(&lb)
    .expect("b_img import");

  let c_img = dev
    .create_wo_texture(geometry)
    .expect("create c_img");

  let kernel_signal = GlobalSignal::new(1).unwrap();

  let args_pool = time("alloc args pool", || {
    ArgsPool::new::<GemmArgs>(&dev, 1)
      .expect("ArgsPool::new")
  });
  let args_pool = Rc::new(args_pool);

  let group_size = invoc.group_size().expect("codegen failure");
  let private_size = invoc.private_size().unwrap();

  let queue = dev.create_single_queue2(None, group_size, private_size)
    .expect("HsaAmdGpuAccel::create_single_queue");

  // ensure the invocation doesn't block on this step:
  invoc.compile().expect("kernel cross codegen");

  let mut invoc = invoc.into_invoc(args_pool);

  println!("starting GPU gemm...");

  bench("gpu gemm", hardness, || {
    let args = GemmArgs {
      a: a_img.as_ref(),
      b: b_img.as_ref(),
      c: c_img.as_ref(),
      queue,
      completion: kernel_signal,
    };
    let grid = Dim2D {
      x: 0..GRID as u32,
      y: 0..GRID as u32,
    };
    let _wait = unsafe {
      invoc.unchecked_call_async(&grid, args)
        .expect("Invoc::call_async")
    };
  });

  drop(a_img);
  drop(b_img);

  c_img.export_all_packed(&mut lc)
    .expect("export gemm results");
  drop(c_img);

  time("nd linalg gemm", || {
    let a = nd::aview1(&la[..]).into_shape(shape).unwrap();
    let b = nd::aview1(&lb[..]).into_shape(shape).unwrap();
    let mut c = nd::aview_mut1(&mut nd_lc[..]).into_shape(shape).unwrap();

    // compute using host and check against the GPU's results:
    nd::linalg::general_mat_mul(1.0 as ETy, &a, &b,
                                0.0 as ETy, &mut c);

    let lc = nd::aview1(&lc[..]).into_shape(shape).unwrap();
    approx::assert_relative_eq!(c, lc, epsilon = 500000.0 * std::f32::EPSILON);
  });
}
