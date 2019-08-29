
//! Debugging environmental variables

use std::env::var;
use std::sync::atomic::*;

static USE_LLC: AtomicBool = AtomicBool::new(false);
static OPT_REMARKS: AtomicBool = AtomicBool::new(false);

fn key(key: &str) -> String {
  format!("GEOBACTER_{}", key)
}

fn b(k: &str) -> bool {
  if let Ok(v) = var(key(k)) {
    v != "0"
  } else {
    false
  }
}

/// Called when a new context is created.
pub(crate) fn initialize() {
  USE_LLC.store(b("USE_LLC"), Ordering::Relaxed);
  OPT_REMARKS.store(b("OPT_REMARKS"), Ordering::Relaxed);

  fence(Ordering::Release);
}

pub fn use_llc() -> bool {
  USE_LLC.load(Ordering::Acquire)
}
pub fn print_opt_remarks() -> bool {
  OPT_REMARKS.load(Ordering::Acquire)
}
