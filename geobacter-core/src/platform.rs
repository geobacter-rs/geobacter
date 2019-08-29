
//! Types and functions to determine what platform code is actually running on.
//! Since Geobacter doesn't operate at the syntax level, attributes like `#[cfg()]`
//! don't work. So instead, we have a common enum definition here which includes
//! all supported accelerator devices. You can then query the platform at runtime
//! with the constant function provided. LLVM *should* then use the constant-ness
//! for const propagation and remove branches for other devices.

use std::mem::{size_of, transmute, };

pub use shared_defs::platform::*;

#[inline(always)]
pub const fn current_platform() -> Platform {
  extern "rust-intrinsic" {
    fn __geobacter_current_platform() -> &'static [u8; size_of::<Platform>()];
  }

  let p: &'static Platform = unsafe {
    let r = __geobacter_current_platform();
    transmute(r)
  };
  *p
}
#[inline(always)]
pub fn is_host() -> bool {
  match current_platform() {
    Platform::Unix |
    Platform::Windows => true,

    _ => false,
  }
}
#[inline(always)]
pub fn is_spirv() -> bool {
  match current_platform() {
    Platform::Vulkan(_) => true,
    _ => false,
  }
}
#[inline(always)]
pub fn is_amdgcn() -> bool {
  match current_platform() {
    Platform::Hsa(self::hsa::Device::AmdGcn(_)) => true,
    _ => false,
  }
}
#[inline(always)]
pub fn is_cuda() -> bool {
  match current_platform() {
    Platform::Cuda => true,
    _ => false,
  }
}
