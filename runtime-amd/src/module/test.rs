
#![allow(dead_code)]

use super::*;
use crate::alloc::*;
use crate::utils::test::*;

use hsa_rt::queue::RingQueue;

struct TestKernel<'a, E, F, Q = DeviceMultiQueue, S = GlobalSignal, G = Dim1D<Range<u32>>> {
  /// Host memory only.
  dst: *mut [E],
  q: Q,
  c: S,
  /// Amazingly, this works. The reason is that `self.f`'s type is known here; thus
  /// rustc calls `F` directly.
  f: F,
  _l: PhantomData<&'a mut E>,
  _g: PhantomData<*const G>,
}
type TestKernel1<'a, E, F, Q, S> = TestKernel<'a, E, F, Q, S, Dim1D<Range<u32>>>;
type TestKernel2<'a, E, F, Q, S> = TestKernel<'a, E, F, Q, S, Dim2D<Range<u32>>>;
type TestKernel3<'a, E, F, Q, S> = TestKernel<'a, E, F, Q, S, Dim3D<Range<u32>>>;

impl<'a, E, F, S, G> TestKernel<'a, E, F, DeviceMultiQueue, S, G>
  where Self: Kernel,
        E: Send + Sync,
        F: Fn(*mut [E], VectorParams<G>) + Unpin + Send + Sync,
        S: SignalHandle + Unpin + Send + Sync,
        G: GridDims<Elem = u32>,
        G::Workgroup: From<RangeTo<u16>>,
{
  fn new(dev: &Arc<HsaAmdGpuAccel>, m: &'a mut LapVec<E>,
         count: &G, v: E, f: F) -> (TestInvoc<Self>, Self)
    where E: Clone,
          S: SignalFactory,
  {
    let s = S::new(dev, 1).unwrap();
    m.resize(count.linear_len().unwrap() as usize, v);
    m.add_access(&dev).unwrap();

    let q = dev.create_multi_queue(None).unwrap();

    let o = TestKernel {
      dst: m.as_mut_slice(),
      q,
      c: s,
      f,
      _l: PhantomData,
      _g: PhantomData,
    };

    let invoc = FuncModule::new(dev);
    let invoc = invoc.into_invoc(args_pool());

    (invoc, o)
  }
}
impl<'a, E, F, G> TestKernel<'a, E, F, DeviceMultiQueue, GlobalSignal, G>
  where Self: Kernel,
        E: Send + Sync,
        F: Fn(*mut [E], VectorParams<G>) + Unpin + Send + Sync,
        G: GridDims<Elem = u32>,
        G::Workgroup: From<RangeTo<u16>>,
{
  fn new_global(dev: &Arc<HsaAmdGpuAccel>, m: &'a mut LapVec<E>,
                count: &G, v: E, f: F) -> (TestInvoc<Self>, Self)
    where E: Clone,
  {
    TestKernel::new(dev, m, count, v, f)
  }
}

impl<'a, E, F, Q, S, G> Completion for TestKernel<'a, E, F, Q, S, G>
  where S: SignalHandle + Unpin + Send + Sync,
{
  type CompletionSignal = S;
  fn completion(&self) -> &S { &self.c }
}

// Keep in sync with the Kernel impls below!
const WORKGROUP1: Dim1D<RangeTo<u16>> = Dim1D {
  x: ..8,
};
const WORKGROUP2: Dim2D<RangeTo<u16>> = Dim2D {
  x: ..8,
  y: ..8,
};
const WORKGROUP3: Dim3D<RangeTo<u16>> = Dim3D {
  x: ..8,
  y: ..8,
  z: ..8,
};

