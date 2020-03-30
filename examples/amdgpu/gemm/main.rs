
// TODO: make the Geobacter attributes "known" to rustc.
#![feature(register_attr)]
#![register_attr(geobacter_attr)]

extern crate grt_amd as geobacter_runtime_amd;

use gstd_amd::*;
use grt_amd::prelude::*;

use num_traits::AsPrimitive;

use rand::distributions::Uniform;
use rand::prelude::*;

use std::fmt;
use std::marker::PhantomData;
use std::mem::{size_of, drop, };
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
    x: ..BLOCK_K as _,
    y: ..BLOCK_K as _,
  };
  type Queue = DeviceSingleQueue;

  fn queue(&self) -> &Self::Queue {
    &self.queue
  }

  /// This function is run on the GPU.
  fn kernel(&self, vp: KVectorParams<Self>) {
    struct GpuWorkItem<'a, 'b>(&'b mut LdsShared<'a, LdsArray<ETy>>,
                               &'b mut LdsShared<'a, LdsArray<ETy>>,
                               ETy)
      where 'a: 'b;
    impl<'b, 'c> AllWorkItems<ETy> for GpuWorkItem<'b, 'c>
      where 'b: 'c,
    {
      #[inline(always)]
      fn with<F, G>(&mut self,
                    vp: &VectorParams<Dim2D<Range<u32>>>,
                    mut init: F, mut with: G)
        where F: for<'a> FnMut(&'a VectorParams<Dim2D<Range<u32>>>) -> (ETy, ETy),
              G: for<'a> FnMut(&'a VectorParams<Dim2D<Range<u32>>>,
                &'a mut ETy, &'a LdsArray<ETy>, &'a LdsArray<ETy>),
      {
        let (a, b) = init(vp);
        let s_a = self.0.init(vp, a);
        let s_b = self.1.init(vp, b);

        with(vp, &mut self.2, &s_a, unsafe { s_b.unchecked_ref() });

        drop(s_a);
        unsafe { s_b.unsynced_drop(); } // avoid the second, unneeded, barrier.
      }
      #[inline(always)]
      fn write<F>(&mut self,
                  vp: &VectorParams<Dim2D<Range<u32>>>,
                  mut f: F)
        where F: for<'a> FnMut(&'a VectorParams<Dim2D<Range<u32>>>, &'a ETy),
      {
        f(vp, &mut self.2)
      }
    }

    let dim = dim_spec_param();

    // These globals are in LDS (workgroup local) memory.
    // XXX this function can't be generic over ETy *only* because Rust prohibits it.
    // statics aren't allowed to close over generic parameters of the parent function.
    lds! {
      let mut lds_sa: Lds<LdsArray<ETy>> = Lds::new();
      let mut lds_sb: Lds<LdsArray<ETy>> = Lds::new();
    }

    lds_sa.with_shared(|mut sa| {
      // TODO: nonnull assumes in LLVM cause lds_sb to not get optimized
      //       off the stack, slowing our kernel down by 200+Gops.
      // Fixing SROA to split the @llvm.assume(icmp ne null) actually causes way
      // more bugs than you'd expect; such a patch is still a WIP.
      lds_sb.with_shared(|mut sb| {
        let lds = GpuWorkItem(&mut sa, &mut sb, 0 as ETy);

        let a = self.a.dst();
        let b = self.b.dst();

        unsafe {
          gemm_v1(&vp, a.as_ref(), b.as_ref(),
                  self.c, dim, lds);
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
  assert!(!platform::is_host());
  param::get_spec_param(&dim_spec_param)
    .cloned()
    .unwrap()
}
/// Do we need to do bounds checks, because BLOCK_K doesn't divide the grid evenly?
fn mod_block_k() -> bool {
  param::get_spec_param(&mod_block_k)
    .cloned()
    .unwrap_or_default()
}

const BLOCK_K: usize = 22;
// If BLOCK_K was a power of two, this would equal to it + 1.
// We don't need the extra row now because 22 seems to be a sweet spot on all the
// cards benched.
const BLOCK_K_STRIDE: usize = BLOCK_K;
type ETy = f32;
type LdsArray<E> = [[E; BLOCK_K_STRIDE]; BLOCK_K];

/// This trait abstracts running code over all workitems. Used by the kernels
/// so that the GEMM can also be ran on a normal CPU.
trait AllWorkItems<E> {
  fn with<F, G>(&mut self,
                vp: &VectorParams<Dim2D<Range<u32>>>,
                init: F, with: G)
    where F: for<'a> FnMut(&'a VectorParams<Dim2D<Range<u32>>>) -> (E, E),
          G: for<'a> FnMut(&'a VectorParams<Dim2D<Range<u32>>>,
            &'a mut E, &'a LdsArray<E>, &'a LdsArray<E>);

  fn write<F>(&mut self,
              vp: &VectorParams<Dim2D<Range<u32>>>,
              f: F)
    where F: for<'a> FnMut(&'a VectorParams<Dim2D<Range<u32>>>, &'a E);
}

fn gemm_v1<F, E>(vp: &VectorParams<Dim2D<Range<u32>>>,
                 a: &[E], b: &[E], c: *mut [E],
                 stride: NonZeroUsize, mut lds: F)
  where F: AllWorkItems<E>,
        E: Copy + AddAssign + Mul<Output = E> + PartialEq + fmt::Debug + 'static,
        u32: AsPrimitive<E>,
{
  let stride = stride.get();
  let mod_k = mod_block_k();

  let mut k = 0usize;
  while k < stride {
    // init SMEM:
    lds.with(vp, |vp| {
      let Dim2D { x: wi_x, y: wi_y, } = vp.wi().as_::<u32>();
      let Dim2D { x: wg_x, y: wg_y, } = vp.wg_idx();

      let ao_y = (wg_y + wi_y) as usize;
      let ao_x = k + wi_x as usize;

      let bo_y = k + wi_y as usize;
      let bo_x = (wg_x + wi_x) as usize;

      let ao = ao_y * stride + ao_x;
      let bo = bo_y * stride + bo_x;

      let a = if mod_k || (ao_y < stride && ao_x < stride) {
        a[ao]
      } else {
        0u32.as_()
      };
      let b = if mod_k || (bo_y < stride && bo_x < stride) {
        b[bo]
      } else {
        0u32.as_()
      };
      (a, b)
    }, |vp, acc, sa, sb| {
      let Dim2D { x: wi_x, y: wi_y, } = vp.wi();
      let mut kci = 0u16;
      while kci < (BLOCK_K as u16) {
        {
          let kci = kci as u32;
          let va = sa[kci as usize][wi_y as usize];
          let vb = sb[wi_x as usize][kci as usize];
          *acc += va * vb;
        }
        kci += 1;
      }
    });
    k += BLOCK_K;
  }

  // copy sc back to C:
  lds.write(vp, |vp, acc| {
    let Dim2D { x: wi_x, y: wi_y, } = vp.wi().as_::<u32>();
    let Dim2D { x: wg_x, y: wg_y, } = vp.wg_idx();

    let i_y = (wg_y + wi_y) as usize;
    let i_x = (wg_x + wi_x) as usize;
    let idx = i_y * stride + i_x;
    if mod_k || (i_y < stride && i_x < stride) {
      unsafe { (&mut *c)[idx] = *acc; }
    }
  });
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
  println!("BLOCK_K = {}", BLOCK_K);

  env_logger::init();
  let ctxt = Context::new().expect("create context");

  let dev = HsaAmdGpuAccel::first_device(&ctxt)
    .expect("no device");

  const AXIS_SIZE_: usize = 4 * 4096 + 1024;
  const AXIS_SIZE: usize = ((AXIS_SIZE_ - 1) / BLOCK_K + 1) * BLOCK_K;
  const SIZE: usize = AXIS_SIZE * AXIS_SIZE;
  const GRID: usize = AXIS_SIZE;

  let shape = (AXIS_SIZE, AXIS_SIZE);
  let dim = NonZeroUsize::new(AXIS_SIZE).unwrap();
  let hardness = (2 * AXIS_SIZE * AXIS_SIZE * AXIS_SIZE) as f64;

  let mut invoc = GemmArgs::module(&dev);
  invoc.define_param(dim_spec_param, &dim);
  invoc.define_param(mod_block_k, &(GRID % BLOCK_K == 0));
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
