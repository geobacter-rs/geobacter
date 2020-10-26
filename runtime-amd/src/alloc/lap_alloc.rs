
use super::*;

use std::sync::atomic::Ordering;

#[derive(Clone, Debug)]
pub struct LapAlloc {
  pub(crate) pool: MemoryPool,
  pub(crate) accessible: Accessible,
}
impl LapAlloc {
  pub fn pool_allocator(&self) -> MemoryPoolAlloc {
    unsafe { self.pool.allocator().unwrap() }
  }
  pub fn pool(&self) -> &MemoryPool {
    &self.pool
  }

  fn try_alloc_pool(&self) -> Result<MemoryPoolAlloc, AllocError> {
    unsafe {
      self.pool.allocator().ok().ok_or(AllocError)
    }
  }

  #[inline(always)]
  pub fn accessible(&self) -> &Accessible { &self.accessible }

  #[inline]
  pub fn is_accessible_to(&self, dev: &HsaAmdGpuAccel) -> bool {
    !self.accessible.get(dev.id())
  }

  /// Update access grants to a new allocation
  unsafe fn update_access_grants(&self, ptr: NonNull<u8>,
                                 _layout: Layout) -> Result<(), HsaError> {
    let mut agents = SmallVec::new();
    self.accessible.agents(Ordering::Acquire, &mut agents);
    if agents.len() == 0 { return Ok(()); }

    // Handle cases where `LapAllocAccess::add_access` is called before allocation.
    // We don't need to check accessibility: it was already checked in
    // LapAllocAccess::add_access. Note: this can *only* happen from the context of
    // a single thread: Arcs can't be shared if the arc itself hasn't finished allocating.
    // Arcs also can't be grown or shrunk. And the other box types require mut for that, so.

    let pool_ptr = MemoryPoolPtr::from_ptr(self.pool.clone(), ptr);
    pool_ptr.grant_agents_access(&agents[..])?;

    Ok(())
  }
}
impl<'a> From<&'a Arc<HsaAmdGpuAccel>> for LapAlloc {
  fn from(accel: &'a Arc<HsaAmdGpuAccel>) -> Self {
    accel.fine_lap_node_alloc(0)
  }
}

unsafe impl AllocRef for LapAlloc {
  fn alloc(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
    let alloc = self.try_alloc_pool()?;
    let out = alloc.alloc(layout)?;
    let bytes = layout.size();
    if bytes == 0 {
      return Ok(out);
    }

    unsafe {
      match self.update_access_grants(out.as_non_null_ptr(), layout) {
        Ok(()) => { },
        Err(_) => {
          alloc.dealloc(out.as_non_null_ptr(), layout);
          return Err(AllocError);
        }
      }
    }

    Ok(out)
  }

  unsafe fn dealloc(&self, ptr: NonNull<u8>, layout: Layout) {
    if layout.size() != 0 {
      self.pool.dealloc_from_pool(ptr)
        // Should errors be ignored instead of panicking?
        .expect("deallocation failure");
    }
  }

  unsafe fn grow(&self, ptr: NonNull<u8>,
                 old_layout: Layout, new_layout: Layout)
                 -> Result<NonNull<[u8]>, AllocError>
  {
    let alloc = self.try_alloc_pool()?;
    let new = alloc.try_grow(ptr, old_layout, new_layout, |new| {
      self.update_access_grants(new.as_non_null_ptr(), new_layout)
        .ok().ok_or(AllocError)
    })?;

    Ok(new)
  }

  unsafe fn grow_zeroed(&self,
                        ptr: NonNull<u8>,
                        old_layout: Layout,
                        new_layout: Layout)
                        -> Result<NonNull<[u8]>, AllocError>
  {
    let alloc = self.try_alloc_pool()?;
    let new = alloc.try_grow_zeroed(ptr, old_layout, new_layout, |new| {
      self.update_access_grants(new.as_non_null_ptr(), new_layout)
        .ok().ok_or(AllocError)
    })?;

    Ok(new)
  }

