
// TODO: make the Geobacter attributes "known" to rustc.
#![feature(register_attr)]
#![register_attr(geobacter_attr)]

extern crate grt_amd as geobacter_runtime_amd;

use gstd_amd::*;
use grt_amd::{*, alloc::*, module::*, signal::*, mem::*, };

use num_traits::AsPrimitive;

use rand::distributions::Uniform;
use rand::prelude::*;

use std::fmt;
use std::marker::PhantomData;
use std::mem::{size_of, MaybeUninit, };
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
    #![allow(unused_attributes)] // geobacter_attr is actually not unused. TODO

    #[derive(Clone, Copy, Debug)]
    struct GpuWorkItem(u16, u16);
    impl AllWorkItems for GpuWorkItem {
      #[inline(always)]
      fn forall_workitems<F>(&self, mut f: F)
        where F: FnMut(/*wi_x:*/ u32, /*wi_y:*/ u32),
      {
        f(self.0 as _, self.1 as _);
      }
      #[inline(always)]
      fn barrier(&self) {
        use gstd_amd::sync::atomic::*;
        work_group_rel_acq_barrier(Scope::WorkGroup);
      }
    }

    let wg = (vp.wg_idx().x, vp.wg_idx().y);
    let wi = vp.wi();

    let sync_threads = GpuWorkItem(wi.x, wi.y);

    let dim = dim_spec_param();

    // These globals are in LDS (workgroup local) memory.
    // XXX this function can't be generic over ETy *only* because Rust prohibits it.
    // statics aren't allowed to close over generic parameters of the parent function.
    // TODO dynamic group storage instead.
    #[geobacter_attr(platform = "amdgpu", address_space = "local")]
    static mut S_A: MaybeUninit<[ETy; BLOCK_SIZE]> = MaybeUninit::uninit();
    #[geobacter_attr(platform = "amdgpu", address_space = "local")]
    static mut S_B: MaybeUninit<[ETy; BLOCK_SIZE]> = MaybeUninit::uninit();
    #[geobacter_attr(platform = "amdgpu", address_space = "local")]
    static mut S_C: MaybeUninit<[ETy; BLOCK_SIZE]> = MaybeUninit::uninit();

    unsafe {
      let a = MaybeCheckedSlice::unchecked(self.a.dst().as_ref());
      let b = MaybeCheckedSlice::unchecked(self.b.dst().as_ref());
      let c = MaybeCheckedMutSlice::unchecked(self.c as _);
      let sa = MaybeCheckedMutSlice::unchecked(&mut *S_A.as_mut_ptr());
      let sb = MaybeCheckedMutSlice::unchecked(&mut *S_B.as_mut_ptr());
      let sc = MaybeCheckedMutSlice::unchecked(&mut *S_C.as_mut_ptr());

      gemm_v1(a, b, c, sa, sb, sc,
              dim, BLOCK_K as _, BLOCK_K_STRIDE as _,
              wg, sync_threads);
    }
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
const BLOCK_K_STRIDE: usize = BLOCK_K;
// don't need the extra row (if present).
const BLOCK_SIZE: usize = BLOCK_K_STRIDE * BLOCK_K;
type ETy = f32;

/// u64 mul/adds/cmp are expensive, so this exists so we can run the algo with checking
/// on the host, and disable them on the GPU.
enum MaybeCheckedSlice<'a, E> {
  Unchecked(*const E),
  Checked(&'a [E]),
}

impl<'a, E> MaybeCheckedSlice<'a, E> {
  unsafe fn unchecked(ptr: &'a [E]) -> Self {
    MaybeCheckedSlice::Unchecked(ptr.as_ptr())
  }
  fn checked(s: &'a [E]) -> Self {
    MaybeCheckedSlice::Checked(s)
  }
}