impl<'a, E, F, Q, S> Kernel for TestKernel1<'a, E, F, Q, S>
  where E: Send + Sync,
        Q: RingQueue + Unpin + Send + Sync,
        F: Fn(*mut [E], VectorParams<Dim1D<Range<u32>>>) + Unpin + Send + Sync,
        S: SignalHandle + Unpin + Send + Sync,
{
  type Grid = Dim1D<Range<u32>>;
  const WORKGROUP: <Self::Grid as GridDims>::Workgroup = Dim1D {
    x: ..8,
  };

  type Queue = Q;

  fn queue(&self) -> &Q { &self.q }

  fn kernel(&self, vp: VectorParams<Self::Grid>)
    where Self: Sized
  {
    (self.f)(self.dst, vp);
  }
}
impl<'a, E, F, Q, S> Kernel for TestKernel2<'a, E, F, Q, S>
  where E: Send + Sync,
        Q: RingQueue + Unpin + Send + Sync,
        F: Fn(*mut [E], VectorParams<Dim2D<Range<u32>>>) + Unpin + Send + Sync,
        S: SignalHandle + Unpin + Send + Sync,
{
  type Grid = Dim2D<Range<u32>>;
  const WORKGROUP: <Self::Grid as GridDims>::Workgroup = Dim2D {
    x: ..8,
    y: ..8,
  };

  type Queue = Q;

  fn queue(&self) -> &Q { &self.q }

  fn kernel(&self, vp: VectorParams<Self::Grid>)
    where Self: Sized
  {
    (self.f)(self.dst, vp);
  }
}
impl<'a, E, F, Q, S> Kernel for TestKernel3<'a, E, F, Q, S>
  where E: Send + Sync,
        Q: RingQueue + Unpin + Send + Sync,
        F: Fn(*mut [E], VectorParams<Dim3D<Range<u32>>>) + Unpin + Send + Sync,
        S: SignalHandle + Unpin + Send + Sync,
{
  type Grid = Dim3D<Range<u32>>;
  const WORKGROUP: <Self::Grid as GridDims>::Workgroup = Dim3D {
    x: ..8,
    y: ..8,
    z: ..8,
  };

  type Queue = Q;

  fn queue(&self) -> &Q { &self.q }

  fn kernel(&self, vp: VectorParams<Self::Grid>)
    where Self: Sized
  {
    (self.f)(self.dst, vp);
  }
}
/// Has to be implemented manually because our derive macro doesn't work here.
unsafe impl<'b, E, F, Q, S, G> Deps for TestKernel<'b, E, F, Q, S, G> {
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), Error>)
    -> Result<(), Error>
  {
    // no deps; we only use host memory for tests.
    Ok(())
  }
}

unsafe impl<'a, E, F, Q, S, G> Send for TestKernel<'a, E, F, Q, S, G>
  where E: Send,
        F: Send,
        Q: Send,
        S: Send,
{ }
unsafe impl<'a, E, F, Q, S, G> Sync for TestKernel<'a, E, F, Q, S, G>
  where E: Sync,
        F: Sync,
        Q: Sync,
        S: Sync,
{ }

#[test]
fn zero_grid_err() {
  let dev = device();

  let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

  const GRID: Dim1D<Range<u32>> = Dim1D { x: 0..0, };
  const FAKE_GRID: Dim1D<Range<u32>> = Dim1D { x: 0..1, };

  fn trivial_f(_: *mut [u32], _: VectorParams<Dim1D<Range<u32>>>) {
    unreachable!();
  }

  unsafe {
    let (mut invoc, k) = TestKernel::new_global(&dev, &mut m,
                                                &FAKE_GRID, 0u32,
                                                trivial_f);
    assert!(invoc.unchecked_call_async(&GRID, k).is_err());
  }
}
#[test]
fn overflow_err() {
  let dev = device();

  let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

  const GRID: Dim2D<Range<u32>> = Dim2D {
    x: 0..u32::max_value(),
    y: 0..u32::max_value(),
  };
  const FAKE_GRID: Dim2D<Range<u32>> = Dim2D { x: 0..1, y: 0..1, };

  fn trivial_f(_: *mut [u32], _: VectorParams<Dim2D<Range<u32>>>) {
    unreachable!();
  }

  unsafe {
    let (mut invoc, k) = TestKernel::new_global(&dev, &mut m,
                                                &FAKE_GRID, 0u32,
                                                trivial_f);
    assert!(invoc.unchecked_call_async(&GRID, k).is_err());
  }
}
#[test]
fn underflow_err() {
  let dev = device();

  let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

  const GRID: Dim1D<Range<u32>> = Dim1D { x: 1..0, };
  const FAKE_GRID: Dim1D<Range<u32>> = Dim1D { x: 0..1, };

  fn trivial_f(_: *mut [u32], _: VectorParams<Dim1D<Range<u32>>>) {
    unreachable!();
  }

  unsafe {
    let (mut invoc, k) = TestKernel::new_global(&dev, &mut m,
                                                &FAKE_GRID, 0u32,
                                                trivial_f);
    assert!(invoc.unchecked_call_async(&GRID, k).is_err());
  }
}

