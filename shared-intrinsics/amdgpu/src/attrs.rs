
use rustc_ast::ast::{MetaItem, };
use rustc_span::symbol::Symbol;
use rustc_middle::ty::{TyCtxt, };

use crate::common::{attrs::*, };

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub enum Condition {
  /// XXX make more specific
  Platform,
}
impl ConditionItem for Condition {
  fn parse_name_value(tcx: TyCtxt, item: &MetaItem) -> Option<Self> {
    if item.check_name(Symbol::intern("platform")) &&
      item.value_str().unwrap().as_str() == "amdgpu" {
      return Some(Condition::Platform);
    }
    let msg = format!("unknown attr key `{}`; (no keys currently)",
                      item.name_or_empty());
    tcx.sess.span_err(item.span, &msg);


    None
  }
}
