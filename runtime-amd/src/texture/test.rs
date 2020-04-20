
use super::*;
use super::channel_mask::*;
use crate::prelude::*;
use crate::utils::test::*;

type TwoDUNormU8Texture<A, Ord, L> = Image<
  A,
  Format<channel_type::NormU8, Ord>,
  geometry::TwoD<usize>,
  L,
  ImageData,
>;
type TwoDU8Texture<A, Ord, L> = Image<
  A,
  Format<u8, Ord>,
  geometry::TwoD<usize>,
  L,
  ImageData,
>;
type TwoDU32Texture<A, Ord, L> = Image<
  A,
  Format<u32, Ord>,
  geometry::TwoD<usize>,
  L,
  ImageData,
>;

struct Test<'a, T, A, F, G, L, K>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{
  dst: *mut [T],
  src: ImageRef<'a, A, F, G, L>,
  f: K,
  completion: GlobalSignal,
}
unsafe impl<'a, T, A, F, G, L, K> Send for Test<'a, T, A, F, G, L, K>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
        K: Send,
{ }
unsafe impl<'a, T, A, F, G, L, K> Sync for Test<'a, T, A, F, G, L, K>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
        K: Sync,
{ }
unsafe impl<'b, T, A, F, G, L, K> Deps for Test<'b, T, A, F, G, L, K>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{
  #[inline(always)]
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), Error>)
    -> Result<(), Error>
  {
    f(&self.completion)
  }
}
impl<'a, T, A, F, G, L, K> Completion for Test<'a, T, A, F, G, L, K>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
{
  type CompletionSignal = GlobalSignal;
  fn completion(&self) -> &GlobalSignal { &self.completion }
}
impl<'b, T, A, F, G, L, K> Kernel for Test<'b, T, A, F, G, L, K>
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
        K: for<'a> Fn(ImageRef<'a, A, F, G, L>, &'a VectorParams<Dim2D<RangeTo<u32>>>) -> T,
        K: Sync + Send + Unpin,
{
  type Grid = Dim2D<RangeTo<u32>>;
  const WORKGROUP: Dim2D<RangeTo<u16>> = Dim2D {
    x: ..4,
    y: ..4,
  };

  type Queue = DeviceMultiQueue;
  fn queue(&self) -> &Self::Queue { queue() }

  fn kernel(&self, vp: KVectorParams<Self>) {
    let dst = unsafe {
      (&mut *self.dst).get_mut(vp.gl_id() as usize)
    };
    if let Some(dst) = dst {
      let tex = self.src.as_ref();
      *dst = (self.f)(tex, &vp);
    }
  }
}

fn launch<T, A, F, G, L, K>(dev: &Arc<HsaAmdGpuAccel>,
                            src: ImageRef<A, F, G, L>,
                            dst: &mut LapVec<T>,
                            grid: &Dim2D<RangeTo<u32>>,
                            f: K)
  where A: AccessDetail,
        F: FormatDetail,
        G: GeometryDetail,
        L: LayoutDetail,
        K: for<'a> Fn(ImageRef<'a, A, F, G, L>, &'a VectorParams<Dim2D<RangeTo<u32>>>) -> T,
        K: Send + Sync + Unpin,
{
  let module = <Test<T, A, F, G, L, K> as Kernel>::module(dev);
  let pool = args_pool();
  let mut invoc = module.into_invoc(pool);

  let args = Test {
    dst: &mut dst[..],
    src,
    f,
    completion: GlobalSignal::new(1).unwrap(),
  };

  unsafe {
    invoc
      .unchecked_call_async(grid, args)
      .expect("GPU invoc")
      .wait_for_zero(true)
      .expect("GPU wait");
  }
}

