
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
use std::num::NonZeroU16;
use std::ops::*;
use std::slice::{from_raw_parts, from_raw_parts_mut};
use std::time::*;

type ImageRef<'a, A> = grt_amd::texture::ImageRef<'a,
  A,
  Format<ETy, channel_order::RGBA>,
  TwoD<u32>,
  layout::Opaque,
>;

/// All row major.
#[derive(GeobacterDeps)]
struct GemmArgs<'b> {
  a: ImageRef<'b, ReadOnly>,
  b: ImageRef<'b, ReadOnly>,
  c: ImageRef<'b, WriteOnly>,
  queue: DeviceMultiQueue,
  completion: GlobalSignal,
}

impl<'b> Completion for GemmArgs<'b> {
  type CompletionSignal = GlobalSignal;
  #[inline(always)] fn completion(&self) -> &GlobalSignal { &self.completion }
}
impl<'b> Kernel for GemmArgs<'b> {
  type Grid = Dim2D<Range<u32>>;
  const WORKGROUP: <Self::Grid as GridDims>::Workgroup = Dim2D {
    x: ..RTS as _,
    y: ..TILE_S as _,
  };
  type Queue = DeviceMultiQueue;

  fn queue(&self) -> &Self::Queue { &self.queue }

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
    // Statics aren't allowed to close over generic parameters of the parent function.
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

/// Get the dimension specialization parameter. This should only be called on
/// the device.
fn dim_spec_param() -> NonZeroU16 {
  debug_assert!(!platform().is_host());
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

const WPT: usize = 4;
const RTS: usize = TILE_S / WPT;

const TILE_S: usize = 32;
type ETy = f32;
// types which allow us to block a whole row at a time during the naive gemm.
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
                   c: ImageRef<A2>,
                   sa: &mut LdsShared<LdsArray<ETy>>,
                   sb: &mut LdsShared<LdsArray<ETy>>,
                   stride: NonZeroU16)
  where A1: access::ReadAccess,
        A2: access::WriteAccess,
{
  let stride = stride.get();
  let mod_k = mod_block_k();

  let wpt = WPT as u16;

  let mut acc = [0u32.as_(); WPT];

  let wi = vp.wi();
  let wg_id = vp.wg_id().as_::<u16>();

  let g_x = wg_id.x * (RTS as u16) + wi.x;
  let g_y = wg_id.y * (TILE_S as u16) + wi.y;

  let mut k = 0u16;
  while k < stride {
    // init SMEM:
    let ao_y = g_y;
    let ao_x = k / wpt + wi.x;

    let bo_y = k + wi.y;
    let bo_x = g_x;

    let mut at: [ETy; WPT] = [0u32.as_(); WPT];
    if mod_k || (ao_y < stride && ao_x * wpt < stride) {
      at = a.load((ao_x as _, ao_y as _));
    }
    let mut bt: [ETy; WPT] = [0u32.as_(); WPT];
    if mod_k || (bo_y < stride && bo_x * wpt < stride) {
      bt = b.load((bo_x as _, bo_y as _));
    }

    let sa = sa.init(&vp, at);
    let sb = sb.init(&vp, bt);

    {
      let (sa, sb) = unsafe {
        let sa = &*(sa.as_ptr() as *const _ as *const LdsTile<ETy>);
        // avoid the second barrier:
        let sb = sb.unchecked_ref();
        (sa, sb)
      };

      let mut kci = 0u16;
      while kci < (TILE_S as u16) {
        let va = sa[wi.y as usize][kci as usize];
        let vbs = &sb[kci as usize][wi.x as usize];

        let mut w = 0u16;
        while w < wpt {
          acc[w as usize] = va
            .mul_add(vbs[w as usize], acc[w as usize]);
          w += 1;
        }

        kci += 1;
      }
    }

    drop(sa);
    // avoid the second barrier:
    unsafe { sb.unsynced_drop() }

    k += TILE_S as u16;
  }

  // Write output:
  if mod_k || (g_y < stride && g_x * wpt < stride) {
     c.store((g_x as _, g_y as _), acc);
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

  let ctxt = Context::new().expect("create context");

  let dev = HsaAmdGpuAccel::nth_device(&ctxt, 0)
    .expect("no device");

  // This is the maximum image size along a single side: 14bits, the hw limit as of GFX9.
  const AXIS_SIZE_: usize = 4 * 4096;
  const AXIS_SIZE: usize = ((AXIS_SIZE_ - 1) / TILE_S + 1) * TILE_S;
  const SIZE: usize = AXIS_SIZE * AXIS_SIZE;
  const GRID: usize = AXIS_SIZE;

  let shape = (AXIS_SIZE, AXIS_SIZE);
  let dim = NonZeroU16::new(AXIS_SIZE as u16).unwrap();
  let hardness = (2 * AXIS_SIZE * AXIS_SIZE * AXIS_SIZE) as f64;

  let mut invoc = GemmArgs::module(&dev);
  invoc.define_param(dim_spec_param, &dim);
  invoc.define_param(mod_block_k, &(GRID % TILE_S == 0));
  invoc.compile_async();

  let alloc = dev.fine_lap_node_alloc(0);

  println!("{}mb on host", (3 * SIZE * size_of::<ETy>()) / 1024 / 1024);
  println!("{}mb on device", (3 * SIZE * size_of::<ETy>()) / 1024 / 1024);

  let mut la = LapVec::with_capacity_in(SIZE, alloc.clone());
  let mut lb = LapVec::with_capacity_in(SIZE, alloc.clone());
  let mut lc: LapVec<ETy> =
    LapVec::with_capacity_in(SIZE, alloc.clone());
  let mut nd_lc = LapVec::with_capacity_in(SIZE, alloc.clone()); // for verification

  la.resize(SIZE, 0u32.as_());
  lb.resize(SIZE, 0u32.as_());
  lc.resize(SIZE, 0u32.as_());
  nd_lc.resize(SIZE, 0u32.as_());

  let setup_memory = |b: &mut LapVec<_>| {
    b.add_access(&dev).expect("grant GPU access to host memory");
  };
  setup_memory(&mut la);
  setup_memory(&mut lb);
  setup_memory(&mut lc);
  setup_memory(&mut nd_lc);

  let mut nd_lc = nd_lc.into_boxed_slice();

  let mut rng = SmallRng::seed_from_u64(1);
  let dist = Uniform::new(-1.0 as ETy, 1.0 as ETy);
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
    width: GRID as u32 / 4,
    height: GRID as u32,
  };
  let mut a_img = dev
    .create_ro_texture(geometry)
    .expect("create a_img");
  let mut b_img = dev
    .create_ro_texture(geometry)
    .expect("create b_img");

  a_img.import_all_packed(unsafe {
    from_raw_parts(la.as_ptr() as *const [f32; 4], la.len() / 4)
  })
    .expect("a_img import");
  b_img.import_all_packed(unsafe {
    from_raw_parts(lb.as_ptr() as *const [f32; 4], lb.len() / 4)
  })
    .expect("b_img import");

  let c_img = dev
    .create_wo_texture(geometry)
    .expect("create c_img");

  let kernel_signal = GlobalSignal::new(1).unwrap();

  let args_pool = time("alloc args pool", || {
    ArgsPool::new::<GemmArgs>(&dev, 1)
      .expect("ArgsPool::new")
  });

  let group_size = invoc.group_size().expect("codegen failure");
  let private_size = invoc.private_size().unwrap();

  let queue = dev.create_multi_queue2(None, private_size, group_size)
    .expect("HsaAmdGpuAccel::create_single_queue");

  // ensure the invocation doesn't block on this step:
  invoc.compile().expect("kernel cross codegen");

  let mut invoc = invoc.into_invoc(&args_pool);

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
      x: 0..GRID as u32 / 4,
      y: 0..GRID as u32,
    };
    let _wait = unsafe {
      invoc.unchecked_call_async(&grid, args)
        .expect("Invoc::call_async")
    };
  });

  drop(a_img);
  drop(b_img);

  c_img.export_all_packed(unsafe {
    from_raw_parts_mut(lc.as_mut_ptr() as *mut [f32; 4], lc.len() / 4)
  })
    .expect("export gemm results");
  drop(c_img);

  println!("got GPU results; beginning CPU verification...");

  let a = nd::aview1(&la[..]).into_shape(shape).unwrap();
  let b = nd::aview1(&lb[..]).into_shape(shape).unwrap();
  let mut c = nd::aview_mut1(&mut nd_lc[..]).into_shape(shape).unwrap();

  bench("nd linalg gemm", hardness, || {
    // compute using host and check against the GPU's results:
    nd::linalg::general_mat_mul(1.0 as ETy, &a, &b,
                                0.0 as ETy, &mut c);
  });
  let lc = nd::aview1(&lc[..]).into_shape(shape).unwrap();
  approx::assert_relative_eq!(c, lc, epsilon = (AXIS_SIZE as f32) * std::f32::EPSILON);
}
