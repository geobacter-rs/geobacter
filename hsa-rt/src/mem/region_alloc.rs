
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

unsafe impl AllocRef for RegionAlloc {
  fn alloc(&mut self, layout: Layout, init: AllocInit)
    -> Result<MemoryBlock, AllocErr>
  {
    let bytes = layout.size();
    if bytes == 0 {
      return Ok(MemoryBlock {
        ptr: layout.dangling(),
        size: 0,
      });
    }

    let align = layout.align();
    if align > self.alignment {
      return Err(AllocErr);
    }

    let len = ((bytes - 1) / self.granule + 1) * self.granule;

    let mut dest = 0 as *mut c_void;
    let dest_ptr = &mut dest as *mut *mut c_void;
    check_err!(ffi::hsa_memory_allocate(self.region.0, len as _, dest_ptr))
      .ok().ok_or(AllocErr)?;

    let agent_ptr = NonNull::new(dest as *mut u8)
      .ok_or(AllocErr)?;

    let mut block = MemoryBlock {
      ptr: agent_ptr,
      size: bytes,
    };
    block.init(init);

    Ok(block)
  }
  unsafe fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
    if layout.size() != 0 {
      self.region.deallocate(ptr.as_ptr())
        // Should errors be ignored instead of panicking?
        .expect("deallocation failure");
    }
  }

  unsafe fn grow(&mut self, ptr: NonNull<u8>,
                 layout: Layout, new_size: usize,
                 placement: ReallocPlacement,
                 init: AllocInit)
    -> Result<MemoryBlock, AllocErr>
  {
    let len = layout.size();
    let mut memory = MemoryBlock {
      ptr,
      size: len,
    };
    let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
    if len != 0 {
      let max = ((len - 1) / self.granule + 1) * self.granule;
      if max >= new_size {
        // we can do in-place here
        memory.size = new_size;
        memory.init_offset(init, len);
        return Ok(memory);
      }
    }

    if let ReallocPlacement::InPlace = placement {
      return Err(AllocErr);
    }

    let mut new_memory = self.alloc(new_layout,
                                    AllocInit::Uninitialized)?;

    if len != 0 {
      std::ptr::copy_nonoverlapping(memory.ptr.as_ptr(),
                                    new_memory.ptr.as_ptr(),
                                    len);
    }
    new_memory.init_offset(init, len);
    self.dealloc(memory.ptr, layout);

    Ok(new_memory)
  }

  unsafe fn shrink(&mut self, ptr: NonNull<u8>,
                   layout: Layout,
                   new_size: usize,
                   placement: ReallocPlacement)
    -> Result<MemoryBlock, AllocErr>
  {
    let min = || ((layout.size() - 1) / self.granule + 0) * self.granule;
    if ReallocPlacement::InPlace == placement || layout.size() == 0 || new_size >= min() {
      // we can do in-place here, possibly wasting some space
      return Ok(MemoryBlock {
        ptr,
        size: new_size,
      });
    }

    let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
    let new_memory = self.alloc(new_layout,
                                AllocInit::Uninitialized)?;
    if new_size != 0 {
      std::ptr::copy_nonoverlapping(ptr.as_ptr(),
                                    new_memory.ptr.as_ptr(),
                                    new_size);
    }

    self.dealloc(ptr, layout);

    Ok(new_memory)
  }
}

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