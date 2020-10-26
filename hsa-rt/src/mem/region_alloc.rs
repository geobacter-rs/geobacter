use std::alloc::*;
use std::convert::*;
use std::ffi::c_void;
use std::ptr::{copy_nonoverlapping, NonNull, null_mut};
use std::slice::from_raw_parts;

use error::*;

use super::region::Region;

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
  fn alloc(&self, layout: Layout)
    -> Result<NonNull<[u8]>, AllocError>
  {
    unsafe {
      let bytes = layout.size();
      if bytes == 0 {
        let ptr = layout.dangling();
        let ptr = from_raw_parts(ptr.as_ptr(), 0);
        return Ok(NonNull::from(ptr));
      }

      let size = layout.size();
      let align = layout.align();
      let layout = if align > self.alignment {
        layout.pad_to_align()
      } else {
        layout
      };

      let granule_padding = layout.padding_needed_for(self.granule);
      let len = layout.size().wrapping_add(granule_padding);

      let mut dest = 0 as *mut c_void;
      let dest_ptr = &mut dest as *mut *mut c_void;
      check_err!(ffi::hsa_memory_allocate(self.region.0, len as _, dest_ptr))
        .ok().ok_or(AllocError)?;

      if dest == null_mut() { return Err(AllocError); }

      // now align the pointer
      if align > self.alignment {
        let dest_ptr = dest as usize;

        let aligned = dest_ptr.wrapping_add(align).wrapping_sub(1) & !align.wrapping_sub(1);
        assert!(aligned + size <= len);
        dest = aligned as *mut c_void;
      }

      let s = from_raw_parts(dest as *mut u8 as *const u8, size);
      Ok(NonNull::from(s))
    }
  }
  unsafe fn dealloc(&self, ptr: NonNull<u8>, layout: Layout) {
    if layout.size() != 0 {
      self.region.deallocate(ptr.as_ptr())
        // Should errors be ignored instead of panicking?
        .expect("deallocation failure");
    }
  }

  unsafe fn grow(&self,
                 ptr: NonNull<u8>,
                 old_layout: Layout,
                 new_layout: Layout)
    -> Result<NonNull<[u8]>, AllocError>
  {
    let new_size = new_layout.size();
    let old_size = old_layout.size();
    if new_size < old_size {
      return Err(AllocError);
    }
    if old_layout.align() < new_layout.align() {
      return Err(AllocError);
    }
    if old_size != 0 {
      let inplace_layout = old_layout
        .align_to(self.granule)
        .map_err(|_| AllocError)?
        .pad_to_align();
      if new_layout.size() <= inplace_layout.size() {
        // we can do it in-place here
        let out_ptr = from_raw_parts(ptr.as_ptr(), new_size);
        let out_ptr = NonNull::from(out_ptr);
        return Ok(out_ptr);
      }
    }

    let new_ptr = self.alloc(new_layout)?;
    // SAFETY: because `new_layout.size()` must be greater than or equal to
    // `old_layout.size()`, both the old and new memory allocation are valid for reads and
    // writes for `old_layout.size()` bytes. Also, because the old allocation wasn't yet
    // deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
    // safe. The safety contract for `dealloc` must be upheld by the caller.
    copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_mut_ptr(),
                        old_size);
    self.dealloc(ptr, old_layout);
    Ok(new_ptr)
  }
  unsafe fn grow_zeroed(&self,
                        ptr: NonNull<u8>,
                        old_layout: Layout,
                        new_layout: Layout)
    -> Result<NonNull<[u8]>, AllocError>
  {
    let mut new_ptr = self.grow(ptr, old_layout, new_layout)?;
    let t = &mut new_ptr.as_mut()[old_layout.size()..new_layout.size()];
    t.as_mut_ptr()
      .write_bytes(0, t.len());
    Ok(new_ptr)
  }

  unsafe fn shrink(&self,
                   ptr: NonNull<u8>,
                   old_layout: Layout,
                   new_layout: Layout)
    -> Result<NonNull<[u8]>, AllocError>
  {
    let old_size = old_layout.size();
    let new_size = new_layout.size();
    if new_size > old_size {
      return Err(AllocError);
    }
    if old_layout.align() < new_layout.align() {
      return Err(AllocError);
    }
    if new_size == 0 {
      self.dealloc(ptr, old_layout);
      return self.alloc(new_layout);
    }

    let min = old_size & !self.granule;
    if new_size >= min {
      // shrink by doing nothing
      let ptr = from_raw_parts(ptr.as_ptr(), new_size);
      return Ok(NonNull::from(ptr));
    }

    // shrink by reallocating
    let new_ptr = self.alloc(new_layout)?;
    copy_nonoverlapping(ptr.as_ptr(),
                        new_ptr.as_non_null_ptr().as_ptr(),
                        new_size);
    self.dealloc(ptr, old_layout);

    Ok(new_ptr)
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