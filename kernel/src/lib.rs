
//! This crate is shared between the runtime crate and the rustc plugin.
//!
#![feature(rustc_private)]

#[macro_use]
extern crate serde_derive;
extern crate serde;
#[macro_use]
extern crate bitflags;
extern crate rustc;
extern crate ir;
extern crate uuid;

//pub mod global_mem;

/*#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct KernelId {
  pub krate: u64,
  pub f: u64,
}

/// Stores all the info we want to know about a function that
/// will be translated to SPIRV. These are stored as a const inside
/// the function body.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KernelInfo {
  pub id: KernelId,
  pub ir: ir::Module,
}


/// A structure to hold all the info use when we create a concrete SPIRV
/// module out of a kernel.
#[derive(Serialize, Deserialize, Hash, Debug, Clone)]
pub struct MonomorphizationInfo {
  pub ir_hashes: Vec<(KernelId, u64)>,
}

#[derive(Serialize, Deserialize, Hash, Debug, Clone)]
pub struct StaticCrateKernels {
  id: uuid::UuidBytes,
  deps: &'static [&'static StaticCrateKernels],
  kernels: &'static [usize],
}
*/