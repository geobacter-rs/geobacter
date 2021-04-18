use super::*;

use std::sync::atomic::{AtomicU64, Ordering};

use indexvec::Idx;

/// As of time of writ, the largest count of GPUs per server I could find for any price was 32.
/// A more common max was 20 GPUs per server.
pub struct Accessible {
  bits: AtomicU64,
  pub(crate) lock: Mutex<()>,
}

impl Accessible {
  #[inline(always)]
  pub(crate) fn new_local(id: AcceleratorId) -> Self {
    let mut this = Accessible::default();
    this.set_mut(id);
    this
  }
  #[inline(always)]
  pub(crate) fn get(&self, id: AcceleratorId) -> bool {
    let bitvec = self.bits.load(Ordering::Acquire);
    bitvec & (1u64 << id.index()) == 0
  }
  #[inline(always)]
  pub(crate) fn get_mut(&mut self, id: AcceleratorId) -> bool {
    (*self.bits.get_mut()) & (1u64 << id.index()) == 0
  }
  #[inline(always)]
  pub(crate) fn set(&self, order: Ordering, id: AcceleratorId) -> bool {
    let bit = 1u64 << id.index();
    let prev = self.bits.fetch_or(bit, order);
    (prev & bit) == 0
  }
  #[inline(always)]
  pub(crate) fn set_mut(&mut self, id: AcceleratorId) -> bool {
    let bit = 1u64 << id.index();
    let bitvec = self.bits.get_mut();
    let b = (*bitvec & bit) == 0;
    *bitvec |= bit;
    b
  }
  #[inline(always)]
  pub(crate) fn unset(&self, id: AcceleratorId) {
    let bit = 1u64 << id.index();
    self.bits.fetch_and(!bit, Ordering::AcqRel);
  }

  /// You *must* hold the lock! Or otherwise promise that concurrent access isn't possible.
  pub(crate) unsafe fn agents(&self, order: Ordering, out: &mut SmallVec<[Agent; 16]>) {
    out.clear();

    let bitvec = self.bits.load(order);

    if bitvec == 0 { return; }

    let ctx = Context::global().expect("no Geobacter context?");
    for i in 0..64 {
      if bitvec & (1 << i) != 0 {
        let id = AcceleratorId::new(i as _);
        if let Some(dev) = ctx.get_dev_ref::<HsaAmdGpuAccel>(id) {
          out.push(dev.agent().clone());
        }
      }
    }
  }

  #[inline]
  pub fn len(&self) -> usize {
    let bits = self.bits.load(Ordering::Acquire);
    bits.count_ones() as _
  }
}
impl Default for Accessible {
  #[inline(always)]
  fn default() -> Self {
    Accessible {
      bits: AtomicU64::new(0),
      lock: Mutex::new(()),
    }
  }
}
impl Clone for Accessible {
  #[inline(always)]
  fn clone(&self) -> Self {
    Accessible {
      bits: AtomicU64::new(self.bits.load(Ordering::Acquire)),
      lock: Mutex::new(()),
    }
  }
}
impl fmt::Debug for Accessible {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let bits = self.bits.load(Ordering::Acquire);
    write!(f, "Accessible({:#b})", bits)
  }
}
