
use std::alloc::*;

use parking_lot::Mutex;

use super::*;

use self::accessible::Accessible;

pub mod accessible;
pub mod lap_alloc;

pub use self::lap_alloc::*;

pub trait LapAccessible {
  fn is_accessible_to(&self, dev: &HsaAmdGpuAccel) -> bool;
}

/// No device removal.
pub trait LapGrantAccess {
  /// TODO: it would be nice to allow granting new devices access (which is expensive)
  /// concurrently. This is safe because only removal is concurrent-unsafe.
  fn add_access(self, device: &HsaAmdGpuAccel) -> Result<(), HsaError>;
}

trait AllocMaybeSyncAccessGrant {
  fn grant_access(self, dev: &HsaAmdGpuAccel, pool_ptr: Option<MemoryPoolPtr<[u8]>>) -> Result<(), HsaError>;
}

/// A Vec-esk type allocated from a CPU visible memory pool.
/// Locality here means host memory.
///
/// This type is safe to dereference.
///
/// Lap: LocallyAccessiblePool
pub type LapVec<T> = alloc_wg::vec::Vec<T, LapAlloc>;
/// A Box-esk type allocated from a CPU visible memory pool.
/// Locality here means host memory.
///
/// This type is safe to dereference.
///
/// Lap: LocallyAccessiblePool
pub type LapBox<T> = alloc_wg::boxed::Box<T, LapAlloc>;

pub type LapArc<T> = alloc_wg::sync::Arc<T, LapAlloc>;
pub type LapWeak<T> = alloc_wg::sync::Weak<T, LapAlloc>;

impl<T> LapAccessible for LapVec<T> {
  fn is_accessible_to(&self, dev: &HsaAmdGpuAccel) -> bool {
    if std::mem::size_of::<T>() == 0 {
      // These are never actually allocated, so we can just say they're
      // always accessible.
      return true;
    }

    self.alloc_ref().is_accessible_to(dev)
  }
}
impl<'a, T> LapGrantAccess for &'a mut LapVec<T> {
  fn add_access(self, device: &HsaAmdGpuAccel) -> Result<(), HsaError> {
    if std::mem::size_of::<T>() == 0 {
      // These are never actually allocated, so we can just say they're
      // always accessible.
      return Ok(());
    }

    let ptr = unsafe { self.pool_ptr() };
    self.alloc_ref_mut().grant_access(device, ptr)
  }
}

impl<T> LapAccessible for LapBox<T>
  where T: ?Sized,
{
  fn is_accessible_to(&self, dev: &HsaAmdGpuAccel) -> bool {
    if std::mem::size_of_val(&**self) == 0 {
      // These are never actually allocated, so we can just say they're
      // always accessible: no delayed allocations for boxes.
      return true;
    }

    self.alloc_ref().is_accessible_to(dev)
  }
}
impl<'a, T> LapGrantAccess for &'a mut LapBox<T>
  where T: ?Sized,
{
  fn add_access(self, device: &HsaAmdGpuAccel) -> Result<(), HsaError> {
    if std::mem::size_of_val(&**self) == 0 {
      // These are never actually allocated, so we can just say they're
      // always accessible: no delayed allocations for boxes.
      return Ok(());
    }

    let ptr = unsafe { self.pool_ptr() };
    self.alloc_ref_mut().grant_access(device, ptr)
  }
}
impl<T> LapAccessible for LapArc<T>
  where T: ?Sized,
{
  fn is_accessible_to(&self, dev: &HsaAmdGpuAccel) -> bool {
    // Note: even zero sized T will have an allocation.
    LapArc::alloc_ref(self).is_accessible_to(dev)
  }
}
impl<'a, T> LapGrantAccess for &'a LapArc<T>
  where T: ?Sized,
{
  fn add_access(self, device: &HsaAmdGpuAccel) -> Result<(), HsaError> {
    // Note: even zero sized T will have an allocation.
    let ptr = unsafe { self.pool_ptr() };
    LapArc::alloc_ref(self).grant_access(device, ptr)
  }
}
impl<T> LapAccessible for LapWeak<T>
  where T: ?Sized,
{
  fn is_accessible_to(&self, dev: &HsaAmdGpuAccel) -> bool {
    self.alloc_ref()
      .map(|alloc| alloc.is_accessible_to(dev) )
      .unwrap_or(true)
  }
}
impl<'a, T> LapGrantAccess for &'a LapWeak<T>
  where T: ?Sized,
{
  fn add_access(self, device: &HsaAmdGpuAccel) -> Result<(), HsaError> {
    // Note: even zero sized T will have an allocation.
    let ptr = match unsafe { self.pool_ptr() } {
      Some(ptr) => ptr,
      None => {
        return Ok(());
      }
    };
    match self.alloc_ref() {
      Some(alloc) => alloc.grant_access(device, Some(ptr)),
      None => { Ok(()) }
    }
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::utils::test::*;

  #[test]
  fn ensure_send_sync() {
    fn check<T: Send + Sync>() { }
    check::<LapAlloc>();
  }

  #[test]
  fn lap_vec_late_alloc() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    // now alloc:
    m.resize(1, 0u32);

    let accessible = m.alloc_ref()
      .accessible()
      .len();
    assert_eq!(accessible, 1);
  }

  #[test]
  fn lap_box_zero_sized_alloc() {
    let dev = device();

    let mut m = LapBox::new_in((), dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    assert_eq!(m.alloc_ref().accessible().len(), 0);
  }
  #[test]
  fn lap_vec_clone_access() {
    let dev = device();

    let mut m = LapVec::new_in(dev.fine_lap_node_alloc(0));
    m.add_access(&dev).unwrap();

    // now alloc:
    m.resize(1, 0u32);

    let m2 = m.clone();
    assert!(m2.is_accessible_to(&dev));
  }
}