fn alloc_2d<T, A, F, L>(dev: &Arc<HsaAmdGpuAccel>, grid: &Dim2D<RangeTo<u32>>)
  -> (LapVec<<F as FormatDetail>::HostType>,
      LapVec<T>, Image<A, F, TwoD<usize>, L, ImageData>)
  where T: Default,
        A: AccessDetail,
        F: FormatDetail,
        <F as FormatDetail>::HostType: Default,
        L: LayoutDetail,
{
  let alloc = dev.fine_lap_node_alloc(0);
  let mut dst = LapVec::new_in(alloc.clone());
  dst.add_access(dev).unwrap();
  dst.resize_with(grid.linear_len().unwrap() as usize, T::default);

  let mut src = LapVec::new_in(alloc);
  src.add_access(dev).unwrap();
  src.resize_with(grid.linear_len().unwrap() as usize, Default::default);

  let len = grid.len().as_::<usize>();
  let tex = dev.create_texture(len.into(), L::default())
    .expect("create_image");

  (src, dst, tex)
}
fn alloc_rw_2d_u32_r<T, L>(dev: &Arc<HsaAmdGpuAccel>, grid: &Dim2D<RangeTo<u32>>)
  -> (LapVec<u32>, LapVec<T>, TwoDU32Texture<access::ReadWrite, channel_order::R, L>)
  where T: Default,
        L: LayoutDetail + Default,
{
  alloc_2d(dev, grid)
}
fn alloc_rw_2d_u32_rgba<T, L>(dev: &Arc<HsaAmdGpuAccel>, grid: &Dim2D<RangeTo<u32>>)
  -> (LapVec<[u32; 4]>, LapVec<T>, TwoDU32Texture<access::ReadWrite, channel_order::RGBA, L>)
  where T: Default,
        L: LayoutDetail + Default,
{
  alloc_2d(dev, grid)
}
fn alloc_rw_2d_unorm_u8_rgba<T, L>(dev: &Arc<HsaAmdGpuAccel>, grid: &Dim2D<RangeTo<u32>>)
  -> (LapVec<[u8; 4]>, LapVec<T>,
      TwoDUNormU8Texture<access::ReadWrite, channel_order::RGBA, L>)
  where T: Default,
        L: LayoutDetail + Default,
{
  alloc_2d(dev, grid)
}
fn alloc_rw_2d_u8_argb<T, L>(dev: &Arc<HsaAmdGpuAccel>, grid: &Dim2D<RangeTo<u32>>)
  -> (LapVec<[u8; 4]>, LapVec<T>,
      TwoDU8Texture<access::ReadWrite, channel_order::ARGB, L>)
  where T: Default,
        L: LayoutDetail + Default,
{
  alloc_2d(dev, grid)
}

#[test]
fn import_export() {
  let dev = device();

  let grid = Dim2D {
    x: ..2,
    y: ..2,
  };

  let (mut src, mut dst, mut tex) =
    alloc_rw_2d_u32_r::<u32, layout::Opaque>(&dev, &grid);

  for (i, src) in src.iter_mut().enumerate() {
    *src = i as u32;
  }

  tex.import_all_packed(&src).expect("texture import");
  tex.export_all_packed(&mut dst).expect("texture export");

  assert_eq!(src, dst);
}

#[test]
fn identity_r() {
  let dev = device();

  let grid = Dim2D {
    x: ..8,
    y: ..8,
  };

  let (mut src, mut dst, mut tex) = alloc_rw_2d_u32_r::<u32, Opaque>
    (&dev, &grid);

  for (i, src) in src.iter_mut().enumerate() {
    *src = i as u32;
  }

  tex.import_all_packed(&src).expect("texture import");
  launch(&dev, tex.as_ref(), &mut dst, &grid,
         |tex, vp| {
           tex.load((vp.grid_id().x, vp.grid_id().y))
         });

  for (i, &dst) in dst.iter().enumerate() {
    assert_eq!(i as u32, dst);
  }
}

#[test]
fn identity_rgba() {
  let dev = device();

  let grid = Dim2D {
    x: ..16,
    y: ..16,
  };

  let (mut src, mut dst, mut tex) = alloc_rw_2d_u32_rgba::<[u32; 4], Opaque>
    (&dev, &grid);

  for (i, src) in src.iter_mut().enumerate() {
    let i = (i * 4) as u32;
    *src = [i, i + 1, i + 2, i + 3];
  }

  tex.import_all_packed(&src).expect("texture import");
  launch(&dev, tex.as_ref(), &mut dst, &grid,
         |tex, vp| {
           tex.load((vp.grid_id().x, vp.grid_id().y))
         });

  for (&src, &dst) in src.iter().zip(dst.iter()) {
    assert_eq!(src, dst);
  }
}
#[test]
fn identity_argb() {
  let dev = device();

  let grid = Dim2D {
    x: ..16,
    y: ..16,
  };

  let (mut src, mut dst, mut tex) = alloc_rw_2d_u8_argb::<[u8; 4], Opaque>
    (&dev, &grid);

  for src in src.iter_mut() {
    *src = [1, 2, 3, 4, ];
  }

  tex.import_all_packed(&src).expect("texture import");
  launch(&dev, tex.as_ref(), &mut dst, &grid,
         |tex, vp| {
           tex.load((vp.grid_id().x, vp.grid_id().y))
         });

  for &dst in dst.iter() {
    assert_eq!([2, 3, 4, 1], dst);
  }
}

