
//! Note none of this work on hosts: any access to memory allocated
//! from these pools will always segfault.
//! You AFAICT *must* use memory locking for transferring in and out.

#![allow(deprecated)]

use std::alloc::*;
use std::ffi::c_void;
use std::marker::Unsize;
use std::mem::{size_of, size_of_val, transmute};
use std::ops::CoerceUnsized;
use std::ptr::{self, copy_nonoverlapping, NonNull, null_mut,
               slice_from_raw_parts_mut};
use std::slice::from_raw_parts;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize, };

use {agent::Agent, agent::DeviceType, ApiContext, error::Error, };
use alloc::HsaAlloc;
use ffi;
pub use mem::region::{GlobalFlags, Segment, };
use signal::SignalRef;
use utils::uninit;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct MemoryPool(ffi::hsa_amd_memory_pool_t);
impl Agent {
  pub fn amd_memory_pools(&self) -> Result<Vec<MemoryPool>, Error> {
    extern "C" fn get_pool(pool: ffi::hsa_amd_memory_pool_t,
                            pools: *mut c_void) -> ffi::hsa_status_t {
      let pools: &mut Vec<MemoryPool> = unsafe {
        transmute(pools)
      };
      pools.push(MemoryPool(pool));
      ffi::hsa_status_t_HSA_STATUS_SUCCESS
    }

    let mut out: Vec<MemoryPool> = vec![];
    Ok(check_err!(ffi::hsa_amd_agent_iterate_memory_pools(self.0, Some(get_pool), transmute(&mut out)) => out)?)
  }
}

macro_rules! pool_agent_info {
  ($self:expr, $agent:expr, $id:expr, $out:ty) => {
    {
      let mut out: [$out; 1] = [Default::default(); 1];
      check_err!(ffi::hsa_amd_agent_memory_pool_get_info($agent.0, $self.0,
                                                         $id, out.as_mut_ptr() as *mut _) => out[0])
    }
  }
}
macro_rules! pool_info {
  ($self:expr, $id:expr, $out:ty) => {
    {
      let mut out: [$out; 1] = [Default::default(); 1];
      check_err!(ffi::hsa_amd_memory_pool_get_info($self.0, $id,
                 out.as_mut_ptr() as *mut _) => out[0])
    }
  }
}


#[derive(Clone, Copy, Eq, PartialEq, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum AgentAccess {
  Never,
  DefaultDisallowed,
  DefaultAllowed,
}
impl AgentAccess {
  pub fn never_allowed(&self) -> bool {
    match self {
      AgentAccess::Never => true,
      _ => false,
    }
  }
  pub fn default_allowed(&self) -> bool {
    match self {
      AgentAccess::DefaultAllowed => true,
      _ => false,
    }
  }
  pub fn default_disallowed(&self) -> bool {
    match self {
      AgentAccess::DefaultDisallowed => true,
      _ => false,
    }
  }
}

