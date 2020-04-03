
pub use std::marker::*;
pub use std::ops::{Range, RangeTo, };
pub use std::sync::Arc;

use crate::*;
pub use crate::alloc::*;
pub use crate::module::*;
pub use crate::signal::*;

pub type TestInvoc<A> = Invoc<A, Arc<ArgsPool>, FuncModule<A>>;

lazy_static::lazy_static! {
  static ref DEV: Arc<HsaAmdGpuAccel> = {
    env_logger::init();
    let ctx = grt_core::context::Context::new()
      .expect("create context");
    HsaAmdGpuAccel::first_device(&ctx)
      .unwrap()
  };
}

const ARGS_POOL_SIZE: usize = 16 * 1024 * 1024; // 16Mb.
lazy_static::lazy_static! {
  static ref ARGS_POOL: Arc<ArgsPool> = {
    let p = ArgsPool::new_arena(&DEV, ARGS_POOL_SIZE)
      .unwrap();
    Arc::new(p)
  };
}

pub fn device() -> Arc<HsaAmdGpuAccel> {
  DEV.clone()
}
pub fn args_pool() -> Arc<ArgsPool> {
  // ensure we don't deadlock by explicitly initializing DEV before ARGS_POOL
  &DEV;

  ARGS_POOL.clone()
}