fn rgba_load_masked<R, G, B, A>(expected: [u32; 4])
  where R: Bit, G: Bit, B: Bit, A: Bit,
        geometry::TwoD<usize>: ImageOps<[f32; 4], Mask<R, G, B, A>>,
{
  let dev = device();

  let grid = Dim2D {
    x: ..16,
    y: ..16,
  };

  let (mut src, mut dst, mut tex) = alloc_rw_2d_u32_rgba::<[u32; 4], Opaque>
    (&dev, &grid);

  for src in src.iter_mut() {
    *src = [1u32, 2, 3, 4, ];
  }

  tex.import_all_packed(&src).expect("texture import");
  launch(&dev, tex.as_ref(), &mut dst, &grid,
         |tex, vp| {
           let i = (vp.grid_id().x, vp.grid_id().y);
           tex.load_masked::<_, Mask<R, G, B, A>>(i)
         });

  for &dst in dst.iter() {
    assert_eq!(expected, dst);
  }
}

#[test]
fn rgba_load_masked_n_n_y_n() {
  rgba_load_masked::<N, N, Y, N>([0, 0, 3, 0]);
}

fn rgba_store_masked<R, G, B, A>(pixel: [u32; 4], expected: [u32; 4])
  where R: Bit, G: Bit, B: Bit, A: Bit,
        geometry::TwoD<usize>: ImageOps<[f32; 4], Mask<R, G, B, A>>,
{
  let dev = device();

  let grid = Dim2D {
    x: ..16,
    y: ..16,
  };

  let (mut src, mut dst, mut tex) = alloc_rw_2d_u32_rgba::<[u32; 4], Opaque>
    (&dev, &grid);

  for src in src.iter_mut() {
    *src = [1u32, 2, 3, 4, ];
  }

  tex.import_all_packed(&src).expect("texture import");
  launch(&dev, tex.as_ref(), &mut dst, &grid,
         move |tex, vp| {
           let i = (vp.grid_id().x, vp.grid_id().y);

           tex.store_masked::<_, Mask<R, G, B, A>>(i, pixel);
           [u32::MAX; 4]
         });
  tex.export_all_packed(&mut dst).expect("texture export");

  for &dst in dst.iter() {
    assert_eq!(expected, dst);
  }
}

#[test]
fn rgba_store_masked_n_n_y_n() {
  rgba_store_masked::<N, N, Y, N>([7, 8, 5, 9], [0, 0, 5, 0]);
}

#[test]
fn unorm_u8_to_f32() {
  let dev = device();

  let grid = Dim2D {
    x: ..16,
    y: ..16,
  };

  let (mut src, mut dst, mut tex) = alloc_rw_2d_unorm_u8_rgba::<[f32; 4], Opaque>
    (&dev, &grid);

  for src in src.iter_mut() {
    *src = [
      (255.0f32 / 4.0) as u8,
      (255.0f32 / 2.0) as _,
      (3.0f32 * (255.0f32 / 4.0)) as _,
      255,
    ];
  }

  tex.import_all_packed(&src).expect("texture import");
  launch(&dev, tex.as_ref(), &mut dst, &grid,
         move |tex, vp| {
           tex.load((vp.grid_id().x, vp.grid_id().y))
         });

  let expected = [1.0f32 / 4.0, 2.0 / 4.0, 3.0 / 4.0, 4.0 / 4.0];
  for dst in dst.iter() {
    for (&exp, &dst) in expected.iter().zip(dst.iter()) {
      approx::assert_abs_diff_eq!(exp, dst, epsilon = (1.0f32 / 255.0));
    }
  }
}
