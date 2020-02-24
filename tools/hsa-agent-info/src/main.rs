
extern crate hsa_rt as hsa;

use hsa::agent::*;

pub fn main() {
  let ctxt = hsa::ApiContext::try_upref()
    .expect("error creating HSA context");
  let agents = ctxt.agents()
    .expect("find_agents");

  let cpu = agents.iter()
    .find(|agent| Some(DeviceType::Cpu) == agent.device_type().ok() )
    .map(Agent::clone)
    .expect("no CPUs found??");

  for (num, agent) in agents.into_iter().enumerate() {
    println!("Agent #{}:", num);
    println!("\tName = {}", agent.name().expect("failed to get agent name"));
    println!("\tVendor = {}", agent.vendor_name().unwrap());
    println!("\tFeature = {}", if let Ok(f) = agent.feature() {
      format!("{:?}", f)
    } else {
      "N/A".into()
    });
    println!("\tDevice type = {:?}", agent.device_type().unwrap());
    println!("\tQueue size = {:?}", agent.queue_size().unwrap());
    println!("\tQueue type = {:?}", agent.queue_type().unwrap());
    println!("\tVersion = {:?}", agent.version().unwrap());

    // not sure why, but this gives us a bunch of invalid, possibly corrupt, data.
    /*let caches = agent.caches().unwrap();
    for (num, cache) in caches.into_iter().enumerate() {
      println!("\tCache #{}:", num);
      println!("\t\tName: {}", cache.name().unwrap_or_else(|_| "<bad name!>".into() ));
      println!("\t\tLevel: {}", cache.level().unwrap());
      println!("\t\tSize: {}", cache.size().unwrap());
    }*/

    match agent.isas() {
      Ok(isas) => {
        for (num, isa) in isas.into_iter().enumerate() {
          println!("\tISA #{}:", num);
          let info = isa.info().expect("get all ISA info");

          println!("{:#?}", info);
        }
      },
      Err(e) => {
        println!("\tFailed to get available ISAs: `{:?}`", e);
      },
    }

    match agent.all_regions() {
      Ok(regions) => {
        for region in regions.into_iter() {
          println!("\tRegion ID(0x{:x}):", region.id());
          println!("\t\tSegment: {:?}", region.segment().expect("can't get region segment"));
          println!("\t\tGlobal Flags: {:?}",
                   region.global_flags().expect("can't get region global flags"));
          println!("\t\tSize: {}", region.size().expect("can't get region size"));
          println!("\t\tAlloc Max Size: {}", region.alloc_max_size().unwrap());
          println!("\t\tAlloc Max Private Workgroup Size: {}",
                   match region.alloc_max_private_workgroup_size() {
                     Ok(size) => format!("{}", size),
                     Err(_) => "N/A".to_string(),
                   });
          println!("\t\tRuntime Alloc Allowed: {}",
                   region.runtime_alloc_allowed().unwrap());
          println!("\t\tRuntime Alloc Granule: {}",
                   region.runtime_alloc_granule().unwrap());
          println!("\t\tRuntime Alloc Alignment: {}",
                   region.runtime_alloc_alignment().unwrap());
        }
      },
      Err(e) => {
        println!("\tFailed to get available regions: `{:?}`", e);
      },
    }
    if let Ok(pools) = agent.amd_memory_pools() {
      for pool in pools.into_iter() {
        println!("\tPool ID(0x{:x}):", pool.id());
        println!("\t\tSegment: {:?}", pool.segment().expect("can't get region segment"));
        println!("\t\tGlobal Flags: {:?}",
                 pool.global_flags().expect("can't get region global flags"));
        println!("\t\tSize: {}", pool.total_size().expect("can't get region size"));
        println!("\t\tAlloc Allowed: {}", pool.alloc_allowed());
        println!("\t\tAlloc Granule: {}",
                 pool.alloc_granule().unwrap().unwrap_or(1));
        println!("\t\tAlloc Alignment: {}",
                 pool.alloc_alignment().unwrap().unwrap_or(1));
        println!("\t\tCPU Access: {:?}", pool.agent_access(&cpu));
      }
    }
  }
}
