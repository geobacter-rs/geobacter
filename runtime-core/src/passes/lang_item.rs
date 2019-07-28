
use super::{Pass, PassType};
use crate::codegen::PlatformCodegen;
use crate::syntax_pos::symbol::sym;

pub struct LangItemPass;

impl<P> Pass<P> for LangItemPass
  where P: PlatformCodegen,
{
  fn pass_type(&self) -> PassType<P> {
    PassType::Replacer(|tcx, dd, def_id| {
      use rustc::middle::lang_items::*;

      // check for "panic_fmt". this is used in an extern fashion: libcore calls an
      // extern "panic_fmt", which is actually defined in libstd. rustc uses
      // the linker to resolve the call. BUT, we don't have the linker, plus we
      // currently require all functions have MIR available, so for the
      // "panic_fmt" case we manually rewrite the def_id to the libstd one.

      let eh_personality_did = tcx.lang_items()
        .require(LangItem::EhPersonalityLangItem)
        .ok();
      if Some(def_id) == eh_personality_did {
        return Some(dd.instance_of(tcx, &rust_eh_personality).def_id());
      }

      let attrs = tcx.get_attrs(def_id);
      for attr in attrs.iter() {
        if attr.check_name(sym::lang) {
          match attr.value_str() {
            Some(v) if v == sym::panic_impl => {
              let new_def_id = tcx.lang_items()
                .require(LangItem::PanicImplLangItem)
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

// Never called.
pub fn rust_eh_personality() {}
