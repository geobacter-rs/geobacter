
use rustc::session::{config, Session};
use rustc::session::CrateDisambiguator;
use rustc_data_structures::stable_hasher::StableHasher;
use rustc_data_structures::fingerprint::Fingerprint;

pub use crate::rustc_interface::util::*;

pub fn compute_crate_disambiguator(session: &Session) -> CrateDisambiguator {
  use std::hash::Hasher;

  // The crate_disambiguator is a 128 bit hash. The disambiguator is fed
  // into various other hashes quite a bit (symbol hashes, incr. comp. hashes,
  // debuginfo type IDs, etc), so we don't want it to be too wide. 128 bits
  // should still be safe enough to avoid collisions in practice.
  let mut hasher = StableHasher::new();

  let mut metadata = session.opts.cg.metadata.clone();
  // We don't want the crate_disambiguator to dependent on the order
  // -C metadata arguments, so sort them:
  metadata.sort();
  // Every distinct -C metadata value is only incorporated once:
  metadata.dedup();

  hasher.write(b"metadata");
  for s in &metadata {
    // Also incorporate the length of a metadata string, so that we generate
    // different values for `-Cmetadata=ab -Cmetadata=c` and
    // `-Cmetadata=a -Cmetadata=bc`
    hasher.write_usize(s.len());
    hasher.write(s.as_bytes());
  }

  // Also incorporate crate type, so that we don't get symbol conflicts when
  // linking against a library of the same name, if this is an executable.
  let is_exe = session
    .crate_types
    .borrow()
    .contains(&config::CrateType::Executable);
  hasher.write(if is_exe { b"exe" } else { b"lib" });

  CrateDisambiguator::from(hasher.finish::<Fingerprint>())
}
