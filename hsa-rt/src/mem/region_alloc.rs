
use std::convert::*;
use std::ffi::c_void;
use std::ptr::NonNull;

use crate::alloc_wg::alloc::*;

use error::*;
use super::region::{Region, };

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct RegionAlloc {
  region: Region,
  granule: usize,
  alignment: usize,
}
impl RegionAlloc {
  pub fn alloc_allowed(&self) -> bool { true }
  pub fn alloc_granule(&self) -> usize { self.granule }
  pub fn alloc_alignment(&self) -> usize { self.alignment }
  pub fn region(&self) -> &Region { &self.region }
}

impl AllocRef for RegionAlloc {
  type Error = Error;

  fn alloc(&mut self, layout: NonZeroLayout)
    -> Result<NonNull<u8>, Self::Error>
  {
    let bytes = layout.size().get();
    let align = layout.align().get();
    if align > self.alignment {
      return Err(Error::InvalidAllocation);
    }

    let len = ((bytes - 1) / self.granule + 1) * self.granule;

    let mut dest = 0 as *mut c_void;
    let dest_ptr = &mut dest as *mut *mut c_void;
    check_err!(ffi::hsa_memory_allocate(self.region.0, len, dest_ptr))?;

    let agent_ptr = NonNull::new(dest as *mut u8)
      .ok_or(Error::General)?;
    Ok(agent_ptr)
  }

  #[inline]
  fn usable_size(&self, layout: NonZeroLayout) -> (usize, usize) {
    let len = layout.size().get();
    let min = ((len - 1) / self.granule + 0) * self.granule;
    let max = ((len - 1) / self.granule + 1) * self.granule;
    (min, max)
  }
}
impl DeallocRef for RegionAlloc {
  type BuildAlloc = RegionAlloc;
  fn get_build_alloc(&mut self) -> Self::BuildAlloc {
    self.clone()
  }
  unsafe fn dealloc(&mut self, ptr: NonNull<u8>,
                    _layout: NonZeroLayout) {
    self.region.deallocate(ptr.as_ptr())
      // Should errors be ignored instead of panicking?
      .expect("deallocation failure");
  }
}
impl BuildAllocRef for RegionAlloc {
  type Ref = RegionAlloc;
  unsafe fn build_alloc_ref(&mut self,
                            _ptr: NonNull<u8>,
                            _layout: Option<NonZeroLayout>)
    -> Self::Ref
  {
    self.clone()
  }
}
impl ReallocRef for RegionAlloc { }
impl Abort for RegionAlloc { }

impl TryFrom<Region> for RegionAlloc {
  type Error = Error;
  fn try_from(v: Region) -> Result<Self, Error> {
    if !v.runtime_alloc_allowed()? {
      return Err(Error::InvalidAllocation);
    }

    // This is only valid for fine grained kernel arguments allocations;
    // otherwise the user could safely construct a box to a device local
    // allocation, which would allow safe deref-ing, which would segfault.
    if let Some(flags) = v.global_flags()? {
      if !(flags.kernel_arg() && flags.fine_grained()) {
        return Err(Error::InvalidAllocation);
      }
    } else {
      return Err(Error::InvalidAllocation);
    }

    Ok(Self {
      granule: v.runtime_alloc_granule()?,
      alignment: v.runtime_alloc_alignment()?,
      region: v,
    })
  }
}