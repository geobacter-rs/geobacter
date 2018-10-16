
use std::error::Error;
use std::ffi::c_void;
use std::mem::transmute;

use ffi;
use {ApiContext, agent::Agent, };

pub struct MemoryPool(ffi::hsa_amd_memory_pool_t, ApiContext);
impl Agent {
  pub fn amd_memory_pools(&self) -> Result<Vec<MemoryPool>, Box<Error>> {
    extern "C" fn get_pool(pool: ffi::hsa_amd_memory_pool_t,
                            pools: *mut c_void) -> ffi::hsa_status_t {
      let pools: &mut Vec<MemoryPool> = unsafe {
        transmute(pools)
      };
      pools.push(MemoryPool(pool, ApiContext::upref()));
      ffi::hsa_status_t_HSA_STATUS_SUCCESS
    }

    let mut out: Vec<MemoryPool> = vec![];
    Ok(check_err!(ffi::hsa_amd_agent_iterate_memory_pools(self.0, Some(get_pool), transmute(&mut out)) => out)?)
  }
}
