
use rustc::session::{config, Session};
use rustc::session::CrateDisambiguator;
use rustc::traits;
use rustc::ty;
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
  rustc::hir::provide(providers);
  rustc_mir::provide(providers);
  rustc_mir_build::provide(providers);
  rustc_privacy::provide(providers);
  typeck::provide(providers);
  ty::provide(providers);
  traits::provide(providers);
  rustc_passes::provide(providers);
  rustc_resolve::provide(providers);
  rustc_traits::provide(providers);
  rustc_ty::provide(providers);
  rustc_metadata::provide(providers);
  rustc_lint::provide(providers);
  rustc_codegen_utils::provide(providers);
  rustc_codegen_ssa::provide(providers);
}

pub fn default_provide_extern(providers: &mut ty::query::Providers<'_>) {
  rustc_metadata::provide_extern(providers);
  rustc_codegen_ssa::provide_extern(providers);
}

pub fn spawn_thread_pool<F, R>(f: F) -> R
  where F: FnOnce() -> R + Send,
        R: Send,
{
  use crate::rustc_data_structures::rayon::{ThreadBuilder,
                                            ThreadPool,
                                            ThreadPoolBuilder, };

  let gcx_ptr = &Lock::new(0);

  let config = ThreadPoolBuilder::new()
    // Our modules are usually pretty small; no need to go wild here.
    .num_threads(2)
    // give us a huge stack:
    .stack_size(32 * 1024 * 1024)
    .deadlock_handler(|| unsafe { ty::query::handle_deadlock() });

  let with_pool = move |pool: &ThreadPool| {
    pool.install(move || f())
  };

  syntax::GLOBALS.with(|syntax_globals| {
    rustc_span::GLOBALS.with(|rustc_span_globals| {
      // The main handler runs for each Rayon worker thread and sets up
      // the thread local rustc uses. syntax_globals and syntax_pos_globals are
      // captured and set on the new threads. ty::tls::with_thread_locals sets up
      // thread local callbacks from libsyntax
      let main_handler = move |worker: ThreadBuilder| {
        syntax::GLOBALS.set(syntax_globals, || {
          rustc_span::GLOBALS.set(rustc_span_globals, || {
            ty::tls::GCX_PTR.set(gcx_ptr, || worker.run() )
          })
        })
      };

      config.build_scoped(main_handler, with_pool).unwrap()
    })
  })
}