impl MemoryPool {
  pub fn id(&self) -> u64 { self.0.handle }
  pub fn agent_access(&self, agent: &Agent)
    -> Result<AgentAccess, Error>
  {
    let attr = ffi::hsa_amd_agent_memory_pool_info_t_HSA_AMD_AGENT_MEMORY_POOL_INFO_ACCESS;
    let out = pool_agent_info!(self, agent, attr, ffi::hsa_amd_memory_pool_access_t)?;
    let out = match out {
      ffi::hsa_amd_memory_pool_access_t_HSA_AMD_MEMORY_POOL_ACCESS_ALLOWED_BY_DEFAULT => {
        AgentAccess::DefaultAllowed
      },
      ffi::hsa_amd_memory_pool_access_t_HSA_AMD_MEMORY_POOL_ACCESS_DISALLOWED_BY_DEFAULT => {
        AgentAccess::DefaultDisallowed
      },
      ffi::hsa_amd_memory_pool_access_t_HSA_AMD_MEMORY_POOL_ACCESS_NEVER_ALLOWED => {
        AgentAccess::Never
      },
      _ => {
        return Err(Error::General);
      },
    };

    Ok(out)
  }
  pub fn global_flags(&self) -> Result<Option<GlobalFlags>, Error> {
    let attr = ffi::hsa_amd_memory_pool_info_t_HSA_AMD_MEMORY_POOL_INFO_GLOBAL_FLAGS;
    match self.segment()? {
      Segment::Global => Ok(Some(GlobalFlags(pool_info!(self, attr, u32)?))),
      _ => Ok(None),
    }
  }
  pub fn alloc_alignment(&self) -> Result<Option<usize>, Error> {
    let attr = ffi::hsa_amd_memory_pool_info_t_HSA_AMD_MEMORY_POOL_INFO_RUNTIME_ALLOC_ALIGNMENT;
    if self.alloc_allowed() {
      Ok(Some(pool_info!(self, attr, usize)?))
    } else {
      Ok(None)
    }
  }
  pub fn alloc_allowed(&self) -> bool {
    let attr = ffi::hsa_amd_memory_pool_info_t_HSA_AMD_MEMORY_POOL_INFO_RUNTIME_ALLOC_ALLOWED;
    pool_info!(self, attr, bool)
      .unwrap_or_default()
  }
  pub fn alloc_granule(&self) -> Result<Option<usize>, Error> {
    let attr = ffi::hsa_amd_memory_pool_info_t_HSA_AMD_MEMORY_POOL_INFO_RUNTIME_ALLOC_GRANULE;
    if self.alloc_allowed() {
      Ok(Some(pool_info!(self, attr, usize)?))
    } else {
      Ok(None)
    }
  }
  pub fn segment(&self) -> Result<Segment, Error> {
    let attr = ffi::hsa_amd_memory_pool_info_t_HSA_AMD_MEMORY_POOL_INFO_SEGMENT;
    let out = pool_info!(self, attr, ffi::hsa_amd_segment_t)?;
    let out = match out {
      ffi::hsa_amd_segment_t_HSA_AMD_SEGMENT_GLOBAL => Segment::Global,
      ffi::hsa_amd_segment_t_HSA_AMD_SEGMENT_GROUP => Segment::Group,
      ffi::hsa_amd_segment_t_HSA_AMD_SEGMENT_PRIVATE => Segment::Private,
      ffi::hsa_amd_segment_t_HSA_AMD_SEGMENT_READONLY => Segment::ReadOnly,
      _ => {
        return Err(Error::General);
      },
    };

    Ok(out)
  }
  pub fn total_size(&self) -> Result<usize, Error> {
    let attr = ffi::hsa_amd_memory_pool_info_t_HSA_AMD_MEMORY_POOL_INFO_SIZE;
    Ok(pool_info!(self, attr, usize)?)
  }

  pub fn is_migratable_to(&self, to: &MemoryPool) -> Result<bool, Error> {
    let mut out = [false; 1];
    Ok(check_err!(ffi::hsa_amd_memory_pool_can_migrate(self.0, to.0,
                  out.as_mut_ptr() as *mut _) => out[0])?)
  }

  /// Unsafe because you must ensure the local processor can natively access
  /// this pool! So use in alloc_wg::boxed::Box won't be safe for device memory
  /// when running on the host, *unless* you make sure the host code *never*
  /// dereferences the box.
  #[cfg(feature = "alloc-wg")]
  pub unsafe fn allocator(&self) -> Result<MemoryPoolAlloc, Error> {
    if !self.alloc_allowed() {
      return Err(Error::General);
    }

    let granule = self.alloc_granule()?.unwrap();
    let alignment = self.alloc_alignment()?.unwrap();
    Ok(MemoryPoolAlloc {
      pool: self.clone(),
      granule,
      alignment,
    })
  }

  /// This allocation will always be aligned to `self.alloc_alignment()`. No
  /// other alignment is possible, sorry.
  pub fn alloc_in_pool(&self, bytes: usize) -> Result<MemoryPoolPtr<[u8]>, Error> {
    ApiContext::check_live()
      .map_err(|_| Error::NotInitialized )?;
    if !self.alloc_allowed() {
      return Err(Error::InvalidAllocation);
    }
    let min = self.alloc_granule()?.unwrap_or_default();
    let len = ::std::cmp::max(min, bytes);

    let mut dest = 0 as *mut c_void;
    let dest_ptr = &mut dest as *mut *mut c_void;
    check_err!(ffi::hsa_amd_memory_pool_allocate(self.0, len as _,
                                                 0, dest_ptr))?;

    let agent_ptr = slice_from_raw_parts_mut(dest as *mut u8, len);
    let agent_ptr = NonNull::new(agent_ptr)
      .ok_or(Error::General)?;

    Ok(MemoryPoolPtr(agent_ptr, *self))
  }
  pub unsafe fn dealloc_from_pool<T>(&self, ptr: NonNull<T>)
    -> Result<(), Error>
    where T: ?Sized,
  {
    Ok(check_err!(ffi::hsa_amd_memory_pool_free(ptr.as_ptr() as *mut c_void) => ())?)
  }

