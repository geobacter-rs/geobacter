
use rustc::hir::def_id::{DefId};
use trans::worker::TranslatorCtxtRef;

pub mod lang_item;
pub mod alloc;
pub mod panic;
pub mod compiler_builtins;

pub enum PassType {
  Replacer(for<'a, 'b, 'tcx> fn(TranslatorCtxtRef<'a, 'b, 'a, 'tcx>, DefId) -> Option<DefId>),
}

pub trait Pass: Send + Sync {
  fn pass_type(&self) -> PassType;
}
