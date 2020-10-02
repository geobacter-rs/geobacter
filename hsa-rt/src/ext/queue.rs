
use std::sync::atomic::{AtomicU64, AtomicU32};

use crate::ffi;
use queue::{KernelQueue, QueueKind};

#[repr(C, align(64))]
pub struct AmdQueue {
  pub queue_hndl: ffi::hsa_queue_t,
  reserved1: [u32; 4],
  pub write_dispatch_id: AtomicU64,
  pub group_segment_aperture_base_hi: u32,
  pub private_segment_aperture_base_hi: u32,
  pub max_cu_id: u32,
  pub max_wave_id: u32,
  pub max_legacy_doorbell_dispatch_id_plus_1: AtomicU64,
  pub legacy_doorbell_lock: AtomicU32,
  reserved2: [u32; 9],
  pub read_dispatch_id: AtomicU64,
  pub read_dispatch_id_field_base_byte_offset: u32,
  pub compute_tmpring_size: u32,
  pub scratch_resource_descriptor: [u32; 4],
  pub scratch_backing_memory_location: u64,
  pub scratch_backing_memory_byte_size: u64,
  pub scratch_workitem_byte_size: u32,
  pub queue_properties: u32,
  reserved3: [u32; 2],
  pub queue_inactive_signal: ffi::hsa_signal_t,
  reserved4: [u32; 14],
}

pub trait AsAmdQueue {
  unsafe fn as_amd_queue(&self) -> &AmdQueue;
}
impl<T> AsAmdQueue for KernelQueue<T>
  where T: QueueKind,
{
  #[inline(always)]
  unsafe fn as_amd_queue(&self) -> &AmdQueue {
    &*(self.sys.0 as *const _ as *const AmdQueue)
  }
}
