
use rustc_session::{config, CrateDisambiguator, Session, };
use rustc_middle::ty;
use rustc_data_structures::fingerprint::Fingerprint;
use rustc_data_structures::stable_hasher::StableHasher;
use rustc_data_structures::sync::{Lock, };
use rustc_passes;
use rustc_driver::plugin;
use rustc_privacy;
use rustc_traits;
use rustc_typeck as typeck;

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

pub fn default_provide(providers: &mut ty::query::Providers<'_>) {
  plugin::build::provide(providers);
  rustc_middle::hir::provide(providers);
  rustc_mir::provide(providers);
  rustc_mir_build::provide(providers);
  rustc_privacy::provide(providers);
  typeck::provide(providers);
  ty::provide(providers);
  rustc_trait_selection::traits::provide(providers);
  rustc_passes::provide(providers);
  rustc_resolve::provide(providers);
  rustc_traits::provide(providers);
  rustc_ty::provide(providers);
  rustc_metadata::provide(providers);
  rustc_lint::provide(providers);
  rustc_codegen_ssa::provide(providers);
  rustc_symbol_mangling::provide(providers);
}

pub fn default_provide_extern(providers: &mut ty::query::Providers<'_>) {
  rustc_metadata::provide_extern(providers);
  rustc_codegen_ssa::provide_extern(providers);
}

/// Note: we could be called from arbitrary threads, but rustc generally requires a
/// larger-than-default stack, so we *must* spawn into the global thread pool the context
/// installed during its initialization.
pub fn on_global_thread_pool<F, R>(f: F) -> R
  where F: FnOnce() -> R + Send,
        R: Send,
{
  use crate::rustc_data_structures::rayon::{scope, current_thread_index, };

  let gcx_ptr = &Lock::new(0);

  if current_thread_index().is_some() {
    // don't spawn; we're already on a thread with a large stack
    ty::tls::GCX_PTR.set(gcx_ptr, f)
  } else {
    let mut r = None;

    scope(|scope| {
      scope.spawn(|_| {
        r = Some(ty::tls::GCX_PTR.set(gcx_ptr, f))
      })
    });

    r.unwrap()
  }
}
