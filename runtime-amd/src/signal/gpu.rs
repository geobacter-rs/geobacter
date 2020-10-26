//! Methods that can be run on the GPU.

use std::geobacter::amdgpu::{interrupt::send_interrupt, workitem::ReadFirstLane};
use std::geobacter::platform::platform;
use std::sync::atomic::Ordering;

use hsa_rt::ext::signal::{AsAmdSignal, AmdSignal, AmdSignalKind};

use crate::signal::*;

#[inline(always)]
pub(crate) unsafe fn update_mbox(sig: &AmdSignal) {
  if let Some(mb) = sig.event_mailbox() {
    let id = sig.event_id;
    mb.store(id as _, Ordering::Release);
    send_interrupt(1 | (0 << 4), id.read_first_lane() & 0xff);
  }
}

pub trait AmdHsaSignal: SignalHandle {
  #[inline(always)]
  fn amd_load(&self, order: Ordering) -> Value {
    unsafe {
      self.signal_ref()
        .as_amd_signal()
        .value
        .value
        .load(order)
    }
  }
  #[inline(always)]
  fn gpu_fetch_add(&self, v: Value, order: Ordering) -> Value {
    assert!(platform().is_amdgcn());

    unsafe {
      let s = self.signal_ref().as_amd_signal();
      let v = s.value.value.fetch_add(v, order);
      update_mbox(s);
      v
    }
  }
  #[inline(always)]
  fn gpu_fetch_and(&self, v: Value, order: Ordering) -> Value {
    assert!(platform().is_amdgcn());

    unsafe {
      let s = self.signal_ref().as_amd_signal();
      let v = s.value.value.fetch_and(v, order);
      update_mbox(s);
      v
    }
  }
  #[inline(always)]
  fn gpu_fetch_or(&self, v: Value, order: Ordering) -> Value {
    assert!(platform().is_amdgcn());

    unsafe {
      let s = self.signal_ref().as_amd_signal();
      let v = s.value.value.fetch_or(v, order);
      update_mbox(s);
      v
    }
  }
  #[inline(always)]
  fn gpu_fetch_xor(&self, v: Value, order: Ordering) -> Value {
    assert!(platform().is_amdgcn());

    unsafe {
      let s = self.signal_ref().as_amd_signal();
      let v = s.value.value.fetch_xor(v, order);
      update_mbox(s);
      v
    }
  }
  #[inline(always)]
  fn gpu_fetch_sub(&self, v: Value, order: Ordering) -> Value {
    // XXX AtomicI64::fetch_sub here doesn't deliver interrupts to the host?
    self.gpu_fetch_add(-v, order)
  }
  #[inline(always)]
  fn gpu_swap(&self, v: Value, order: Ordering) -> Value {
    assert!(platform().is_amdgcn());

    unsafe {
      let s = self.signal_ref().as_amd_signal();
      let r = s.value.value.swap(v, order);
      update_mbox(s);
      r
    }
  }
  #[inline(always)]
  fn gpu_compare_and_swap(&self, current: Value, new: Value, order: Ordering) -> Value {
    assert!(platform().is_amdgcn());

    unsafe {
      let s = self.signal_ref().as_amd_signal();
      let r = s.value.value.compare_and_swap(current, new, order);
      if r == current {
        update_mbox(s);
      }
      r
    }
  }
  #[inline(always)]
  fn gpu_compare_exchange(&self, current: Value, new: Value,
                      success: Ordering, failure: Ordering)
                      -> Result<Value, Value>
  {
    assert!(platform().is_amdgcn());

    unsafe {
      let s = self.signal_ref().as_amd_signal();
      let r = s.value.value.compare_exchange(current, new, success, failure)?;
      update_mbox(s);
      Ok(r)
    }
  }
  #[inline(always)]
  fn gpu_compare_exchange_weak(&self, current: Value, new: Value,
                           success: Ordering, failure: Ordering)
                           -> Result<Value, Value>
  {
    assert!(platform().is_amdgcn());

    unsafe {
      let s = self.signal_ref().as_amd_signal();
      let r = s.value.value.compare_exchange_weak(current, new, success, failure)?;
      update_mbox(s);
      Ok(r)
    }
  }
  #[inline(always)]
  fn gpu_store(&self, v: Value, order: Ordering) {
    assert!(platform().is_amdgcn());

    unsafe {
      let s = self.signal_ref().as_amd_signal();
      match s.kind {
        AmdSignalKind::User => {
          s.value.value.store(v, order);
          update_mbox(s);
        }
        // Only queue signals can be anything else
        _ => {
          // ??
          unreachable!()
        }
      }
    }
  }
  #[inline(always)]
  fn gpu_fetch_update<F>(&self, set_order: Ordering,
                     fetch_order: Ordering, mut f: F)
    -> Result<Value, Value>
    where F: FnMut(Value) -> Option<Value>,
  {
    assert!(platform().is_amdgcn());

    unsafe {
      let s = self.signal_ref().as_amd_signal();

      let mut set = false;

      let r = s.value.value.fetch_update(set_order, fetch_order, |v| {
        set = false;
        let v = f(v)?;
        set = true;
        Some(v)
      });

      if set {
        update_mbox(s);
      }
      r
    }
  }
  #[inline(always)]
  fn gpu_fetch_min(&self, v: Value, order: Ordering) -> Value {
    assert!(platform().is_amdgcn());

    unsafe {
      let s = self.signal_ref().as_amd_signal();
      let next = s.value.value.fetch_min(v, order);
      if next > v {
        update_mbox(s);
      }
      next
    }
  }
  #[inline(always)]
  fn gpu_fetch_max(&self, v: Value, order: Ordering) -> Value {
    assert!(platform().is_amdgcn());

    unsafe {
      let s = self.signal_ref().as_amd_signal();
      let next = s.value.value.fetch_max(v, order);
      if next < v {
        update_mbox(s);
      }
      next
    }
  }
}
impl AmdHsaSignal for DeviceSignal { }
impl<'a> AmdHsaSignal for DeviceSignalRef<'a> { }
impl AmdHsaSignal for GlobalSignal { }
impl<'a> AmdHsaSignal for GlobalSignalRef<'a> { }
impl AmdHsaSignal for HostSignal { }
impl<'a> AmdHsaSignal for HostSignalRef<'a> { }

