
use rustc::ty::item_path::{with_forced_absolute_paths};

use super::{Pass, PassType};
use hsa_core::kernel_info::kernel_info_for;

use std::any::Any;
use std::fmt;

use std::intrinsics::abort;

#[inline(never)]
fn rust_panic_with_hook(payload: Box<Any + Send>,
                        message: Option<&fmt::Arguments>,
                        file_line_col: &(&'static str, u32, u32)) -> ! {
  unsafe { abort() }
}
/*extern "C" fn rust_eh_personality(version: i32,
                                   actions: i32,
                                   exception_class: i32,
                                   exception_object: *mut (),
                                   context: *mut ())
  -> i32
{
  unsafe { abort() }
}*/

pub struct PanicPass;

impl Pass for PanicPass {
  fn pass_type(&self) -> PassType {
    PassType::Replacer(|tcx, def_id| {
      use syntax::attr::*;
      use rustc::middle::lang_items::*;

      // check for "panic_fmt". this is used in an extern fashion: libcore calls an
      // extern "panic_fmt", which is actually defined in libstd. rustc uses
      // the linker to resolve the call. BUT, we don't have the linker, plus we
      // currently require all functions have MIR available, so for the
      // "panic_fmt" case we manually rewrite the def_id to the libstd one.

      let attrs = tcx.get_attrs(def_id);
      for attr in attrs.iter() {
        if attr.check_name("lang") {
          match attr.value_str() {
            Some(v) if v == "panic_fmt" => {
              let info = kernel_info_for(&rust_panic_with_hook);
              return Some(tcx.as_def_id(info.id).unwrap());
            },
            _ => { },
          }
        }
      }

      let path = with_forced_absolute_paths(|| tcx.item_path_str(def_id) );
      let info = match &path[..] {
        "std::panicking::rust_panic_with_hook" => {
          kernel_info_for(&rust_panic_with_hook)
        },
        _ => { return None; },
      };

      Some(tcx.as_def_id(info.id).unwrap())
    })
  }
}