#[test]
fn device_wg_limit_err() {
  let dev = device();

  const GRID: Dim2D<Range<u32>> = Dim2D {
    x: 0..1,
    y: 0..1,
  };

  struct LargeWg;
  impl Completion for LargeWg {
    type CompletionSignal = GlobalSignal;
    fn completion(&self) -> &GlobalSignal { unreachable!(); }
  }
  impl Kernel for LargeWg {
    type Grid = Dim2D<Range<u32>>;
    const WORKGROUP: Dim2D<RangeTo<u16>> = Dim2D {
      x: ..1024,
      y: ..1024,
    };

    type Queue = DeviceMultiQueue;
    fn queue(&self) -> &Self::Queue { unreachable!(); }

    fn kernel(&self, _: KVectorParams<Self>)
      where Self: Sized,
    {
      unreachable!();
    }
  }
  unsafe impl Deps for LargeWg {
    fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), Error>)
      -> Result<(), Error>
    {
      Ok(())
    }
  }

  let invoc: FuncModule<LargeWg> = FuncModule::new(&dev);
  let mut invoc = invoc.into_invoc(args_pool());

  unsafe {
    assert!(invoc.unchecked_call_async(&GRID, LargeWg).is_err());
  }
}


mod one_d {
  use super::*;

  #[test]
  fn trivial() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

    const GRID: Dim1D<Range<u32>> = Dim1D { x: 0..16, };

    fn trivial_f(dst: *mut [u32], vp: VectorParams<Dim1D<Range<u32>>>) {
      unsafe {
        (&mut *dst)[*vp.gl_id() as usize] = *vp.gl_id();
      }
    }

    unsafe {
      let (mut invoc, k) = TestKernel::new_global(&dev, &mut m,
                                                  &GRID, 0u32,
                                                  trivial_f);
      let _wait = invoc
        .unchecked_call_async(&GRID, k)
        .unwrap();
    }

