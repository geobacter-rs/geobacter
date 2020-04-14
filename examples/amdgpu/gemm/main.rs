
#![feature(geobacter)]

// TODO: make the Geobacter attributes "known" to rustc.
#![feature(register_attr)]
#![register_attr(geobacter_attr)]

extern crate grt_amd as geobacter_runtime_amd;

use grt_amd::prelude::*;

use num_traits::AsPrimitive;

use rand::distributions::Uniform;
use rand::prelude::*;

use std::fmt;
use std::geobacter::platform::*;
use std::geobacter::spec_param as param;
use std::marker::PhantomData;
use std::mem::size_of;
use std::num::NonZeroUsize;
use std::ops::*;
use std::rc::Rc;
use std::time::*;
use std::sync::Arc;

/// All row major.
#[derive(GeobacterDeps)]
struct GemmArgs<'a, 'b, E>
  where E: Copy + Deps,
{
  a: H2DGlobalRLapBoxMemTransfer<'a, [E], ()>,
  b: H2DGlobalRLapBoxMemTransfer<'a, [E], ()>,
  c: *mut [E],
  queue: DeviceSingleQueue,
  completion: GlobalSignal,
  _lt1: PhantomData<&'b mut E>,
}

impl<'a, 'b, E> Completion for GemmArgs<'a, 'b, E>
  where E: Copy + Deps,
{
  type CompletionSignal = GlobalSignal;
  #[inline(always)] fn completion(&self) -> &GlobalSignal { &self.completion }
}
impl<'a, 'b> Kernel for GemmArgs<'a, 'b, ETy> {
  type Grid = Dim2D<Range<u32>>;
  const WORKGROUP: <Self::Grid as GridDims>::Workgroup = Dim2D {
    x: ..RTS as _,
    y: ..TILE_S as _,
  };
  type Queue = DeviceSingleQueue;

  fn queue(&self) -> &Self::Queue {
    &self.queue
  }

  /// This function is run on the GPU.
  fn kernel(&self, vp: KVectorParams<Self>) {
    let dim = dim_spec_param();

    // These globals are in LDS (workgroup local) memory.
    // XXX this function can't be generic over ETy *only* because Rust prohibits it.
    // statics aren't allowed to close over generic parameters of the parent function.
    lds! {
      let mut lds_sa: Lds<LdsArray<ETy>> = Lds::new();
      let mut lds_sb: Lds<LdsArray<ETy>> = Lds::new();
    }

    lds_sa.with_shared(|mut sa| {
      lds_sb.with_shared(|mut sb| {
        let a = self.a.dst();
        let b = self.b.dst();

        unsafe {
          gemm_v1(&vp, a.as_ref(), b.as_ref(),
                  self.c, &mut sa, &mut sb, dim);
        }
      });
    });
  }
}
unsafe impl<'a, 'b, E> Send for GemmArgs<'a, 'b, E>
  where E: Copy + Deps + Send,
{ }
unsafe impl<'a, 'b, E> Sync for GemmArgs<'a, 'b, E>
  where E: Copy + Deps + Sync,
{ }

