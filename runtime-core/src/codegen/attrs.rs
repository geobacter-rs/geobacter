
use std::fmt::Debug;
use std::str::FromStr;

use num_traits::cast::{cast, NumCast};

use rustc_ast::attr::mk_attr_outer;
use rustc_ast::ast::{self, NestedMetaItem, MetaItem, MetaItemKind};
use rustc_span::{Span, sym, Symbol};
use rustc_middle::ty::TyCtxt;
use rustc_hir::def_id::DefId;

/// Implementers need to implement one of `parse_name_value` or `parse_word`.
pub trait ConditionItem: Debug + PartialEq<Self> + Sized {
  /// If this returns `None`, it is assumed that an error is also emitted.
  fn parse_name_value(tcx: TyCtxt, item: &MetaItem)
    -> Option<Self>
  {
    tcx.sess.span_err(item.span, "expected word literal");

    None
  }
  /// If this returns `None`, it is assumed that an error is also emitted.
  fn parse_word(tcx: TyCtxt, item: &MetaItem)
    -> Option<Self>
  {
    let msg = "expected meta list or name value pair, found word";
    tcx.sess.span_err(item.span, &msg);

    None
  }
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub enum ConditionalExpr<T> {
  Item(T),
  All(Vec<ConditionalExpr<T>>),
  Any(Vec<ConditionalExpr<T>>),
  Not(Box<ConditionalExpr<T>>),
}

impl<T> ConditionalExpr<T>
  where T: ConditionItem,
{
  pub fn all_add(&mut self, v: ConditionalExpr<T>) {
    let mut this: Self = Default::default();
    ::std::mem::swap(&mut this, self);

    match self {
      &mut ConditionalExpr::All(ref mut values) => {
        values.push(v);
        values.push(this);
      },
      _ => unreachable!(),
    }
  }
  pub fn add(&mut self, v: ConditionalExpr<T>) {
    match self {
      &mut ConditionalExpr::Item(_) |
      &mut ConditionalExpr::Not(_) => { self.all_add(v); },
      &mut ConditionalExpr::All(ref mut values) |
      &mut ConditionalExpr::Any(ref mut values) => {
        values.push(v);
      },
    }
  }

  pub fn parse_from_attrs(tcx: TyCtxt, item: &MetaItem) -> Option<Self> {
    match item.kind {
      MetaItemKind::List(ref list) => {
        for mi in list.iter() {
          if !mi.is_meta_item() {
            tcx.sess.span_err(mi.span(), "unsupported literal");
            return None;
          }
        }

        if item.has_name(sym::any) {
          let vals = list.iter()
            .filter_map(|sub_item| {
              Self::parse_from_attrs(tcx, sub_item.meta_item().unwrap())
            })
            .collect();
          return Some(ConditionalExpr::Any(vals));
        } else if item.has_name(sym::all) {
          let vals = list.iter()
            .filter_map(|sub_item| {
              Self::parse_from_attrs(tcx, sub_item.meta_item().unwrap())
            })
            .collect();
          return Some(ConditionalExpr::All(vals));
        } else if item.has_name(sym::not) {
          if list.len() != 1 {
            tcx.sess.span_err(item.span, "expected 1 cfg-pattern");
            return None;
          }

          if let Some(val) = Self::parse_from_attrs(tcx, list[0].meta_item().unwrap()) {
            return Some(ConditionalExpr::Not(Box::new(val)));
          }
        } else {
          tcx.sess.span_err(item.span, &format!("invalid predicate `{}`", item.name_or_empty()));
        }
      },
      MetaItemKind::NameValue(..) => {
        return T::parse_name_value(tcx, item)
          .map(ConditionalExpr::Item);
      },
      MetaItemKind::Word => {
        return T::parse_word(tcx, item)
          .map(ConditionalExpr::Item);
      },
    }

    None
  }
}
impl<T> ConditionalExpr<T>
  where T: PartialEq<T>,
{
  pub fn eval<F>(&self, f: &F) -> bool
    where F: Fn(&T) -> bool,
  {
    match self {
      &ConditionalExpr::Item(ref i) => f(i),
      &ConditionalExpr::Not(ref i) => !i.eval(f),
      &ConditionalExpr::All(ref i) => {
        i.iter()
          .all(|cond| cond.eval(f) )
      },
      &ConditionalExpr::Any(ref i) => {
        i.iter()
          .any(|cond| cond.eval(f) )
      },
    }
  }
}

impl<T> Default for ConditionalExpr<T> {
  fn default() -> Self { ConditionalExpr::All(vec![]) }
}

pub fn from_str_or_unknown_err<'tcx, F, T>(tcx: TyCtxt<'tcx>, span: Span,
                                           value: &str, unknown: F)
  -> Option<T>
  where F: FnOnce(TyCtxt<'tcx>, Span, Option<&str>),
        T: FromStr,
{
  match T::from_str(value) {
    Ok(v) => Some(v),
    Err(_) => {
      unknown(tcx, span, Some(value));
      None
    },
  }
}
pub fn u32_from<T>(tcx: TyCtxt<'_>, span: Span, value: T)
  -> Option<u32>
  where T: NumCast,
{
  let out: Option<u32> = cast(value);

  if out.is_none() {
    let msg = "literal integer either too large for \
                     u32 or negative";
    tcx.sess.span_err(span, &msg);
  }

  out
}

pub fn geobacter_attrs<F>(tcx: TyCtxt<'_>, did: DefId,
                          mut f: F)
  where F: FnMut(&NestedMetaItem),
{
  let attrs = tcx.get_attrs(did);
  for attr in attrs.iter() {
    if !attr.has_name(Symbol::intern("geobacter")) { continue; }

    let list = match attr.meta_item_list() {
      Some(list) => list,
      None => {
        let msg = "#[geobacter] attribute must be of the form \
                   #[geobacter(..)]";
        tcx.sess.span_err(attr.span, &msg);
        continue;
      }
    };

    for item in list.iter() {
      f(item);
    }
  }
}

/// Processes `geobacter_attr` attributes, which are similar to `cfg_attr`,
/// returning the attributes which either aren't `geobacter_attr` or
/// have at least one condition which passes.
/// This function accepts a list of attributes instead of a DefId because
/// the list of attributes will probably need to originate from the unmodified
/// providers.
pub fn geobacter_cfg_attrs<'tcx, T>(tcx: TyCtxt<'tcx>,
                                    previous: &'tcx [ast::Attribute],
                                    root_conditions: &[T])
  -> &'tcx [ast::Attribute]
  where T: ConditionItem,
{
  let gattr = Symbol::intern("geobacter_attr");
  if previous.iter().all(|item| !item.has_name(gattr) ) {
    return previous;
  }

  let mut out = Vec::with_capacity(previous.len());

  for item in previous.iter() {
    if !item.has_name(gattr) {
      out.push(item.clone());
      continue;
    }

    match item.meta_item_list() {
      Some(ref list) if list.len() == 2 => {
        let cond = match list[0].meta_item() {
          Some(v) => v,
          None => {
            tcx.sess.span_err(list[0].span(),
                              "condition must be a meta item");
            continue;
          },
        };

        // TODO eval these conditions in place instead of building this tree.
        let expr = ConditionalExpr::parse_from_attrs(tcx, cond);
        if let Some(expr) = expr {
          if expr.eval(&|cond| root_conditions.iter().any(|root_cond| root_cond == cond )) {
            let sp = list[1].span();

            let attr = match list[1] {
              NestedMetaItem::MetaItem(ref item) => {
                mk_attr_outer(item.clone())
              },
              _ => {
                let msg = "expected a meta item";
                tcx.sess.span_err(sp, &msg);
                continue;
              },
            };

            out.push(attr);
          }
        }
      },
      _ => {
        let msg = "#[geobacter_attr(condition, attribute)] expects two elements: \
                   an `cfg_attr`-esk condition and an attribute";
        tcx.sess.span_err(item.span, &msg);
      },
    }
  }

  tcx.arena.alloc_from_iter(out.into_iter())
}