  /// Since the HSA API doesn't require the pool handle to dealloc, this is here
  /// so you don't have to have it either.
  pub unsafe fn global_dealloc<T>(ptr: NonNull<T>) -> Result<(), Error>
    where T: ?Sized,
  {
    Ok(check_err!(ffi::hsa_amd_memory_pool_free(ptr.as_ptr() as *mut c_void) => ())?)
  }

  /// `self` must be CPU accessible. This is not checked.
  pub unsafe fn lock<T>(&self, host_ptr: NonNull<T>, count: usize, agents: &[Agent])
    -> Result<MemoryPoolPtr<[T]>, Error>
    where T: Sized,
  {
    let agents_len = agents.len();
    let agents_ptr = if agents_len != 0 {
      agents.as_ptr() as *mut ffi::hsa_agent_t
    } else {
      ptr::null_mut()
    };

    let bytes = size_of::<T>() * count;

    log::trace!("locking {} bytes to pool {:?}", bytes, self);

    let mut agent_ptr: *mut T = ptr::null_mut();
    {
      check_err!(ffi::hsa_amd_memory_lock_to_pool(host_ptr.as_ptr() as *mut _,
                                                  bytes as _, agents_ptr,
                                                  agents_len as _,
                                                  self.0, 0,
                                                  transmute(&mut agent_ptr)))?;
    }

    let agent_ptr = slice_from_raw_parts_mut(agent_ptr, bytes);
    let agent_ptr = NonNull::new(agent_ptr)
      .ok_or(Error::General)?;

    Ok(MemoryPoolPtr(agent_ptr, *self))
  }
}

unsafe impl HsaAlloc for MemoryPool {
  fn native_alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, Error> {
    let bytes = layout.size();
    if bytes == 0 {
      return Ok(layout.dangling());
    }

    debug_assert!({
      if let Ok(Some(max_alignment)) = self.alloc_alignment() {
        let align = layout.align();
        align <= max_alignment
      } else {
        true
      }
    }, "unexpected excessive alignment");

    let granule = self.alloc_granule()?.unwrap();

    let len = ((bytes - 1) / granule + 1) * granule;

    let mut dest = 0 as *mut c_void;
    let dest_ptr = &mut dest as *mut *mut c_void;
    check_err!(ffi::hsa_amd_memory_pool_allocate(self.0, len as _, 0, dest_ptr))
      .ok().ok_or(Error::InvalidAllocation)?;

    let agent_ptr = NonNull::new(dest as *mut u8)
      .ok_or(Error::InvalidAllocation)?;

    Ok(agent_ptr)
  }

  fn native_alignment(&self) -> Result<usize, Error> {
    Ok(self.alloc_alignment()?.unwrap_or(1))
  }

  unsafe fn native_dealloc(&mut self, ptr: NonNull<u8>, _: Layout) -> Result<(), Error> {
    self.dealloc_from_pool(ptr)
  }
}

#[cfg(feature = "alloc-wg")]
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct MemoryPoolAlloc {
  pool: MemoryPool,
  granule: usize,
  alignment: usize,
}
#[cfg(feature = "alloc-wg")]
impl MemoryPoolAlloc {
  pub fn alloc_allowed(&self) -> bool { true }
  pub fn alloc_granule(&self) -> usize { self.granule }
  pub fn alloc_alignment(&self) -> usize { self.alignment }
  pub fn pool(&self) -> MemoryPool { self.pool }
}

