
use std::alloc::Layout;
use std::fmt;
use std::mem::{transmute, size_of, };
use std::os::raw::c_void;
use std::ptr::NonNull;

use ApiContext;
use agent::{Agent};
use alloc::HsaAlloc;
use crate::error::Error;
use ffi;

#[cfg(feature = "alloc-wg")]
pub use super::region_alloc::*;

macro_rules! region_info {
  ($self:expr, $id:expr, $out:expr) => {
    {
      let mut out = $out;
      check_err!(ffi::hsa_region_get_info($self.0, $id,
                                          out.as_mut_ptr() as *mut _) => out)
    }
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Segment {
  Global,
  ReadOnly,
  Private,
  Group,
  KernelArg,
}
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct GlobalFlags(pub u32);
impl GlobalFlags {
  pub fn kernel_arg(&self) -> bool {
    (self.0 & ffi::hsa_region_global_flag_t_HSA_REGION_GLOBAL_FLAG_KERNARG) != 0
  }
  pub fn fine_grained(&self) -> bool {
    (self.0 & ffi::hsa_region_global_flag_t_HSA_REGION_GLOBAL_FLAG_FINE_GRAINED) != 0
  }
  pub fn coarse_grained(&self) -> bool {
    (self.0 & ffi::hsa_region_global_flag_t_HSA_REGION_GLOBAL_FLAG_COARSE_GRAINED) != 0
  }
}

impl fmt::Debug for GlobalFlags {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "GlobalFlags(")?;

    let mut first = true;
    let mut get_space = || {
      if first {
        first = false;
        ""
      } else {
        " "
      }
    };
    if self.kernel_arg() {
      write!(f, "{}kernel arg,", get_space())?;
    }
    if self.fine_grained() {
      write!(f, "{}fine grained,", get_space())?;
    }
    if self.coarse_grained() {
      write!(f, "{}course grained,", get_space())?;
    }

    write!(f, ")")
  }
}

/// TODO implement `std::alloc::Alloc`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Region(pub(crate) ffi::hsa_region_t, ApiContext);

impl Region {
  pub fn id(&self) -> u64 { self.0.handle }
  pub fn segment(&self) -> Result<Segment, Error> {
    let segment = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_SEGMENT,
                               [0u32; 1])?;
    match segment[0] {
      ffi::hsa_region_segment_t_HSA_REGION_SEGMENT_GLOBAL => Ok(Segment::Global),
      ffi::hsa_region_segment_t_HSA_REGION_SEGMENT_READONLY => Ok(Segment::ReadOnly),
      ffi::hsa_region_segment_t_HSA_REGION_SEGMENT_PRIVATE => Ok(Segment::Private),
      ffi::hsa_region_segment_t_HSA_REGION_SEGMENT_GROUP => Ok(Segment::Group),
      ffi::hsa_region_segment_t_HSA_REGION_SEGMENT_KERNARG => Ok(Segment::KernelArg),
      _ => Err(Error::General),
    }
  }

  // will return Ok(None) iff self is not a global segment.
  pub fn global_flags(&self) -> Result<Option<GlobalFlags>, Error> {
    match self.segment()? {
      Segment::Global => {},
      _ => { return Ok(None); },
    }
    let flags = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_GLOBAL_FLAGS,
                             [0u32; 1])?;
    Ok(Some(GlobalFlags(flags[0])))
  }
  /// XXX 32bit machine controlling a cluster with 64bit regions?!
  /// Probably won't ever be an issue, I bet.
  pub fn size(&self) -> Result<usize, Error> {
    let size = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_SIZE,
                            [0usize; 1])?;
    Ok(size[0])
  }
  pub fn alloc_max_size(&self) -> Result<usize, Error> {
    let size = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_ALLOC_MAX_SIZE,
                            [0usize; 1])?;
    Ok(size[0])
  }
  pub fn alloc_max_private_workgroup_size(&self) -> Result<u32, Error> {
    let size = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_ALLOC_MAX_PRIVATE_WORKGROUP_SIZE,
                            [0u32; 1])?;
    Ok(size[0])
  }
  pub fn runtime_alloc_allowed(&self) -> Result<bool, Error> {
    let b = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_RUNTIME_ALLOC_ALLOWED,
                         [false; 1])?;
    Ok(b[0])
  }
  pub fn runtime_alloc_granule(&self) -> Result<usize, Error> {
    let size = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_RUNTIME_ALLOC_GRANULE,
                            [0usize; 1])?;
    Ok(size[0])
  }
  pub fn runtime_alloc_alignment(&self) -> Result<usize, Error> {
    let size = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_RUNTIME_ALLOC_ALIGNMENT,
                            [0usize; 1])?;
    Ok(size[0])
  }
  /// All allocations will have alignment `runtime_alloc_alignment`.
  /// This is generally the page size.
  #[allow(unused_unsafe)]
  pub unsafe fn allocate<T>(&self, count: usize) -> Result<NonNull<T>, Error> {
    let bytes = size_of::<T>() * count;
    let mut ptr: *mut T = 0 as _;
    check_err!(ffi::hsa_memory_allocate(self.0, bytes,
                                        transmute(&mut ptr)))?;

    Ok(NonNull::new_unchecked(ptr))
  }
  #[allow(unused_unsafe)]
  pub unsafe fn deallocate<T>(&self, ptr: *mut T) -> Result<(), Error>  {
    // actually don't need `self`.
    check_err!(ffi::hsa_memory_free(ptr as *mut _))?;
    Ok(())
  }
}
unsafe impl HsaAlloc for Region {
  fn native_alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, Error> {
    if !self.runtime_alloc_allowed().unwrap_or_default() {
      return Err(Error::InvalidArgument);
    }

    let bytes = layout.size();
    if bytes == 0 {
      return Ok(layout.dangling());
    }

    debug_assert!({
      if let Ok(max_alignment) = self.runtime_alloc_alignment() {
        let align = layout.align();
        align <= max_alignment
      } else {
        true
      }
    }, "unexpected excessive alignment");

    let granule = self.runtime_alloc_granule()?;

    let len = ((bytes - 1) / granule + 1) * granule;

    let mut ptr: *mut u8 = 0 as _;
    check_err!(ffi::hsa_memory_allocate(self.0, len,
                                        transmute(&mut ptr)))
      .ok().ok_or(Error::InvalidAllocation)?;

    let agent_ptr = NonNull::new(ptr)
      .ok_or(Error::InvalidAllocation)?;

    Ok(agent_ptr)
  }

  fn native_alignment(&self) -> Result<usize, Error> {
    self.runtime_alloc_alignment()
  }

  unsafe fn native_dealloc(&mut self, ptr: NonNull<u8>, _: Layout) -> Result<(), Error> {
    self.deallocate(ptr.as_ptr())
  }
}

impl Agent {
  pub fn all_regions(&self) -> Result<Vec<Region>, Error> {
    extern "C" fn get(out: ffi::hsa_region_t,
                      items: *mut c_void) -> ffi::hsa_status_t {
      let items: &mut Vec<Region> = unsafe {
        transmute(items)
      };
      items.push(Region(out, ApiContext::upref()));
      ffi::hsa_status_t_HSA_STATUS_SUCCESS
    }

    let mut out: Vec<Region> = vec![];
    Ok(check_err!(ffi::hsa_agent_iterate_regions(self.0, Some(get),
                                                 transmute(&mut out)) => out)?)
  }
}
