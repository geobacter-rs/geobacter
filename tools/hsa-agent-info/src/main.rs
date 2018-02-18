
extern crate hsa_rt as hsa;

use hsa::agent;
use hsa::mem::region::{QueryRegions};

pub fn main() {
  let ctxt = hsa::ApiContext::new();
  let agents = agent::find_agents(&ctxt)
    .expect("find_agents");

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
          println!("\t\tName: {}", isa.name().unwrap_or_else(|_| "<bad name!>".into()));
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
  }
}