#[cfg(feature = "alloc-wg")]
unsafe impl AllocRef for MemoryPoolAlloc {
  fn alloc(&mut self, layout: Layout)
    -> Result<NonNull<[u8]>, AllocErr>
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
      check_err!(ffi::hsa_amd_memory_pool_allocate(self.pool.0, len as _,
                                                   0, dest_ptr))
        .ok().ok_or(AllocErr)?;

      if dest == null_mut() { return Err(AllocErr); }

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
  unsafe fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
    if layout.size() != 0 {
      self.pool.dealloc_from_pool(ptr)
        // Should errors be ignored instead of panicking?
        .expect("deallocation failure");
    }
  }
  unsafe fn grow(&mut self, ptr: NonNull<u8>,
                 old_layout: Layout, new_layout: Layout)
                 -> Result<NonNull<[u8]>, AllocErr>
  {
    let new_size = new_layout.size();
    let old_size = old_layout.size();
    if new_size < old_size {
      return Err(AllocErr);
    }
    if old_layout.align() < new_layout.align() {
      return Err(AllocErr);
    }
    if old_size != 0 {
      let inplace_layout = old_layout
        .align_to(self.granule)
        .map_err(|_| AllocErr )?
        .pad_to_align();
      if new_layout.size() <= inplace_layout.size() {
        // we can do it in-place here
        let out_ptr = from_raw_parts(ptr.as_ptr(), new_size);
        let out_ptr = NonNull::from(out_ptr);
        return Ok(out_ptr);
      }
    }

    let new_ptr = AllocRef::alloc(self, new_layout)?;
    // SAFETY: because `new_layout.size()` must be greater than or equal to
    // `old_layout.size()`, both the old and new memory allocation are valid for reads and
    // writes for `old_layout.size()` bytes. Also, because the old allocation wasn't yet
    // deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
    // safe. The safety contract for `dealloc` must be upheld by the caller.
    copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_mut_ptr(),
                        old_size);
    AllocRef::dealloc(self, ptr, old_layout);
    Ok(new_ptr)
  }
  unsafe fn grow_zeroed(&mut self,
                        ptr: NonNull<u8>,
                        old_layout: Layout,
                        new_layout: Layout)
                        -> Result<NonNull<[u8]>, AllocErr>
  {
    let mut new_ptr = self.grow(ptr, old_layout, new_layout)?;
    let t = &mut new_ptr.as_mut()[old_layout.size()..new_layout.size()];
    t.as_mut_ptr()
      .write_bytes(0, t.len());
    Ok(new_ptr)
  }

  unsafe fn shrink(&mut self, ptr: NonNull<u8>,
                   old_layout: Layout,
                   new_layout: Layout)
                   -> Result<NonNull<[u8]>, AllocErr>
  {
    let old_size = old_layout.size();
    let new_size = new_layout.size();
    if new_size > old_size {
      return Err(AllocErr);
    }
    if old_layout.align() < new_layout.align() {
      return Err(AllocErr);
    }
    if new_size == 0 {
      AllocRef::dealloc(self, ptr, old_layout);
      return AllocRef::alloc(self, new_layout);
    }

    let min = old_size & !self.granule;
    if new_size >= min {
      // shrink by doing nothing
      let ptr = from_raw_parts(ptr.as_ptr(), new_size);
      return Ok(NonNull::from(ptr));
    }

    // shrink by reallocating
    let new_ptr = AllocRef::alloc(self, new_layout)?;
    copy_nonoverlapping(ptr.as_ptr(),
                        new_ptr.as_non_null_ptr().as_ptr(),
                        new_size);
    AllocRef::dealloc(self, ptr, old_layout);

    Ok(new_ptr)
  }
}
#[cfg(feature = "alloc-wg")]
unsafe impl HsaAlloc for MemoryPoolAlloc {
  fn native_alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, Error> {
    let bytes = layout.size();
    if bytes == 0 {
      return Ok(layout.dangling());
    }

    debug_assert!(layout.align() <= self.alloc_alignment(),
                  "unexpected excessive alignment");

    let granule = self.alloc_granule();

    let len = ((bytes - 1) / granule + 1) * granule;

    let mut dest = 0 as *mut c_void;
    let dest_ptr = &mut dest as *mut *mut c_void;
    check_err!(ffi::hsa_amd_memory_pool_allocate(self.pool.0, len as _, 0, dest_ptr))
      .ok().ok_or(Error::InvalidAllocation)?;

    let agent_ptr = NonNull::new(dest as *mut u8)
      .ok_or(Error::InvalidAllocation)?;

    Ok(agent_ptr)
  }

