
//! Note none of this work on hosts: any access to memory allocated
//! from these pools will always segfault.
//! You AFAICT *must* use memory locking for transferring in and out.

use std::error::Error;
use std::ffi::c_void;
use std::fmt;
use std::mem::{transmute, size_of, };
use std::ptr::NonNull;
use std::slice::{from_raw_parts, from_raw_parts_mut, };

use ffi;
use {ApiContext, agent::Agent, agent::DeviceType,
     signal::Signal, };

pub use mem::region::Segment;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct MemoryPool(ffi::hsa_amd_memory_pool_t);
impl Agent {
  pub fn amd_memory_pools(&self) -> Result<Vec<MemoryPool>, Box<Error>> {
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


#[derive(Clone, Copy, Eq, PartialEq, Debug, Hash, Serialize, Deserialize)]
pub enum AgentAccess {
  Never,
  DefaultDisallowed,
  DefaultAllowed,
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct GlobalFlags(pub u32);
impl GlobalFlags {
  pub fn kernel_arg(&self) -> bool {
    (self.0 & ffi::hsa_amd_memory_pool_global_flag_s_HSA_AMD_MEMORY_POOL_GLOBAL_FLAG_KERNARG_INIT) != 0
  }
  pub fn fine_grained(&self) -> bool {
    (self.0 & ffi::hsa_amd_memory_pool_global_flag_s_HSA_AMD_MEMORY_POOL_GLOBAL_FLAG_FINE_GRAINED) != 0
  }
  pub fn coarse_grained(&self) -> bool {
    (self.0 & ffi::hsa_amd_memory_pool_global_flag_s_HSA_AMD_MEMORY_POOL_GLOBAL_FLAG_COARSE_GRAINED) != 0
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


impl MemoryPool {
  pub fn agent_access(&self, agent: &Agent)
    -> Result<AgentAccess, Box<Error>>
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
        return Err(format!("unknown pool access type {}", out).into());
      },
    };

    Ok(out)
  }
  pub fn global_flags(&self) -> Result<Option<GlobalFlags>, Box<Error>> {
    let attr = ffi::hsa_amd_memory_pool_info_t_HSA_AMD_MEMORY_POOL_INFO_GLOBAL_FLAGS;
    match self.segment()? {
      Segment::Global => Ok(Some(GlobalFlags(pool_info!(self, attr, u32)?))),
      _ => Ok(None),
    }
  }
  pub fn alloc_alignment(&self) -> Result<Option<usize>, Box<Error>> {
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
  pub fn alloc_granule(&self) -> Result<Option<usize>, Box<Error>> {
    let attr = ffi::hsa_amd_memory_pool_info_t_HSA_AMD_MEMORY_POOL_INFO_RUNTIME_ALLOC_GRANULE;
    if self.alloc_allowed() {
      Ok(Some(pool_info!(self, attr, usize)?))
    } else {
      Ok(None)
    }
  }
  pub fn segment(&self) -> Result<Segment, Box<Error>> {
    let attr = ffi::hsa_amd_memory_pool_info_t_HSA_AMD_MEMORY_POOL_INFO_SEGMENT;
    let out = pool_info!(self, attr, ffi::hsa_amd_segment_t)?;
    let out = match out {
      ffi::hsa_amd_segment_t_HSA_AMD_SEGMENT_GLOBAL => Segment::Global,
      ffi::hsa_amd_segment_t_HSA_AMD_SEGMENT_GROUP => Segment::Group,
      ffi::hsa_amd_segment_t_HSA_AMD_SEGMENT_PRIVATE => Segment::Private,
      ffi::hsa_amd_segment_t_HSA_AMD_SEGMENT_READONLY => Segment::ReadOnly,
      _ => {
        return Err(format!("unknown pool segment type {}", out).into());
      },
    };

    Ok(out)
  }
  pub fn total_size(&self) -> Result<usize, Box<Error>> {
    let attr = ffi::hsa_amd_memory_pool_info_t_HSA_AMD_MEMORY_POOL_INFO_SIZE;
    Ok(pool_info!(self, attr, usize)?)
  }

  pub fn is_migratable_to(&self, to: &MemoryPool) -> Result<bool, Box<Error>> {
    let mut out = [false; 1];
    Ok(check_err!(ffi::hsa_amd_memory_pool_can_migrate(self.0, to.0,
                  out.as_mut_ptr() as *mut _) => out[0])?)
  }

  pub fn alloc_in_pool<T>(&self, count: usize) -> Result<MemoryPoolPtr<T>, Box<Error>>
    where T: Sized,
  {
    ApiContext::check_live()?;
    if !self.alloc_allowed() {
      return Err("allocation not allowed in this pool".into());
    }

    let len = size_of::<T>() * count;
    let mut dest = 0 as *mut c_void;
    let dest_ptr = &mut dest as *mut *mut c_void;
    check_err!(ffi::hsa_amd_memory_pool_allocate(self.0, len,
                                                 0, dest_ptr))?;

    Ok(MemoryPoolPtr(unsafe {
      NonNull::new_unchecked(dest_ptr as *mut T)
    }, *self))
  }
  /// Does *not* run `T`'s dtor.
  pub unsafe fn dealloc_in_pool<T>(&self, ptr: MemoryPoolPtr<T>)
    -> Result<(), Box<Error>>
  {
    Ok(check_err!(ffi::hsa_amd_memory_pool_free(ptr.as_ptr() as *mut c_void) => ())?)
  }
}
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct MemoryPoolPtr<T>(NonNull<T>, MemoryPool)
  where T: ?Sized;

impl<T> MemoryPoolPtr<T>
  where T: ?Sized,
{
  pub fn as_ptr(&self) -> *mut T { self.0.as_ptr() }
  pub unsafe fn as_slice(&self, len: usize) -> &[T]
    where T: Sized,
  {
    from_raw_parts(self.as_ptr() as *const _,
                   len)
  }
  pub unsafe fn as_mut_slice(&self, len: usize) -> &[T]
    where T: Sized,
  {
    from_raw_parts_mut(self.as_ptr(), len)
  }

  /// doesn't check that the ptr is actually from this pool
  pub unsafe fn from_ptr(pool: MemoryPool,
                                ptr: NonNull<T>)
    -> Self
  {
    MemoryPoolPtr(ptr, pool)
  }

  pub fn pool(&self) -> &MemoryPool { &self.1 }


  /// Modifies the physical address of the pointer. The virtual address is
  /// left alone.
  pub fn move_to_pool(&mut self, to: &MemoryPool) -> Result<(), Box<Error>> {
    check_err!(ffi::hsa_amd_memory_migrate(self.as_ptr() as *const _,
                                           to.0, 0) => ())?;

    self.1 = *to;
    Ok(())
  }

  /*fn check_agent_access(&self, agent: &Agent) -> Result<bool, Box<Error>> {
    match self.1.agent_access(agent)? {
      AgentAccess::Disallowed => {
        Err(format!("agent {} can't be provided access to this memory pool {:?} ptr",
                    agent.name()?, self.1).into())
      },

    }
  }*/

  /// Grants access to this ptr to a single agent.
  pub fn grant_agent_access(&self, agent: &Agent) -> Result<(), Box<Error>> {
    let agents = [agent.0; 1];

    let agents_len = agents.len();
    let agents_ptr = agents.as_ptr();

    check_err!(ffi::hsa_amd_agents_allow_access(agents_len as _,
                                                agents_ptr,
                                                0 as *const _,
                                                self.0.as_ptr() as *const _) => ())?;
    Ok(())
  }
  pub fn grant_agents_access(&self, agents: &[Agent]) -> Result<(), Box<Error>> {
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
  fn query_info(&self, ret_accessible: bool) -> Result<PtrInfo<T>, Box<Error>> {
    unsafe extern "C" fn alloc_accessible(count: usize) -> *mut c_void {
      use std::mem::{size_of, forget, };

      assert_eq!(size_of::<Agent>(), size_of::<ffi::hsa_agent_t>(),
                 "Agent wrapper type has extra padding");

      let mut alloc: Vec<Agent> = Vec::with_capacity(count);

      let alloc_ptr = alloc.as_mut_ptr() as *mut ffi::hsa_agent_t;
      forget(alloc);
      alloc_ptr as *mut _
    }

    let mut info: [ffi::hsa_amd_pointer_info_t; 1] = [unsafe {
      ::std::mem::uninitialized()
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
      size: info.sizeInBytes,
      owner: Agent(info.agentOwner, ApiContext::upref()),
      accessible_by,
    };

    Ok(info)
  }
  fn accessible_info(&self) -> Result<PtrInfo<T>, Box<Error>> {
    self.query_info(true)
  }
  fn info(&self) -> Result<PtrInfo<T>, Box<Error>> {
    self.query_info(false)
  }
}

impl<T> QueryPtrInfo<T> for MemoryPoolPtr<T> {
  fn as_alloc_ptr(&self) -> *const c_void { self.0.as_ptr() as *const _ }
}

/// The header files don't specify if `src` and `dst` must be pool
/// members; I've taken the conservative option here.
#[allow(unused_unsafe)]
pub unsafe fn async_copy<T>(dst: MemoryPoolPtr<T>,
                            src: MemoryPoolPtr<T>,
                            count: usize,

                            dst_agent: &Agent,
                            src_agent: &Agent,

                            deps: &[&Signal],
                            completion: &Signal)
  -> Result<(), Box<Error>>
  where T: ?Sized,
{
  use std::mem::size_of_val;

  let dst_ptr = dst.as_ptr() as *mut c_void;
  let src_ptr = src.as_ptr() as *const c_void;

  let size = size_of_val(src.0.as_ref()) * count;

  info!("enqueuing async dma copy ({} bytes)", size);

  let deps: Vec<_> = deps
    .iter()
    .map(|dep| dep.0 )
    .collect();

  let deps_len = deps.len() as _;
  let deps_ptr = deps.as_ptr();

  check_err!(ffi::hsa_amd_memory_async_copy(dst_ptr, dst_agent.0,
                                            src_ptr, src_agent.0,
                                            size,

                                            deps_len, deps_ptr,
                                            completion.0) => ())?;
  Ok(())
}
