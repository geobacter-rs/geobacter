
use rustc::ty::item_path::{with_forced_absolute_paths};

use super::{Pass, PassType};
use hsa_core::kernel::kernel_id_for;

use std::any::Any;
use std::fmt;

use std::intrinsics::abort;
use core::panic::{PanicInfo, BoxMeUp, };

fn rust_panic_with_hook(_payload: &mut dyn BoxMeUp,
                        _message: Option<&fmt::Arguments>,
                        _file_line_col: &(&str, u32, u32)) -> ! {
  unsafe { abort() }
}
pub fn rust_begin_panic(_: &PanicInfo) -> ! {
  unsafe { abort() }
}

pub struct PanicPass;

impl Pass for PanicPass {
  fn pass_type(&self) -> PassType {
    PassType::Replacer(|tcx, def_id| {
      // check for "panic_fmt". this is used in an extern fashion: libcore calls an
      // extern "panic_fmt", which is actually defined in libstd. rustc uses
      // the linker to resolve the call. BUT, we don't have the linker, plus we
      // currently require all functions have MIR available, so for the
      // "panic_fmt" case we manually rewrite the def_id to the libstd one.

      /*let attrs = tcx.get_attrs(def_id);
      for attr in attrs.iter() {
        if attr.check_name("lang") {
          match attr.value_str() {
            Some(v) if v == "panic_fmt" => {
              let info = kernel_id_for(&rust_panic_impl);
              return Some(tcx.as_def_id(info).unwrap());
            },
            _ => { },
          }
        }
      }*/

      let path = with_forced_absolute_paths(|| tcx.item_path_str(def_id) );
      let info = match &path[..] {
        "core::panicking::panic_fmt::::panic_impl" => {
          kernel_id_for(&rust_begin_panic)
        },
        "std::panicking::rust_panic_with_hook" => {
          kernel_id_for(&rust_panic_with_hook)
        },
        _ => { return None; },
      };

      Some(tcx.as_def_id(info).unwrap())
    })
  }
}
