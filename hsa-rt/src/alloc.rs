use std::alloc::Layout;
use std::convert::TryInto;
use std::fmt;
use std::ops::Deref;
use std::ptr::NonNull;

use error::Error;

pub enum NonNullAlignedPair<T: ?Sized> {
  Native(NonNull<T>),
  Realigned {
    aligned: NonNull<T>,
    unaligned_offset: i32,
  }
}

impl<T> NonNullAlignedPair<T>
  where T: Sized,
{
  unsafe fn unaligned_ptr(&self) -> NonNull<T> {
    match self {
      NonNullAlignedPair::Native(ptr) => *ptr,
      NonNullAlignedPair::Realigned {
        aligned,
        unaligned_offset,
      } => {
        let ptr = aligned.as_ptr();
        NonNull::new_unchecked(ptr.offset(*unaligned_offset as _))
      },
    }
  }
}
impl<T> Clone for NonNullAlignedPair<T> {
  #[inline(always)]
  fn clone(&self) -> Self { *self }
}
impl<T> Copy for NonNullAlignedPair<T> { }

impl<T: ?Sized> fmt::Debug for NonNullAlignedPair<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Pointer::fmt(&self.as_ptr(), f)
  }
}
impl<T: ?Sized> fmt::Pointer for NonNullAlignedPair<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Pointer::fmt(&self.as_ptr(), f)
  }
}
impl<T> Deref for NonNullAlignedPair<T>
  where T: ?Sized,
{
  type Target = NonNull<T>;
  fn deref(&self) -> &Self::Target {
    match self {
      NonNullAlignedPair::Native(ptr) => ptr,
      NonNullAlignedPair::Realigned {
        aligned, ..
      } => aligned,
    }
  }
}

/// An allocator for possibly non-visible memory.
pub unsafe trait HsaAlloc {
  /// Image data alignment requirements will often exceed than the native alignment of the
  /// underlying pools/regions. This function handles this case by allocating extra space,
  /// and realigning the resulting allocation.
  fn alloc(&mut self, layout: Layout) -> Result<NonNullAlignedPair<u8>, Error> {
    let native_aligment = self.native_alignment()?;
    let align = layout.align();
    // don't alloc if the size is zero.
    if layout.size() != 0 && align > native_aligment {
      // allocate extra space so we can realign the pointer:
      let size = layout.size() + align;
      let unaligned_layout = Layout::from_size_align(size, native_aligment)
        .ok().ok_or(Error::Overflow)?;
      let ptr = self.native_alloc(unaligned_layout)?.as_ptr() as usize;

      let aligned_ptr = (ptr + align - 1) & (!(align - 1));
      let offset = (-((aligned_ptr - ptr) as isize)).try_into()
        .ok().ok_or(Error::Overflow)?;
      let aligned = unsafe {
        NonNull::new_unchecked(aligned_ptr as *mut u8)
      };

      Ok(NonNullAlignedPair::Realigned {
        aligned,
        unaligned_offset: offset,
      })
    } else {
      // allocation alignment is < native alignment, allocate directly.
      let ptr = self.native_alloc(layout)?;
      Ok(NonNullAlignedPair::Native(ptr))
    }
  }

  unsafe fn dealloc(&mut self, ptr: NonNullAlignedPair<u8>, layout: Layout)
    -> Result<(), Error>
  {
    let ptr = ptr.unaligned_ptr();
    self.native_dealloc(ptr, layout)
  }

  fn native_alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, Error>;
  fn native_alignment(&self) -> Result<usize, Error>;
  unsafe fn native_dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) -> Result<(), Error>;
}
