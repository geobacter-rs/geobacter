
use rustc::lint;
use rustc::session::{config, Session};
use rustc::session::CrateDisambiguator;
use rustc_data_structures::stable_hasher::StableHasher;
use rustc_data_structures::fingerprint::Fingerprint;
use syntax::util::lev_distance::find_best_match_for_name;
use syntax::symbol::{Symbol, sym};
use syntax::{self, ast};

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
const CRATE_TYPES: &[(Symbol, config::CrateType)] = &[
  (sym::rlib, config::CrateType::Rlib),
  (sym::dylib, config::CrateType::Dylib),
  (sym::cdylib, config::CrateType::Cdylib),
  (sym::lib, config::default_lib_output()),
  (sym::staticlib, config::CrateType::Staticlib),
  (sym::proc_dash_macro, config::CrateType::ProcMacro),
  (sym::bin, config::CrateType::Executable),
];
pub fn categorize_crate_type(s: Symbol) -> Option<config::CrateType> {
  Some(CRATE_TYPES.iter().find(|(key, _)| *key == s)?.1)
}
pub fn check_attr_crate_type(attrs: &[ast::Attribute], lint_buffer: &mut lint::LintBuffer) {
  // Unconditionally collect crate types from attributes to make them used
  for a in attrs.iter() {
    if a.check_name(sym::crate_type) {
      if let Some(n) = a.value_str() {
        if let Some(_) = categorize_crate_type(n) {
          return;
        }

        if let ast::MetaItemKind::NameValue(spanned) = a.meta().unwrap().kind {
          let span = spanned.span;
          let lev_candidate = find_best_match_for_name(
            CRATE_TYPES.iter().map(|(k, _)| k),
            &n.as_str(),
            None
          );
          if let Some(candidate) = lev_candidate {
            lint_buffer.buffer_lint_with_diagnostic(
              lint::builtin::UNKNOWN_CRATE_TYPES,
              ast::CRATE_NODE_ID,
              span,
              "invalid `crate_type` value",
              lint::builtin::BuiltinLintDiagnostics::
              UnknownCrateTypes(
                span,
                "did you mean".to_string(),
                format!("\"{}\"", candidate)
              )
            );
          } else {
            lint_buffer.buffer_lint(
              lint::builtin::UNKNOWN_CRATE_TYPES,
              ast::CRATE_NODE_ID,
              span,
              "invalid `crate_type` value"
            );
          }
        }
      }
    }
  }
}