    for (i, &v) in m.iter().enumerate() {
      assert_eq!(v as usize, i);
    }
  }

  #[test]
  fn grid_rounding() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

    const GRID: Dim1D<Range<u32>> = Dim1D { x: 0..12, };
    const FULL_GRID: Dim1D<Range<u32>> = Dim1D { x: 0..16, };

    fn trivial_f(dst: *mut [u32], vp: VectorParams<Dim1D<Range<u32>>>) {
      let glid = gamd_std::dispatch_packet().global_linear_id();
      unsafe {
        (&mut *dst)[glid] = *vp.gl_id();
      }
    }

    unsafe {
      let (mut invoc, k) = TestKernel::new_global(&dev, &mut m,
                                                  &FULL_GRID, 0u32,
                                                  trivial_f);
      let _wait = invoc
        .unchecked_call_async(&GRID, k)
        .unwrap();
    }

    for (i, &v) in m.iter().enumerate() {
      if i < 12 {
        assert_eq!(v as usize, i);
      } else {
        assert_eq!(v, 0);
      }
    }
  }

  #[test]
  fn glid_offset() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

    const GRID: Dim1D<Range<u32>> = Dim1D { x: 32..48, };

    fn trivial_f(dst: *mut [u32], vp: VectorParams<Dim1D<Range<u32>>>) {
      let glid = gamd_std::dispatch_packet().global_linear_id();
      unsafe {
        (&mut *dst)[glid] = *vp.gl_id();
      }
    }

    unsafe {
      let (mut invoc, k) = TestKernel1::new_global(&dev, &mut m,
                                                   &GRID, 0u32,
                                                   trivial_f);
      let _wait = invoc
        .unchecked_call_async(&GRID, k)
        .unwrap();
    }

    for (i, &v) in m.iter().enumerate() {
      assert_eq!(v as usize, i + 32);
    }
  }

  #[test]
  fn workitem_idx() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

    const GRID: Dim1D<Range<u32>> = Dim1D { x: 0..32, };

    fn trivial_f(dst: *mut [u32], vp: VectorParams<Dim1D<Range<u32>>>) {
      let glid = gamd_std::dispatch_packet().global_linear_id();
      unsafe {
        (&mut *dst)[glid] = vp.wi().x as u32;
      }
    }

    unsafe {
      let (mut invoc, k) = TestKernel1::new_global(&dev, &mut m,
                                                   &GRID, 0u32,
                                                   trivial_f);
      let _wait = invoc
        .unchecked_call_async(&GRID, k)
        .unwrap();
    }

    for (i, &v) in m.iter().enumerate() {
      assert_eq!(v as usize, i % (WORKGROUP1.x.end as usize));
    }
  }

  #[test]
  fn workgroup_id() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

    const GRID: Dim1D<Range<u32>> = Dim1D { x: 0..32, };

    fn trivial_f(dst: *mut [u32], vp: VectorParams<Dim1D<Range<u32>>>) {
      let glid = gamd_std::dispatch_packet().global_linear_id();
      unsafe {
        (&mut *dst)[glid] = vp.wg_id().x;
      }
    }

    unsafe {
      let (mut invoc, k) = TestKernel1::new_global(&dev, &mut m,
                                                   &GRID, 0u32,
                                                   trivial_f);
      let _wait = invoc
        .unchecked_call_async(&GRID, k)
        .unwrap();
    }

    let wg_size = WORKGROUP1.x.end as usize;
    for (i, &v) in m.iter().enumerate() {
      assert_eq!(v as usize, i / wg_size);
    }
  }

  #[test]
  fn workgroup_idx() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

    const GRID: Dim1D<Range<u32>> = Dim1D { x: 0..32, };

    fn trivial_f(dst: *mut [u32], vp: VectorParams<Dim1D<Range<u32>>>) {
      let glid = gamd_std::dispatch_packet().global_linear_id();
      unsafe {
        (&mut *dst)[glid] = vp.wg_idx().x;
      }
    }

    unsafe {
      let (mut invoc, k) = TestKernel1::new_global(&dev, &mut m,
                                                   &GRID, 0u32,
                                                   trivial_f);
      let _wait = invoc
        .unchecked_call_async(&GRID, k)
        .unwrap();
    }

    let wg_size = WORKGROUP1.x.end as usize;
    for (i, &v) in m.iter().enumerate() {
      assert_eq!(v as usize, (i / wg_size) * wg_size);
    }
  }

  #[test]
  fn grid_range_types() {
    // just check that this compiles
    fn enforce_grid_dims<T>(_: &T)
      where T: GridDims,
    { }
    enforce_grid_dims(&Dim1D { x: ..32, });
    enforce_grid_dims(&Dim1D { x: 0..32, });
    enforce_grid_dims(&Dim1D { x: 0..=32, });
    enforce_grid_dims(&Dim1D { x: ..=32, });
  }
}

mod three_d {
  use super::*;

  #[test]
  fn trivial() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

    let grid: Dim3D<Range<u32>> = (0..16).into();

    fn trivial_f(dst: *mut [u32], vp: VectorParams<Dim3D<Range<u32>>>) {
      unsafe {
        (&mut *dst)[*vp.gl_id() as usize] = *vp.gl_id();
      }
    }

    unsafe {
      let (mut invoc, k) = TestKernel::new_global(&dev, &mut m,
                                                  &grid, 0u32,
                                                  trivial_f);
      let _wait = invoc
        .unchecked_call_async(&grid, k)
        .unwrap();
    }

