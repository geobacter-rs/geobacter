
use crate::syntax::ast::{MetaItem, };
use crate::rustc::ty::{TyCtxt, };

use crate::common::{attrs::*, };

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub enum Condition {
  // TODO
}
impl ConditionItem for Condition {
  fn parse_name_value(tcx: TyCtxt, item: &MetaItem) -> Option<Self> {
    let msg = format!("unknown attr key `{}`; (no keys currently)",
                      item.name_or_empty());
    tcx.sess.span_err(item.span, &msg);


    None
  }
  fn parse_word(tcx: TyCtxt, item: &MetaItem) -> Option<Self> {
    let msg = format!("unknown attr name `{}`; (no keys currently)",
                      item.name_or_empty());
    tcx.sess.span_err(item.span, &msg);


    None
  }
}
