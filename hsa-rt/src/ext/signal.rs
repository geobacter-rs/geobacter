use std::ptr::{read_volatile, write_volatile};
use std::sync::atomic::{AtomicI64, AtomicU64};

use ext::queue::AmdQueue;
use signal::{Signal, SignalRef};

#[repr(u64)]
pub enum AmdSignalKind {
  Invalid = 0,
  User = 1,
  Doorbell = -1i64 as u64,
  LegacyDoorbell = -2i64 as u64,
}

pub union AmdSignalValue {
  pub value: AtomicI64,
  legacy_hardware_doorbell_ptr: *mut u32,
  hardware_doorbell_ptr: *const AtomicU64,
}
impl AmdSignalValue {
  #[inline(always)]
  pub fn legacy_hardware_doorbell_ptr(&self) -> u32 {
    unsafe { read_volatile(self.legacy_hardware_doorbell_ptr as *const u32) }
  }
  #[inline(always)]
  pub fn set_legacy_hardware_doorbell_ptr(&self, v: u32) {
    unsafe { write_volatile(self.legacy_hardware_doorbell_ptr, v) }
  }
  #[inline(always)]
  pub fn hardware_doorbell_ptr(&self) -> *const AtomicU64 {
    unsafe { read_volatile(&self.hardware_doorbell_ptr) }
  }
}

pub union AmdSignalQueuePtr {
  pub queue_ptr: *mut AmdQueue,
  _reserved2: u64,
}

#[repr(C, align(64))]
pub struct AmdSignal {
  pub kind: AmdSignalKind,
  pub value: AmdSignalValue,
  event_mailbox_ptr: u64,
  pub event_id: u32,
  reserved1: u32,
  pub start_ts: u64,
  pub end_ts: u64,
  pub queue_ptr: AmdSignalQueuePtr,
  reserved3: [u32; 2],
}

impl AmdSignal {
  #[inline(always)]
  pub fn queue_ptr(&self) -> *mut AmdQueue {
    unsafe { self.queue_ptr.queue_ptr }
  }
  #[inline(always)]
  pub fn event_mailbox_ptr(&self) -> *const AtomicU64 {
    self.event_mailbox_ptr as usize as *const AtomicU64
  }
  #[inline(always)]
  pub unsafe fn event_mailbox(&self) -> Option<&'static AtomicU64> {
    self.event_mailbox_ptr().as_ref()
  }
}

pub trait AsAmdSignal<'a> {
  unsafe fn as_amd_signal(self) -> &'a AmdSignal;
}
impl<'a> AsAmdSignal<'a> for &'a Signal {
  #[inline(always)]
  unsafe fn as_amd_signal(self) -> &'a AmdSignal {
    &*(self.0.handle as usize as *const AmdSignal)
  }
}
impl<'a> AsAmdSignal<'a> for SignalRef<'a> {
  #[inline(always)]
  unsafe fn as_amd_signal(self) -> &'a AmdSignal {
    &*(self.0.handle as usize as *const AmdSignal)
  }
}