  #[inline(always)]
  fn native_alignment(&self) -> Result<usize, Error> {
    Ok(self.alloc_alignment())
  }

  unsafe fn native_dealloc(&mut self, ptr: NonNull<u8>, _: Layout) -> Result<(), Error> {
    self.pool.dealloc_from_pool(ptr)
  }
}
#[cfg(feature = "alloc-wg")]
impl ::std::ops::Deref for MemoryPoolAlloc {
  type Target = MemoryPool;
  fn deref(&self) -> &MemoryPool { &self.pool }
}

/// Note: we deliberately do not offer ergonomic access to the value
/// stored. It is possible for this pointer to alias other memory.
#[derive(Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct MemoryPoolPtr<T>(NonNull<T>, MemoryPool)
  where T: ?Sized;

impl<T> MemoryPoolPtr<T>
  where T: ?Sized,
{
  pub fn as_ptr(&self) -> NonNull<T> { self.0 }

  pub const fn cast<U>(self) -> MemoryPoolPtr<U> {
    MemoryPoolPtr(self.0.cast(), self.1)
  }

  /// doesn't check that the ptr is actually from this pool
  pub unsafe fn from_ptr(pool: MemoryPool, ptr: NonNull<T>) -> Self {
    MemoryPoolPtr(ptr, pool)
  }

  pub fn into_bytes(self) -> MemoryPoolPtr<[u8]> {
    let bytes = size_of_val(unsafe {
      self.0.as_ref()
    });
    let ptr = slice_from_raw_parts_mut(self.0.as_ptr() as *mut u8,
                                       bytes);

    MemoryPoolPtr(unsafe {
      NonNull::new_unchecked(ptr)
    }, self.1)
  }

  pub fn pool(&self) -> &MemoryPool { &self.1 }

  pub unsafe fn dealloc(self) -> Result<(), Error> {
    self.1.dealloc_from_pool(self.0)
  }

  /// Modifies the physical address of the pointer. The virtual address is
  /// left alone.
  ///
  /// XXX *This is actually unimplmented in the AMD HSA runtime*. Never succeeds.
  pub fn move_to_pool(&mut self, to: &MemoryPool) -> Result<(), Error> {
    check_err!(ffi::hsa_amd_memory_migrate(self.0.as_ptr() as *const _,
                                           to.0, 0) => ())?;

    self.1 = *to;
    Ok(())
  }

  /// Grants access to this ptr to a single agent.
  pub fn grant_agent_access(&self, agent: &Agent) -> Result<(), Error> {
    let agents = [agent.0; 1];

    let agents_len = agents.len();
    let agents_ptr = agents.as_ptr();

    check_err!(ffi::hsa_amd_agents_allow_access(agents_len as _,
                                                agents_ptr,
                                                0 as *const _,
                                                self.0.as_ptr() as *const _) => ())?;
    Ok(())
  }
  pub fn grant_agents_access(&self, agents: &[Agent]) -> Result<(), Error> {
    assert_eq!(size_of::<Agent>(), size_of::<ffi::hsa_agent_t>(),
               "Agent wrapper type has extra padding");

    let agents_len = agents.len();
    let agents_ptr = agents.as_ptr() as *const ffi::hsa_agent_t;

    check_err!(ffi::hsa_amd_agents_allow_access(agents_len as _,
                                                agents_ptr,
                                                0 as *const _,
                                                self.0.as_ptr() as *const _) => ())?;
    Ok(())
  }
}
impl<T> MemoryPoolPtr<[T]>
  where T: Sized,
{
  /// Contrary to what you may think, this is completely safe to call
  /// from *any* agent, due to the way that Rust unsized types work.
  /// Unsized pointers are really a pair of pointer sized *values*;
  /// reading the length of a slice pointer or reference just reads
  /// that second value in the pair.
  pub fn len(&self) -> usize {
    unsafe { self.0.as_ref().len() }
  }

  pub fn into_ptr(self) -> MemoryPoolPtr<T> {
    MemoryPoolPtr(unsafe {
      NonNull::new_unchecked(self.0.as_ptr() as *mut T)
    }, self.1)
  }
}
impl<T> Clone for MemoryPoolPtr<T>
  where T: ?Sized,
{
  fn clone(&self) -> Self { *self }
}
impl<T> Copy for MemoryPoolPtr<T>
  where T: ?Sized,
{ }
impl<T, U> CoerceUnsized<MemoryPoolPtr<U>> for MemoryPoolPtr<T>
  where T: Unsize<U> + ?Sized,
        U: ?Sized,
{ }

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum PtrType {
  Other,
  Hsa,
  Graphics,
  Ipc,
  Locked,
}
impl PtrType {
  fn from_ffi(v: ffi::hsa_amd_pointer_type_t) -> Self {
    match v {
      ffi::hsa_amd_pointer_type_t_HSA_EXT_POINTER_TYPE_GRAPHICS =>
        PtrType::Graphics,
      ffi::hsa_amd_pointer_type_t_HSA_EXT_POINTER_TYPE_HSA =>
        PtrType::Hsa,
      ffi::hsa_amd_pointer_type_t_HSA_EXT_POINTER_TYPE_IPC =>
        PtrType::Ipc,
      ffi::hsa_amd_pointer_type_t_HSA_EXT_POINTER_TYPE_LOCKED =>
        PtrType::Locked,
      _ => PtrType::Other,
    }
  }
}
pub struct PtrInfo<T>
  where T: ?Sized,
{
  pub ty: PtrType,
  pub agent_base_addr: *mut T,
  pub host_base_addr: *mut T,
  pub size: usize,
  // TODO should this even be provided?
  //user_data: Option<&'static dyn Any + Send + Sync>,
  pub owner: Agent,
  accessible_by: Vec<Agent>,
}

