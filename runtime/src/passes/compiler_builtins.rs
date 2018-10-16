
use rustc::ty::item_path::{with_forced_absolute_paths};

use super::{Pass, PassType};
use hsa_core::kernel::kernel_id_for;
use compiler_builtins;

fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
  unsafe { compiler_builtins::mem::memcmp(s1, s2, n) }
}

#[derive(Clone, Debug)]
pub struct CompilerBuiltinsReplacerPass;

impl Pass for CompilerBuiltinsReplacerPass {
  fn pass_type(&self) -> PassType {
    PassType::Replacer(|tcx, def_id| {
      let path = with_forced_absolute_paths(|| tcx.item_path_str(def_id) );
      let info = match &path[..] {
        "core::slice::::memcmp" => {
          kernel_id_for(&memcmp)
        },
        _ => { return None; },
      };

      Some(tcx.as_def_id(info).unwrap())
    })
  }
}
