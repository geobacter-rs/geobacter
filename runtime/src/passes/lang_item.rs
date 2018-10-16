
use super::{Pass, PassType};

pub struct LangItemPass;

impl Pass for LangItemPass {
  fn pass_type(&self) -> PassType {
    PassType::Replacer(|tcx, def_id| {
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
              let new_def_id = tcx.lang_items()
                .require(LangItem::PanicFnLangItem)
                .unwrap();
              return Some(new_def_id);
            },
            _ => { },
          }
        }
      }

      None
    })
  }
}