impl<T> PtrInfo<T>
  where T: ?Sized,
{
  pub fn accessible_by_agent(&self, agent: &Agent) -> bool {
    match self.ty {
      PtrType::Locked => {
        if let Ok(DeviceType::Cpu) = agent.device_type() {
          // these agents won't be included in our list.
          return true;
        }
      },
      _ => { },
    }

    self.accessible_by.binary_search(agent).is_ok()
  }

  /// Note: if `self.ty` is `PtrType::Locked`, this won't include
  /// Cpu agents. Will be sorted.
  pub fn accessible_by(&self) -> &[Agent] {
    self.accessible_by.as_ref()
  }
}

pub trait QueryPtrInfo<T> {
  fn as_alloc_ptr(&self) -> *const c_void;
  fn query_info(&self, ret_accessible: bool) -> Result<PtrInfo<T>, Error> {
    unsafe extern "C" fn alloc_accessible(count: ffi::size_t) -> *mut c_void {
      use std::mem::{forget, };

      assert_eq!(size_of::<Agent>(), size_of::<ffi::hsa_agent_t>(),
                 "Agent wrapper type has extra padding");

      let mut alloc: Vec<Agent> = Vec::with_capacity(count as _);

      let alloc_ptr = alloc.as_mut_ptr() as *mut ffi::hsa_agent_t;
      forget(alloc);
      alloc_ptr as *mut _
    }

    let mut info: [ffi::hsa_amd_pointer_info_t; 1] = [unsafe {
      uninit()
    }; 1];
    info[0].size = size_of::<ffi::hsa_amd_pointer_info_t>() as _;
    let alloc = if ret_accessible {
      Some(alloc_accessible as _)
    } else {
      None
    };
    let mut num_agents = 0u32;
    let mut agents = 0 as *mut ffi::hsa_agent_t;

    let num_agents_ptr = if ret_accessible {
      &mut num_agents as *mut _
    } else {
      0 as *mut _
    };
    let agents_ptr = if ret_accessible {
      &mut agents as *mut *mut _
    } else {
      0 as *mut _
    };

    check_err!(ffi::hsa_amd_pointer_info(self.as_alloc_ptr() as *mut _,
                                         info.as_mut_ptr(),
                                         alloc,
                                         num_agents_ptr,
                                         agents_ptr))?;
    let info = info[0];
    let mut accessible_by = if ret_accessible {
      unsafe {
        Vec::from_raw_parts(agents as *mut Agent,
                            num_agents as usize,
                            num_agents as usize)
      }
    } else {
      Vec::new()
    };

    accessible_by.sort_unstable();

    let info = PtrInfo {
      ty: PtrType::from_ffi(info.type_),
      agent_base_addr: info.agentBaseAddress as *mut _,
      host_base_addr: info.hostBaseAddress as *mut _,
      size: info.sizeInBytes as _,
      owner: Agent(info.agentOwner, ApiContext::upref()),
      accessible_by,
    };

    Ok(info)
  }
  fn accessible_info(&self) -> Result<PtrInfo<T>, Error> {
    self.query_info(true)
  }
  fn info(&self) -> Result<PtrInfo<T>, Error> {
    self.query_info(false)
  }
}

