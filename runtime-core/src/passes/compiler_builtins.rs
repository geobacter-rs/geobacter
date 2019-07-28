
use super::{Pass, PassType};
use compiler_builtins;
use hsa_core::kernel::KernelInstance;

use crate::codegen::PlatformCodegen;

fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
  unsafe { compiler_builtins::mem::memcmp(s1, s2, n) }
}

#[derive(Clone, Debug)]
pub struct CompilerBuiltinsReplacerPass;

impl<P> Pass<P> for CompilerBuiltinsReplacerPass
  where P: PlatformCodegen,
{
  fn pass_type(&self) -> PassType<P> {
    PassType::Replacer(|tcx, dd, def_id| {
      let path = tcx.def_path_str(def_id);
      let info = match &path[..] {
        "core::slice::::memcmp" => {
          KernelInstance::get(&memcmp).kernel_id
        },
        _ => { return None; },
      };

      Some(dd.as_def_id(info).unwrap())
    })
  }
}
