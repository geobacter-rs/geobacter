
//! XXX: print some sort of message.

use rustc::ty::item_path::{with_forced_absolute_paths};

use super::{Pass, PassType};
use hsa_core::kernel::kernel_id_for;

use std::fmt;




pub struct PanicPass;

impl Pass for PanicPass {
  fn pass_type(&self) -> PassType {
    PassType::Replacer(|tcx, dd, def_id| {
      // check for "panic_fmt". this is used in an extern fashion: libcore calls an
      // extern "panic_fmt", which is actually defined in libstd. rustc uses
      // the linker to resolve the call. BUT, we don't have the linker, plus we
      // currently require all functions have MIR available, so for the
      // "panic_fmt" case we manually rewrite the def_id to the libstd one.

      let path = with_forced_absolute_paths(|| tcx.item_path_str(def_id) );
      let info = match &path[..] {
        "core::panicking::panic_fmt::::panic_impl" => {
          kernel_id_for(&rust_begin_panic)
        },
        "std::panicking::rust_panic_with_hook" => {
          kernel_id_for(&rust_panic_with_hook)
        },
        "core::panicking::panic_bounds_check::ha83b88848a48c215" => {
          kernel_id_for(&rust_panic_bounds_check)
        },
        _ => { return None; },
      };

      Some(dd.as_def_id(info).unwrap())
    })
  }
}