impl<T> QueryPtrInfo<T> for MemoryPoolPtr<T> {
  fn as_alloc_ptr(&self) -> *const c_void { self.0.as_ptr() as *const _ }
}

pub unsafe fn async_copy(dst: MemoryPoolPtr<[u8]>,
                         src: MemoryPoolPtr<[u8]>,
                         bytes: usize,

                         dst_agent: &Agent,
                         src_agent: &Agent,

                         deps: &[SignalRef],
                         completion: SignalRef)
  -> Result<(), Error>
{
  let dst_ptr = dst.0.as_ptr() as *mut _;
  let src_ptr = src.0.as_ptr() as *const _;

  log::trace!("enqueuing async dma copy ({} bytes)", bytes);

  let deps: Vec<_> = deps
    .iter()
    .map(|dep| dep.0 )
    .collect();

  let deps_len = deps.len() as _;
  let deps_ptr = if deps_len != 0 {
    deps.as_ptr()
  } else {
    ptr::null()
  };

  check_err!(ffi::hsa_amd_memory_async_copy(dst_ptr, dst_agent.0,
                                            src_ptr, src_agent.0,
                                            bytes as _,

                                            deps_len, deps_ptr,
                                            completion.0) => ())?;
  Ok(())
}

pub fn lock_nullable_ptr<T>(ptr: *mut T, count: usize,
                            agents: &[Agent])
  -> Result<*mut T, Error>
  where T: Sized,
{
  if let Some(ptr) = NonNull::new(ptr) {
    Ok(lock_memory(ptr, count, agents)?.as_ptr())
  } else {
    Ok(ptr)
  }
}

/// Locks the specified pointer in host memory and returns the
/// agent local pointer. The memory is locked globally if no agents
/// are given.
///
/// It is perfectly valid to lock overlapping regions repeatedly.
/// Note however the agent pointers will be different each time
/// (unless the agents all support the full profile).
pub fn lock_memory<T>(ptr: NonNull<T>, count: usize,
                      agents: &[Agent])
  -> Result<NonNull<T>, Error>
  where T: Sized,
{
  debug_assert_eq!(size_of::<Agent>(), size_of::<ffi::hsa_agent_t>());

  let bytes = size_of::<T>() * count;
  log::trace!("locking {} bytes in RAM", bytes);

  let agents_len = agents.len();
  let agents_ptr = if agents_len != 0 {
    agents.as_ptr() as *mut ffi::hsa_agent_t
  } else {
    ptr::null_mut()
  };

  let mut agent_ptr: *mut T = ptr::null_mut();
  {
    check_err!(ffi::hsa_amd_memory_lock(ptr.as_ptr() as *mut _,
                                        bytes as _, agents_ptr,
                                        agents_len as _,
                                        transmute(&mut agent_ptr)))?;
  }

  Ok(NonNull::new(agent_ptr).expect("got null agent ptr from HSA"))
}
/// Unlock the memory. Does not free/deallocate the memory, just unlocks
/// it, allowing it to be paged out to the page file or deallocated.
///
/// # Safety
///
/// This is unsafe because no checks are done to ensure the pointer isn't
/// still in use by any agents. It it up to you to ensure this.
pub unsafe fn unlock_memory<T>(host_ptr: NonNull<T>, count: usize)
  -> Result<(), Error>
  where T: Sized,
{
  log::trace!("unlocking {} bytes", size_of::<T>() * count);

  let ptr = host_ptr.as_ptr() as *mut _;
  check_err!(ffi::hsa_amd_memory_unlock(ptr))
}