/// Get the dimension specialization parameter. This should only be called on
/// the device.
fn dim_spec_param() -> NonZeroUsize {
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

fn gemm_v1<E>(vp: &VectorParams<Dim2D<Range<u32>>>,
              a: &[E], b: &[E], c: *mut [E],
              sa: &mut LdsShared<LdsArray<E>>,
              sb: &mut LdsShared<LdsArray<E>>,
              stride: NonZeroUsize)
  where E: Copy + AddAssign + Mul<Output = E> + PartialEq + fmt::Debug + Sync + 'static,
        u32: AsPrimitive<E>,
{
  let stride = stride.get();
  let mod_k = mod_block_k();

  let wpt = WPT as u32;

  let mut acc = [0u32.as_(); WPT];

  let wg_size = Dim2D::from(TILE_S as u32);
  let wg = vp.wg_id();

  let Dim2D { x: wi_x, y: wi_y, } = (vp.wi() * Dim2D {
    x: wpt as u16,
    y: 1,
  }).as_::<u32>();
  let Dim2D { x: wg_x, y: wg_y, } = wg * wg_size;

  let mut k = 0usize;
  while k < stride {
    let mut at: [_; WPT] = [0u32.as_(); WPT];
    let mut bt: [_; WPT] = [0u32.as_(); WPT];
    // init SMEM:
    if !mod_k {
      let mut w = 0u32;
      while w < wpt {
        let ao_y = (wg_y + wi_y) as usize;
        let ao_x = k + (wi_x + w) as usize;

        let bo_y = k + wi_y as usize;
        let bo_x = (wg_x + wi_x + w) as usize;

        let ao = ao_y * stride + ao_x;
        let bo = bo_y * stride + bo_x;

        let a = if ao_y < stride && ao_x < stride {
          a[ao]
        } else {
          0u32.as_()
        };
        let b = if bo_y < stride && bo_x < stride {
          b[bo]
        } else {
          0u32.as_()
        };

        at[w as usize] = a;
        bt[w as usize] = b;

        w += 1;
      }
    } else {
      let ao_y = (wg_y + wi_y) as usize;
      let ao_x = k + wi_x as usize;

      let bo_y = k + wi_y as usize;
      let bo_x = (wg_x + wi_x) as usize;

      let ao = ao_y * stride + ao_x;
      let bo = bo_y * stride + bo_x;

      at = unsafe {
        *(a[ao..ao + WPT].as_ptr() as *const [E; WPT])
      };
      bt = unsafe {
        *(b[bo..bo + WPT].as_ptr() as *const [E; WPT])
      };
    }
    k += TILE_S;

    let sa = sa.init(&vp, at);
    let sb = sb.init(&vp, bt);

    let sa = unsafe { &*(sa.as_ptr() as *const _ as *const LdsTile<E>) };
    let sb = unsafe { &*(sb.as_ptr() as *const _ as *const LdsTile<E>) };

    let mut kci = 0u16;
    while kci < (TILE_S as u16) {
      let va = sa[wi_y as usize][kci as usize];

      let vbs = unsafe {
        let sb = &sb[kci as usize][wi_x as usize..(wi_x + wpt) as usize];
        *(sb.as_ptr() as *const [E; WPT])
      };

      let mut w = 0u32;
      while w < wpt {
        acc[w as usize] += va * vbs[w as usize];
        w += 1;
      }

      kci += 1;
    }
  }

  // copy sc back to C:
  if !mod_k {
    let mut w = 0u32;
    while w < wpt {
      let i_y = (wg_y + wi_y) as usize;
      let i_x = (wg_x + wi_x + w) as usize;
      let idx = i_y * stride + i_x;
      if mod_k || (i_y < stride && i_x < stride) {
        unsafe {
          (&mut *c)[idx] = acc[w as usize];
        }
      }

      w += 1;
    }
  } else {
    let i_y = (wg_y + wi_y) as usize;
    let i_x = (wg_x + wi_x) as usize;
    let idx = i_y * stride + i_x;
    unsafe {
      *((&mut *c)[idx..idx + WPT].as_mut_ptr() as *mut [E; WPT]) = acc;
    }
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

  const AXIS_SIZE_: usize = 4 * 4096 + 1024;
  const AXIS_SIZE: usize = ((AXIS_SIZE_ - 1) / TILE_S + 1) * TILE_S;
  const SIZE: usize = AXIS_SIZE * AXIS_SIZE;
  const GRID: usize = AXIS_SIZE;

  let shape = (AXIS_SIZE, AXIS_SIZE);
  let dim = NonZeroUsize::new(AXIS_SIZE).unwrap();
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
  let mut nd_lc = LapVec::with_capacity_in(SIZE, alloc);

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

  let mut async_copy_signal = Arc::new(GlobalSignal::new(0).unwrap());
  let kernel_signal = GlobalSignal::new(1).unwrap();

  println!("a: host ptr: 0x{:p}-0x{:p}",
           la.as_ptr(), unsafe { (la.as_ptr() as *const ETy).add(la.len()) },
  );
  println!("b: host ptr: 0x{:p}-0x{:p}",
           lb.as_ptr(), unsafe { (lb.as_ptr() as *const ETy).add(lb.len()) },
  );
  println!("c: host ptr: 0x{:p}-0x{:p}",
           lc.as_ptr(), unsafe { (lc.as_ptr() as *const ETy).add(lc.len()) },
  );

  let (da, db) = (&la, &lb)
    .memcopy(&dev, (), &mut async_copy_signal)
    .expect("HsaAmdGpuAccel::async_copy_into");

  println!("a: host ptr: 0x{:p}-0x{:p}, agent ptr: 0x{:p}-0x{:p}",
           la.as_ptr(), unsafe { (la.as_ptr() as *const ETy).add(la.len()) },
           da.dst().as_ptr(),
           unsafe {
             (da.dst().as_ptr() as *const ETy).add(da.dst().len())
           },
  );
  println!("b: host ptr: 0x{:p}-0x{:p}, agent ptr: 0x{:p}-0x{:p}",
           lb.as_ptr(), unsafe { (lb.as_ptr() as *const ETy).add(lb.len()) },
           db.dst().as_ptr(),
           unsafe {
             (db.dst().as_ptr() as *const ETy).add(db.dst().len())
           },
  );
  println!("c: host ptr: 0x{:p}-0x{:p}",
           lc.as_ptr(), unsafe { (lc.as_ptr() as *const ETy).add(lc.len()) },
  );

  let args_pool = time("alloc args pool", || {
    ArgsPool::new::<GemmArgs<ETy>>(&dev, 1)
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

  da.wait_for_zero(false).unwrap();
  // Don't need this second one, but w/e.
  db.wait_for_zero(false).unwrap();
  println!("starting GPU gemm...");

  bench("gpu gemm", hardness, || {
    let args = GemmArgs {
      a: da,
      b: db,
      c: &mut lc[..],
      queue,
      completion: kernel_signal,
      _lt1: PhantomData,
    };
    let grid = Dim2D {
      x: 0..GRID as u32,
      y: 0..GRID as u32,
    };
    let _wait = unsafe {
      invoc.unchecked_call_async(&grid, args)
        .expect("Invoc::call_async")
    };
    // no need to copy results; the GPU writes directly to visible RAM.
    // In fact, I've benched this; having the GPU write to RAM is faster than
    // writing to VRAM and then copying.
  });

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