    for (i, &v) in m.iter().enumerate() {
      assert_eq!(v as usize, i);
    }
  }
  #[test]
  fn glid_offset() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

    let grid: Dim3D<Range<u32>> = Dim3D {
      x: (0..16).into(),
      y: (32..48).into(),
      z: (0..16).into(),
    };

    fn trivial_f(dst: *mut [u32], vp: VectorParams<Dim3D<Range<u32>>>) {
      let glid = gamd_std::dispatch_packet().global_linear_id();
      unsafe {
        (&mut *dst)[glid] = *vp.gl_id();
      }
    }

    unsafe {
      let (mut invoc, k) = TestKernel::new_global(&dev, &mut m,
                                                  &grid, 0u32,
                                                  trivial_f);
      let _wait = invoc
        .unchecked_call_async(&grid, k)
        .unwrap();
    }

    for (i, &v) in m.iter().enumerate() {
      assert_eq!(v as usize, i + 32 * 16);
    }
  }
  #[test]
  fn workitem_idx() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

    let grid: Dim3D<Range<u32>> = Dim3D {
      x: (0..16).into(),
      y: (0..32).into(),
      z: (0..8).into(),
    };

    fn trivial_f(dst: *mut [(u32, u32, u32)], vp: VectorParams<Dim3D<Range<u32>>>) {
      let glid = gamd_std::dispatch_packet().global_linear_id();
      unsafe {
        (&mut *dst)[glid] = (vp.wi().x as _, vp.wi().y as _, vp.wi().z as _);
      }
    }

    unsafe {
      let (mut invoc, k) = TestKernel::new_global(&dev, &mut m,
                                                  &grid, (0u32, 0u32, 0u32),
                                                  trivial_f);
      let _wait = invoc
        .unchecked_call_async(&grid, k)
        .unwrap();
    }

    let wg_size = WORKGROUP3.len().as_::<usize>();
    let grid_size = grid.len().as_::<usize>();
    for (i, &(x, y, z)) in m.iter().enumerate() {
      assert_eq!(x as usize, i % wg_size.x);
      assert_eq!(y as usize, (i / grid_size.x) % wg_size.y);
      assert_eq!(z as usize, (i / grid_size.x / grid_size.y) % wg_size.z);
    }
  }
  #[test]
  fn workgroup_id() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

    let grid: Dim3D<Range<u32>> = Dim3D {
      x: (0..16).into(),
      y: (0..32).into(),
      z: (0..8).into(),
    };

    fn trivial_f(dst: *mut [(u32, u32, u32)], vp: VectorParams<Dim3D<Range<u32>>>) {
      let glid = gamd_std::dispatch_packet().global_linear_id();
      unsafe {
        (&mut *dst)[glid] = (vp.wg_id().x, vp.wg_id().y, vp.wg_id().z);
      }
    }

    unsafe {
      let (mut invoc, k) = TestKernel::new_global(&dev, &mut m,
                                                  &grid, (0u32, 0u32, 0u32),
                                                  trivial_f);
      let _wait = invoc
        .unchecked_call_async(&grid, k)
        .unwrap();
    }

    let wg_size = WORKGROUP3.len().as_::<u32>();
    let grid_size = grid.len();
    for zi in grid.z.clone() {
      for yi in grid.y.clone() {
        for xi in grid.x.clone() {
          let idx = (zi * grid_size.y + yi) * grid_size.x + xi;
          let (x, y, z) = m[idx as usize];
          assert_eq!(x, xi / wg_size.x);
          assert_eq!(y, yi / wg_size.y);
          assert_eq!(z, zi / wg_size.z);
        }
      }
    }
  }
  #[test]
  fn workgroup_idx() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));

    let grid: Dim3D<Range<u32>> = Dim3D {
      x: (0..16).into(),
      y: (0..32).into(),
      z: (0..8).into(),
    };

    fn trivial_f(dst: *mut [(u32, u32, u32)], vp: VectorParams<Dim3D<Range<u32>>>) {
      let glid = gamd_std::dispatch_packet().global_linear_id();
      unsafe {
        (&mut *dst)[glid] = (vp.wg_idx().x, vp.wg_idx().y, vp.wg_idx().z);
      }
    }

    unsafe {
      let (mut invoc, k) = TestKernel::new_global(&dev, &mut m,
                                                  &grid, (0u32, 0u32, 0u32),
                                                  trivial_f);
      let _wait = invoc
        .unchecked_call_async(&grid, k)
        .unwrap();
    }

    let wg_size = WORKGROUP3.len().as_::<u32>();
    let grid_size = grid.len();
    for zi in grid.z.clone() {
      for yi in grid.y.clone() {
        for xi in grid.x.clone() {
          let idx = (zi * grid_size.y + yi) * grid_size.x + xi;
          let (x, y, z) = m[idx as usize];
          assert_eq!(x, (xi / wg_size.x) * wg_size.x);
          assert_eq!(y, (yi / wg_size.y) * wg_size.y);
          assert_eq!(z, (zi / wg_size.z) * wg_size.z);
        }
      }
    }
  }
}