impl<'a, E> Index<usize> for MaybeCheckedSlice<'a, E> {
  type Output = E;
  fn index(&self, idx: usize) -> &E {
    match self {
      MaybeCheckedSlice::Unchecked(ptr) => unsafe {
        &*ptr.add(idx)
      },
      MaybeCheckedSlice::Checked(slice) => &slice[idx],
    }
  }
}
enum MaybeCheckedMutSlice<'a, E> {
  Unchecked(*mut E),
  Checked(&'a mut [E]),
}
impl<'a, E> MaybeCheckedMutSlice<'a, E> {
  unsafe fn unchecked(ptr: *mut [E]) -> Self {
    MaybeCheckedMutSlice::Unchecked((*ptr).as_mut_ptr())
  }
  fn checked(s: &'a mut [E]) -> Self {
    MaybeCheckedMutSlice::Checked(s)
  }
}
impl<'a, E> Index<usize> for MaybeCheckedMutSlice<'a, E> {
  type Output = E;
  fn index(&self, idx: usize) -> &E {
    match self {
      MaybeCheckedMutSlice::Unchecked(ptr) => unsafe {
        &*ptr.add(idx)
      },
      MaybeCheckedMutSlice::Checked(slice) => &slice[idx],
    }
  }
}
impl<'a, E> IndexMut<usize> for MaybeCheckedMutSlice<'a, E> {
  fn index_mut(&mut self, idx: usize) -> &mut E {
    match self {
      MaybeCheckedMutSlice::Unchecked(ptr) => unsafe {
        &mut *ptr.add(idx)
      },
      MaybeCheckedMutSlice::Checked(slice) => &mut slice[idx],
    }
  }
}

/// This trait abstracts running code over all workitems. Used by the kernels
/// so that the GEMM can also be ran on a normal CPU.
trait AllWorkItems {
  fn forall_workitems<F>(&self, f: F)
    where F: FnMut(/*wi_x:*/ u32, /*wi_y:*/ u32);
  fn barrier(&self);
  #[inline(always)]
  fn forall_workitems_synced<F>(&self, f: F)
    where F: FnMut(/*wi_x:*/ u32, /*wi_y:*/ u32),
  {
    self.forall_workitems(f);
    self.barrier();
  }
}

unsafe fn gemm_v1<F, E>(a: MaybeCheckedSlice<E>,
                        b: MaybeCheckedSlice<E>,
                        mut c: MaybeCheckedMutSlice<E>,
                        mut sa: MaybeCheckedMutSlice<E>,
                        mut sb: MaybeCheckedMutSlice<E>,
                        mut sc: MaybeCheckedMutSlice<E>,
                        stride: NonZeroUsize, smem_len: u32, smem_stride: u32,
                        (wg_x, wg_y): (u32, u32),
                        sync_threads: F)
  where F: AllWorkItems,
        E: Copy + AddAssign + Mul<Output = E> + PartialEq + fmt::Debug + 'static,
        u32: AsPrimitive<E>,
{
  let stride = stride.get();
  let mod_k = mod_block_k();

  sync_threads.forall_workitems(|wi_x, wi_y| {
    sc[(wi_y * smem_stride + wi_x) as usize] = 0u32.as_();
  });

  let mut k = 0usize;
  while k < stride {
    // init SMEM:
    sync_threads.forall_workitems_synced(|wi_x, wi_y| {
      let sao = (wi_y * smem_stride as u32 + wi_x) as usize;
      let sbo = (wi_x * smem_stride as u32 + wi_y) as usize;

      let ao_y = (wg_y + wi_y) as usize;
      let ao_x = k + wi_x as usize;

      let bo_y = k + wi_y as usize;
      let bo_x = (wg_x + wi_x) as usize;

      let ao = ao_y * stride + ao_x;
      let bo = bo_y * stride + bo_x;

      sa[sao] = if mod_k || (ao_y < stride && ao_x < stride) {
        a[ao]
      } else {
        0u32.as_()
      };
      sb[sbo] = if mod_k || (bo_y < stride && bo_x < stride) {
        b[bo]
      } else {
        0u32.as_()
      };
    });

    // naive gemm from SMEM:
    sync_threads.forall_workitems_synced(|wi_x, wi_y| {
      let vcp = &mut sc[(wi_y * smem_stride + wi_x) as usize];

      let mut kci = 0u16;
      while kci < (smem_len as u16) {
        {
          let kci = kci as u32;

          let ia = (wi_y * smem_stride + kci) as usize;
          let ib = (wi_x * smem_stride + kci) as usize;

          let va = sa[ia];
          let vb = sb[ib];

          *vcp += va * vb;
        }

        kci += 1;
      }
    });

    k += smem_len as usize;
  }

  // copy sc back to C:
  sync_threads.forall_workitems(|wi_x, wi_y| {
    let i_y = (wg_y + wi_y) as usize;
    let i_x = (wg_x + wi_x) as usize;
    let idx = i_y * stride + i_x;
    if mod_k || (i_y < stride && i_x < stride) {
      // ensure each output is written only once.
      host_debug_assert_eq!(c[idx], 0u32.as_());
      let vc = &sc[(wi_y * smem_stride + wi_x) as usize];
      c[idx] = *vc;
    }
  });
}