  unsafe fn shrink(&self,
                   ptr: NonNull<u8>,
                   old_layout: Layout,
                   new_layout: Layout)
                   -> Result<NonNull<[u8]>, AllocError>
  {
    let alloc = self.try_alloc_pool()?;
    let new = alloc.try_shrink(ptr, old_layout, new_layout, |new| {
      self.update_access_grants(new.as_non_null_ptr(), new_layout)
        .ok().ok_or(AllocError)
    })?;

    Ok(new)
  }
}

impl<'a> AllocMaybeSyncAccessGrant for &'a mut LapAlloc {
  fn grant_access(self, dev: &HsaAmdGpuAccel,
                  pool_ptr: Option<MemoryPoolPtr<[u8]>>) -> Result<(), HsaError>
  {
    if !self.accessible.get_mut(dev.id()) {
      // nothing to do.
      return Ok(());
    }

    let aa = match self.pool.agent_access(dev.agent()) {
      Ok(v) => v,
      Err(err) => {
        warn!("failed to get agent access of pool({:?}): {:?}", self.pool, err);
        return Err(err);
      },
    };

    if aa.never_allowed() {
      error!("pool {:?} not sharable with this device ({:?})",
             self.pool, dev.id());
      return Err(HsaError::General);
    } else if aa.default_disallowed() {
      if pool_ptr.is_none() { return Ok(()); }
      let pool_ptr = pool_ptr.unwrap();
      // no need to worry about races here; we have a mut reference.
      let mut agents: SmallVec<[Agent; 16]> = SmallVec::default();
      unsafe {
        self.accessible.agents(Ordering::Relaxed, &mut agents);
      }
      agents.push(dev.agent().clone());
      match pool_ptr.grant_agents_access(&agents) {
        Ok(()) => { },
        Err(err) => {
          warn!("failed to grant access to allocation in pool({:?}): {:?}", self.pool, err);
          return Err(err);
        },
      }
      self.accessible.set_mut(dev.id());
    } else if aa.default_allowed() {
      // nothing to do
    } else {
      unreachable!("{:?}", aa);
    }

    Ok(())
  }
}
impl<'a> AllocMaybeSyncAccessGrant for &'a LapAlloc {
  fn grant_access(self, dev: &HsaAmdGpuAccel,
                  pool_ptr: Option<MemoryPoolPtr<[u8]>>) -> Result<(), HsaError>
  {
    if !self.accessible.get(dev.id()) { return Ok(()); }

    let aa = match self.pool.agent_access(dev.agent()) {
      Ok(v) => v,
      Err(err) => {
        warn!("failed to get agent access of pool({:?}): {:?}", self.pool, err);
        return Err(err);
      },
    };

    if aa.never_allowed() {
      error!("pool {:?} not sharable with this device ({:?})",
             self.pool, dev.id());
      return Err(HsaError::General);
    } else if aa.default_disallowed() {
      if pool_ptr.is_none() { return Ok(()); }
      let pool_ptr = pool_ptr.unwrap();
      let mut agents: SmallVec<[Agent; 16]> = SmallVec::default();
      agents.reserve(self.accessible.len());

      let _lock = self.accessible.lock.lock(); // Ensure we don't trample other threads.
      if !self.accessible.set(Ordering::AcqRel, dev.id()) {
        // another thread beat us
        return Ok(());
      }
      unsafe {
        self.accessible.agents(Ordering::Acquire, &mut agents);
      }
      match pool_ptr.grant_agents_access(&agents[..]) {
        Ok(()) => {},
        Err(err) => {
          // ensure other threads don't think we've successfully granted access
          self.accessible.unset(dev.id());
          drop(_lock);

          warn!("failed to grant access to allocation in pool({:?}): {:?}", self.pool, err);
          return Err(err);
        },
      };

    } else if aa.default_allowed() {
      // nothing to do
      self.accessible.set(Ordering::AcqRel, dev.id());
    } else {
      unreachable!("{:?}", aa);
    }

    Ok(())
  }
}