#[cfg(test)]
mod test {
  use super::*;
  use crate::utils::test::*;

  use std::thread::*;
  use hsa_rt::signal::{SignalLoad, SignalStore};

  #[derive(GeobacterDeps)]
  struct Test<'a> {
    #[geobacter_amd(ignore)]
    signal: GlobalSignalRef<'a>,

    wait: GlobalSignalRef<'a>,

    completion: GlobalSignal,
  }
  impl<'a> Kernel for Test<'a> {
    type Grid = Dim1D<Range<u32>>;
    const WORKGROUP: <Self::Grid as GridDims>::Workgroup = Dim1D {
      x: ..16,
    };

    type Queue = DeviceMultiQueue;
    #[inline(always)]
    fn queue(&self) -> &Self::Queue {
      queue()
    }

    fn kernel(&self, vp: KVectorParams<Self>) {
      if vp.gl_id() != 0 { return; }

      self.signal.store(0, Ordering::Release);
    }
  }
  impl<'a> Completion for Test<'a> {
    type CompletionSignal = GlobalSignal;
    #[inline(always)]
    fn completion(&self) -> &GlobalSignal {
      &self.completion
    }
  }

  #[test]
  fn device_side_signals() {
    let dev = device();
    let args_pool = args_pool();

    let signal = GlobalSignal::new(1).unwrap();
    let wait = GlobalSignal::new(1).unwrap();

    let completion = GlobalSignal::new(1).unwrap();

    let m = Test::module(&dev);
    let mut invoc = m.into_invoc(args_pool);
    let _call_wait = unsafe {
      let args = Test {
        signal: signal.as_ref(),
        wait:   wait.as_ref(),
        completion,
      };
      let grid = Dim1D {
        x: 0..16,
      };
      invoc.unchecked_call_async(&grid, args)
        .unwrap()
    };

    sleep(Duration::new(1, 0));
    assert_eq!(signal.load_scacquire(), 1);

    wait.store_screlease(0);

    let r = signal
      .wait_for_zero_timeout(false, Duration::new(1, 0));
    assert!(r.is_ok());
  }

  macro_rules! mk_host_panic_test {
    ($($t:ident, $af:ident,)*) => {$(
      #[test] #[should_panic]
      fn $t() {
        let _dev = device();

        let signal = GlobalSignal::new(0).unwrap();
        signal.$af(1, Ordering::Relaxed);
      }
    )*};
  }
  mk_host_panic_test!(
    fetch_add_hsot_panic, gpu_fetch_add,
    fetch_and_host_panic, gpu_fetch_and,
    fetch_or_host_panic, gpu_fetch_or,
    fetch_xor_host_panic, gpu_fetch_xor,
    fetch_sub_host_panic, gpu_fetch_sub,
    swap_host_panic, gpu_swap,
    store_host_panic, gpu_store,
    fetch_min_host_panic, gpu_fetch_min,
    fetch_max_host_panic, gpu_fetch_max,
  );

  #[test] #[should_panic]
  fn compare_and_swap_host_panic() {
    let _dev = device();

    let signal = GlobalSignal::new(0).unwrap();
    signal.gpu_compare_and_swap(0, 1, Ordering::Relaxed);
  }
  #[test] #[should_panic]
  fn compare_exchange_host_panic() {
    let _dev = device();

    let signal = GlobalSignal::new(0).unwrap();
    let _ = signal.gpu_compare_exchange(0, 1,
                                        Ordering::SeqCst,
                                        Ordering::SeqCst);
  }
  #[test] #[should_panic]
  fn compare_exchange_weak_host_panic() {
    let _dev = device();

    let signal = GlobalSignal::new(0).unwrap();
    let _ = signal.gpu_compare_exchange_weak(0, 1,
                                             Ordering::SeqCst,
                                             Ordering::SeqCst);
  }
  #[test] #[should_panic]
  fn fetch_update_host_panic() {
    let _dev = device();

    let signal = GlobalSignal::new(0).unwrap();
    let _ = signal.gpu_fetch_update(Ordering::SeqCst,
                                    Ordering::SeqCst,
                                    |_| { Some(1) });
  }

  /// Should not panic
  #[test]
  fn host_load() {
    let _dev = device();

    let signal = GlobalSignal::new(0).unwrap();
    assert_eq!(signal.amd_load(Ordering::Relaxed), 0);
  }
}