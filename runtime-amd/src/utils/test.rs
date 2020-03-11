
use crate::*;

lazy_static::lazy_static! {
  static ref DEV: Arc<HsaAmdGpuAccel> = {
    let ctx = grt_core::context::Context::new()
      .expect("create context");
    HsaAmdGpuAccel::first_device(&ctx)
      .unwrap()
  };
}

pub fn device() -> Arc<HsaAmdGpuAccel> {
  DEV.clone()
}
