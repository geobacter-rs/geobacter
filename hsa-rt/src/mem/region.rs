
use std::borrow::{BorrowMut, Borrow, };
use std::error::Error;
use std::fmt;
use std::marker::Unsize;
use std::mem::{transmute, size_of, };
use std::ops::{Deref, DerefMut, CoerceUnsized, };
use std::os::raw::c_void;
use std::ptr::NonNull;

use ApiContext;
use agent::{Agent};
use ffi;

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

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Region(pub(crate) ffi::hsa_region_t, ApiContext);

impl Region {
  pub fn id(&self) -> u64 { self.0.handle }
  pub fn segment(&self) -> Result<Segment, Box<Error>> {
    let segment = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_SEGMENT,
                               [0u32; 1])?;
    match segment[0] {
      ffi::hsa_region_segment_t_HSA_REGION_SEGMENT_GLOBAL => Ok(Segment::Global),
      ffi::hsa_region_segment_t_HSA_REGION_SEGMENT_READONLY => Ok(Segment::ReadOnly),
      ffi::hsa_region_segment_t_HSA_REGION_SEGMENT_PRIVATE => Ok(Segment::Private),
      ffi::hsa_region_segment_t_HSA_REGION_SEGMENT_GROUP => Ok(Segment::Group),
      ffi::hsa_region_segment_t_HSA_REGION_SEGMENT_KERNARG => Ok(Segment::KernelArg),
      _ => {
        return Ok(Err(format!("unknown segment enum value: {}", segment[0]))?);
      },
    }
  }

  // will return Ok(None) iff self is not a global segment.
  pub fn global_flags(&self) -> Result<Option<GlobalFlags>, Box<Error>> {
    match self.segment()? {
      Segment::Global => {},
      _ => { return Ok(None); },
    }
    let flags = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_GLOBAL_FLAGS,
                             [0u32; 1])?;
    Ok(Some(GlobalFlags(flags[0])))
  }
  /// XXX 32bit machine controlling a cluster with 64bit regions?!
  /// Probably won't ever be an issue, I guess.
  pub fn size(&self) -> Result<usize, Box<Error>> {
    let size = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_SIZE,
                            [0usize; 1])?;
    Ok(size[0])
  }
  pub fn alloc_max_size(&self) -> Result<usize, Box<Error>> {
    let size = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_ALLOC_MAX_SIZE,
                            [0usize; 1])?;
    Ok(size[0])
  }
  pub fn alloc_max_private_workgroup_size(&self) -> Result<u32, Box<Error>> {
    let size = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_ALLOC_MAX_PRIVATE_WORKGROUP_SIZE,
                            [0u32; 1])?;
    Ok(size[0])
  }
  pub fn runtime_alloc_allowed(&self) -> Result<bool, Box<Error>> {
    let b = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_RUNTIME_ALLOC_ALLOWED,
                         [false; 1])?;
    Ok(b[0])
  }
  pub fn runtime_alloc_granule(&self) -> Result<usize, Box<Error>> {
    let size = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_RUNTIME_ALLOC_GRANULE,
                            [0usize; 1])?;
    Ok(size[0])
  }
  pub fn runtime_alloc_alignment(&self) -> Result<usize, Box<Error>> {
    let size = region_info!(self, ffi::hsa_region_info_t_HSA_REGION_INFO_RUNTIME_ALLOC_ALIGNMENT,
                            [0usize; 1])?;
    Ok(size[0])
  }
  /// All allocations will have alignment `runtime_alloc_alignment`.
  /// This is generally the page size.
  #[allow(unused_unsafe)]
  pub unsafe fn allocate<T>(&self, count: usize) -> Result<NonNull<T>, Box<Error>> {
    assert_ne!(size_of::<T>(), 0, "can't allocate with a zero size");
    let bytes = size_of::<T>() * count;
    let mut ptr: *mut T = 0 as _;
    check_err!(ffi::hsa_memory_allocate(self.0, bytes,
                                        transmute(&mut ptr)))?;

    Ok(NonNull::new_unchecked(ptr))
  }
  #[allow(unused_unsafe)]
  pub unsafe fn deallocate<T>(&self, ptr: *mut T) -> Result<(), Box<Error>>  {
    // actually don't need `self`.
    check_err!(ffi::hsa_memory_free(ptr as *mut _))?;
    Ok(())
  }
}

impl Agent {
  pub fn all_regions(&self) -> Result<Vec<Region>, Box<Error>> {
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

pub struct RegionBox<T>
  where T: ?Sized,
{
  b: NonNull<T>,
}

impl<T> RegionBox<T> {
  pub fn new(region: &Region, v: T) -> Result<Self, Box<Error>> {
    let mut ptr = unsafe { region.allocate(1)? };
    unsafe { *ptr.as_mut() = v; }
    Ok(RegionBox {
      b: ptr,
    })
  }
  pub fn new_default(region: &Region) -> Result<Self, Box<Error>>
    where T: Default,
  {
    Self::new(region, Default::default())
  }

  pub fn clone_into(&self, region: &Region) -> Result<Self, Box<Error>>
    where T: Clone,
  {
    let v = unsafe { self.b.as_ref().clone() };
    Self::new(region, v)
  }
  pub unsafe fn into_raw(self) -> NonNull<T> {
    let ptr = self.b;
    ::std::mem::forget(self);
    ptr
  }
  pub unsafe fn from_raw(ptr: NonNull<T>) -> Self {
    RegionBox {
      b: ptr,
    }
  }
}
impl<T> Deref for RegionBox<T>
  where T: ?Sized,
{
  type Target = T;
  fn deref(&self) -> &T { unsafe { self.b.as_ref() } }
}
impl<T> DerefMut for RegionBox<T>
  where T: ?Sized,
{
  fn deref_mut(&mut self) -> &mut T { unsafe { self.b.as_mut() } }
}
impl<T> Drop for RegionBox<T>
  where T: ?Sized,
{
  fn drop(&mut self) {
    unsafe {
      let ptr = self.b.as_ptr();
      ::std::ptr::drop_in_place(ptr);
      ffi::hsa_memory_free(ptr as *mut _); // ignore result code.
    }
  }
}
impl<T> Borrow<T> for RegionBox<T>
  where T: ?Sized,
{
  fn borrow(&self) -> &T { unsafe { self.b.as_ref() } }
}
impl<T> BorrowMut<T> for RegionBox<T>
  where T: ?Sized,
{
  fn borrow_mut(&mut self) -> &mut T { unsafe { self.b.as_mut() } }
}
impl<T> AsRef<T> for RegionBox<T>
  where T: ?Sized,
{
  fn as_ref(&self) -> &T { unsafe { self.b.as_ref() } }
}
impl<T> AsMut<T> for RegionBox<T>
  where T: ?Sized,
{
  fn as_mut(&mut self) -> &mut T { unsafe { self.b.as_mut() } }
}
impl<T, U> CoerceUnsized<RegionBox<U>> for RegionBox<T>
  where T: ?Sized + Unsize<U>,
        U: ?Sized
{ }