#[allow(dead_code)]
fn test_gemm_v1(a: &[ETy], b: &[ETy], c: &mut [ETy], dim: NonZeroUsize) {

  #[derive(Clone, Copy, Debug)]
  struct HostAllWorkItems;
  impl AllWorkItems for HostAllWorkItems {
    #[inline(always)]
    fn forall_workitems<F>(&self, mut f: F)
      where F: FnMut(/*wi_x:*/ u32, /*wi_y:*/ u32),
    {
      let mut wi_y = 0u32;
      while wi_y < BLOCK_K as u32 {
        let mut wi_x = 0u32;
        while wi_x < BLOCK_K as u32 {

          f(wi_x, wi_y);

          wi_x += 1;
        }
        wi_y += 1;
      }
    }
    #[inline(always)]
    fn barrier(&self) { }
  }

  let sync_threads = HostAllWorkItems;

  (0..dim.get())
    .step_by(BLOCK_K)
    .flat_map(|wg_y| {
      (0..dim.get())
        .step_by(BLOCK_K)
        .map(move |wg_x| (wg_x, wg_y) )
    })
    .for_each(|(wg_x, wg_y)| {
      let a = MaybeCheckedSlice::checked(a);
      let b = MaybeCheckedSlice::checked(b);
      let c = MaybeCheckedMutSlice::checked(c);

      let mut t_a: MaybeUninit<[ETy; BLOCK_SIZE]> = MaybeUninit::uninit();
      let mut t_b: MaybeUninit<[ETy; BLOCK_SIZE]> = MaybeUninit::uninit();
      let mut t_c: MaybeUninit<[ETy; BLOCK_SIZE]> = MaybeUninit::uninit();
      let wg = (wg_x as _, wg_y as _);

      unsafe {
        let sa = MaybeCheckedMutSlice::checked(&mut *t_a.as_mut_ptr());
        let sb = MaybeCheckedMutSlice::checked(&mut *t_b.as_mut_ptr());
        let sc = MaybeCheckedMutSlice::checked(&mut *t_c.as_mut_ptr());

        gemm_v1(a, b, c, sa, sb, sc,
                dim, BLOCK_K as _, BLOCK_K_STRIDE as _,
                wg, sync_threads);
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

  //test_gemm_v1(&la, &lb, &mut lc, dim);

  time("nd linalg gemm", || {
    let a = nd::aview1(&la[..]).into_shape(shape).unwrap();
    let b = nd::aview1(&lb[..]).into_shape(shape).unwrap();
    let mut c = nd::aview_mut1(&mut nd_lc[..]).into_shape(shape).unwrap();

    // compute using host and check against the GPU's results:
    nd::linalg::general_mat_mul(1.0 as ETy, &a, &b,
                                0.0 as ETy, &mut c);

    let lc = nd::aview1(&lc[..]).into_shape(shape).unwrap();
    approx::assert_relative_eq!(c, lc, epsilon = 5000.0 * std::f32::EPSILON);
  });
}
