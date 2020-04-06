
use std::ptr::NonNull;

use alloc_wg::alloc::*;
use alloc_wg::vec::Vec;

use hsa_rt::mem::region::RegionAlloc;

use super::*;

pub type ArgsBox<T> = alloc_wg::boxed::Box<T, hsa_rt::mem::region::RegionAlloc>;

/// Use this to invoc in a loop without allocating every iteration
/// AND without running amuck of Rust's borrow checker.
/// XXX Use the AMD vendor extensions to get the cacheline size, instead of
/// hardcoding to 1 << 7 here.
pub struct ArgsPool {
  device: Arc<HsaAmdGpuAccel>,
  base: ArgsBox<[u8]>,
  allocated: AtomicUsize,
}
impl ArgsPool {
  /// Create storage for `n` function calls for use on the provided accelerator.
  pub fn new<A>(accel: &Arc<HsaAmdGpuAccel>, count: usize) -> Result<Self, Error>
    where A: Kernel + Sized,
  {
    use std::cmp::max;

    let kernargs_region = accel.kernargs_region().clone();

    let layout = Layout::new::<super::InvocArgs<A>>();
    let pool_alignment = kernargs_region.alloc_alignment();
    if pool_alignment < layout.align() {
      return Err(Error::Alloc(layout));
    }
    let (layout, _) = layout.repeat(count)
      .ok()
      .ok_or(Error::Overflow)?;
    let bytes = layout.size();
    let pool_min_alloc = kernargs_region.alloc_granule();
    // bump the size to the minimum allocation size:
    let bytes = max(pool_min_alloc, bytes);

    let mut arena: Vec<u8, RegionAlloc> =
      Vec::try_with_capacity_in(bytes, kernargs_region)?;
    unsafe {
      arena.set_len(bytes);
    }

    Ok(ArgsPool {
      device: accel.clone(),
      allocated: AtomicUsize::new(arena.as_ptr() as usize),
      base: arena.try_into_boxed_slice()?,
    })
  }

  pub fn new_arena(accel: &Arc<HsaAmdGpuAccel>, bytes: usize)
    -> Result<Self, Error>
  {
    use std::cmp::max;

    let kernargs_region = accel.kernargs_region().clone();
    let pool_min_alloc = kernargs_region.alloc_granule();
    // bump the size to the minimum allocation size:
    let bytes = max(pool_min_alloc, bytes);

    let mut arena: Vec<u8, RegionAlloc> =
      Vec::try_with_capacity_in(bytes, kernargs_region)?;
    unsafe {
      arena.set_len(bytes);
    }

    Ok(ArgsPool {
      device: accel.clone(),
      allocated: AtomicUsize::new(arena.as_ptr() as usize),
      base: arena.try_into_boxed_slice()?,
    })
  }

  fn base(&self) -> &ArgsBox<[u8]> {
    &self.base
  }
  pub fn size(&self) -> usize { self.base().len() }

  fn start_byte(&self) -> usize { self.base().as_ptr() as usize }
  fn end_byte(&self) -> usize {
    self.start_byte() + self.size()
  }

  pub fn region(&self) -> &RegionAlloc {
    self.base.build_alloc()
  }

  /// Allocate a single `Args` block. Returns `None` when out of space.
  /// `args` must be Some. If allocation is successful, the returned
  /// pointer will be uninitialized.
  pub unsafe fn alloc<A>(&self) -> Option<Unique<A>>
    where A: Sized,
  {
    fn alignment_padding(size: usize, align: usize) -> usize {
      (align - (size - 1) % align) - 1
    }

    let layout = Layout::new::<A>()
      // force alignment to at least the cacheline size to avoid false
      // sharing.
      .align_to(128) // XXX hardcoded. could use this fact to avoid the loop below
      // TODO return an error here so users don't think it's OOM.
      .ok()?;

    let mut allocated_start = self.allocated.load(Ordering::Acquire);

    loop {
      let padding = alignment_padding(allocated_start,
                                      layout.align());

      let alloc_size = layout.size() + padding;

      if allocated_start + alloc_size > self.end_byte() {
        // no more space available, bail.
        return None;
      }

      match self.allocated.compare_exchange_weak(allocated_start,
                                                 allocated_start + alloc_size,
                                                 Ordering::SeqCst,
                                                 Ordering::Relaxed) {
        Ok(_) => {
          // ensure the start of the allocation is actually aligned
          allocated_start += padding;
        },
        Err(new_allocated_start) => {
          allocated_start = new_allocated_start;
          continue;
        }
      }

      let ptr: *mut A = transmute(allocated_start);
      return Some(Unique::new_unchecked(ptr));
    }
  }

  /// Reset the allocation ptr to the base. The mutable requirement ensures
  /// no device calls are in flight.
  pub fn wash(&mut self) {
    let base = self.base().as_ptr() as usize;
    *self.allocated.get_mut() = base;
  }
}
impl Clone for ArgsPool {
  fn clone(&self) -> Self {
    let size = self.size();
    ArgsPool::new_arena(&self.device, size)
      .expect("failed to clone ArgsPool")
  }
}

#[derive(Clone)]
pub struct ArgsPoolAlloc<P>(pub(super) P)
  where P: Deref<Target = ArgsPool>;
unsafe impl<P> AllocRef for ArgsPoolAlloc<P>
  where P: Deref<Target = ArgsPool>,
{
  fn alloc(&mut self, _layout: Layout, _init: AllocInit)
    -> Result<MemoryBlock, AllocErr>
  {
    Err(AllocErr)
  }
  unsafe fn dealloc(&mut self, _ptr: NonNull<u8>, _layout: Layout) {
    // no-op
  }

  unsafe fn grow(&mut self, _ptr: NonNull<u8>,
                 _layout: Layout, _new_size: usize,
                 _placement: ReallocPlacement,
                 _init: AllocInit)
    -> Result<MemoryBlock, AllocErr>
  {
    Err(AllocErr)
  }

  unsafe fn shrink(&mut self, ptr: NonNull<u8>,
                   _layout: Layout,
                   new_size: usize,
                   _placement: ReallocPlacement)
    -> Result<MemoryBlock, AllocErr>
  {
    Ok(MemoryBlock {
      ptr,
      size: new_size,
    })
  }
}
