//! Map
//!

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::str::FromStr;

use crate::num_traits::cast::{cast, NumCast, };

use crate::vko::descriptor::descriptor::{DescriptorBufferDesc,
                                         DescriptorDescTy, };

use crate::syntax::attr::{mk_attr_id, mk_attr_outer, };
use crate::syntax::ast::{self, NestedMetaItem, NestedMetaItemKind,
                         MetaItem, MetaItemKind, LitKind, };
use crate::syntax::source_map::Spanned;
use crate::syntax_pos::{Span, };
use crate::rustc::hir::{def_id::DefId, };
use crate::rustc::mir::{self, Location, };
use crate::rustc::mir::visit::{Visitor, };
use crate::rustc::ty::{TyCtxt, ParamEnv, Instance, Adt, AdtDef, };
use crate::rustc_data_structures::sync::{Lrc, };

use crate::lcore::*;
use crate::lcore::ss::ExeModel;
use crate::rustc_intrinsics::help::*;

use crate::{LegionellaLangItemTypes, };

fn unknown_capability<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>, span: Span,
                                _: Option<&str>)
{
  let msg = "#[legionella(capabilities = \"..\")] expects one of \
                   enum values listed in the SPIR-V spec (too many to \
                   list here)";
  tcx.sess.span_err(span, &msg);
}
fn unknown_exe_model<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                               span: Span,
                               found: Option<&str>)
{
  let found = if let Some(found) = found {
    format!(" `{}`", found)
  } else {
    "".into()
  };
  let msg = format!("unexpected exe_model enum{}; expected one of Host, \
                     Vertex, \
                     Geometry, TessellationControl, TessellationEval, or \
                     Fragment", found);
  tcx.sess.span_err(span, &msg);
}
fn unknown_storage_class<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>, span: Span,
                                   found: Option<&str>)
{
  let found = if let Some(found) = found {
    format!(" `{}`", found)
  } else {
    "".into()
  };
  let msg = format!("unexpected storage_class enum{}; expected one of names \
                     listed in the SPIR-V spec", found);
  tcx.sess.span_err(span, &msg);
}

fn from_str_or_unknown_err<'a, 'tcx, F, T>(tcx: TyCtxt<'a, 'tcx, 'tcx>, span: Span,
                                           value: &str, unknown: F)
  -> Option<T>
  where F: FnOnce(TyCtxt<'a, 'tcx, 'tcx>, Span, Option<&str>),
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
fn exe_model_from_str<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                span: Span,
                                value: &str)
  -> Option<ExeModel>
{
  from_str_or_unknown_err(tcx, span, value, unknown_exe_model)
}
fn storage_class_from_str<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                    span: Span,
                                    value: &str)
  -> Option<StorageClass>
{
  from_str_or_unknown_err(tcx, span, value, unknown_storage_class)
}

fn u32_from<'a, 'tcx, T>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                         span: Span,
                         value: T)
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

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub enum Condition {
  ExeModel(ExeModel),
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub enum ConditionalExpr<T> {
  Item(T),
  All(Vec<ConditionalExpr<T>>),
  Any(Vec<ConditionalExpr<T>>),
  Not(Box<ConditionalExpr<T>>),
}

impl<T> ConditionalExpr<T> {
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

  pub fn parse_from_attrs<'a, 'tcx, F>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       item: &MetaItem,
                                       unknown_item: &F)
    -> Option<Self>
    where F: Fn(TyCtxt<'a, 'tcx, 'tcx>, Span, Option<&str>),
          T: FromStr,
  {
    match item.node {
      MetaItemKind::List(ref list) => {
        for mi in list.iter() {
          if !mi.is_meta_item() {
            tcx.sess.span_err(mi.span, "unsupported literal");
            return None;
          }
        }

        match &*item.name().as_str() {
          "any" => {
            let vals = list.iter()
              .filter_map(|sub_item| {
                Self::parse_from_attrs(tcx, sub_item.meta_item().unwrap(),
                                       unknown_item)
              })
              .collect();
            return Some(ConditionalExpr::Any(vals));
          },
          "all" => {
            let vals = list.iter()
              .filter_map(|sub_item| {
                Self::parse_from_attrs(tcx, sub_item.meta_item().unwrap(),
                                       unknown_item)
              })
              .collect();
            return Some(ConditionalExpr::All(vals));
          },
          "not" => {
            if list.len() != 1 {
              tcx.sess.span_err(item.span, "expected 1 cfg-pattern");
              return None;
            }

            if let Some(val) = Self::parse_from_attrs(tcx, list[0].meta_item().unwrap(),
                                                      unknown_item) {
              return Some(ConditionalExpr::Not(Box::new(val)));
            }
          },
          p => {
            let msg = format!("invalid predicate `{}`", p);
            tcx.sess.span_err(item.span, &msg);
            return None;
          },
        }
      },
      MetaItemKind::NameValue(..) => {
        tcx.sess.span_err(item.span, "expected word literal");
        return None;
      },
      MetaItemKind::Word => {
        return from_str_or_unknown_err(tcx, item.span,
                                       &*item.name().as_str(),
                                       unknown_item)
          .map(ConditionalExpr::Item);
      },
    }
    None
  }
}
impl ConditionalExpr<Condition> {
  /// TODO deduplicate this with `parse_from_attrs` above.
  pub fn parse_cond_from_attrs<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                         item: &MetaItem)
    -> Option<Self>
  {
    match item.node {
      MetaItemKind::List(ref list) => {
        for mi in list.iter() {
          if !mi.is_meta_item() {
            tcx.sess.span_err(mi.span, "unsupported literal");
            return None;
          }
        }

        match &*item.name().as_str() {
          "any" => {
            let vals = list.iter()
              .filter_map(|sub_item| {
                Self::parse_cond_from_attrs(tcx, sub_item.meta_item().unwrap())
              })
              .collect();
            return Some(ConditionalExpr::Any(vals));
          },
          "all" => {
            let vals = list.iter()
              .filter_map(|sub_item| {
                Self::parse_cond_from_attrs(tcx, sub_item.meta_item().unwrap())
              })
              .collect();
            return Some(ConditionalExpr::All(vals));
          },
          "not" => {
            if list.len() != 1 {
              tcx.sess.span_err(item.span, "expected 1 cfg-pattern");
              return None;
            }

            if let Some(val) = Self::parse_cond_from_attrs(tcx, list[0].meta_item().unwrap()) {
              return Some(ConditionalExpr::Not(Box::new(val)));
            }
          },
          p => {
            tcx.sess.span_err(item.span, &format!("invalid predicate `{}`", p));
          }
        }
      },
      MetaItemKind::NameValue(..) => {
        match &*item.name().as_str() {
          "exe_model" => {
            let em = exe_model_from_str(tcx, item.span,
                                        &*item.value_str().unwrap().as_str());
            if let Some(model) = em {
              return Some(ConditionalExpr::Item(Condition::ExeModel(model)));
            }
          },
          key => {
            let msg = format!("unknown attr key `{}`; expected `exe_model`",
                              key);
            tcx.sess.span_err(item.span, &msg);
          },
        }
      },
      MetaItemKind::Word => {
        let msg = "expected meta list or name value pair, found word";
        tcx.sess.span_err(item.span, &msg);
      },
    }
    None
  }
}

impl<T> Default for ConditionalExpr<T> {
  fn default() -> Self { ConditionalExpr::All(vec![]) }
}

#[derive(Clone, Debug)]
pub struct DescriptorSetBinding {
  pub set: u32,
  pub binding: u32,
  pub desc_ty: DescriptorDescTy,
  pub array_count: usize,
  pub read_only: bool,
}
pub fn find_descriptor_set_binding_nums<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                                  item: &NestedMetaItem)
  -> (Option<u32>, Option<u32>)
{
  if item.check_name("set") {
    let value = match item.value_str() {
      Some(value) => value,
      None => {
        let msg = "#[legionella(set)] attribute must be of the form \
                        #[legionella(set = \"<integer>\")]";
        tcx.sess.span_err(item.span, &msg);
        return (None, None);
      },
    };
    let id = match u32::from_str(&*value.as_str()) {
      Ok(b) => b,
      Err(err) => {
        let msg = format!("can't parse u32: {:?}", err);
        tcx.sess.span_err(item.span, &msg);
        return (None, None);
      },
    };

    return (Some(id), None);
  } else if item.check_name("binding") {
    let value = match item.value_str() {
      Some(value) => value,
      None => {
        let msg = "#[legionella(binding)] attribute must be of the form \
                         #[legionella(binding = \"<integer>\")]";
        tcx.sess.span_err(item.span, &msg);
        return (None, None);
      },
    };
    let id = match u32::from_str(&*value.as_str()) {
      Ok(b) => b,
      Err(err) => {
        let msg = format!("can't parse u32: {:?}", err);
        tcx.sess.span_err(item.span, &msg);
        return (None, None);
      },
    };

    return (None, Some(id));
  }

  return (None, None);
}
pub fn optional_descriptor_set_binding_nums<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                                      did: DefId)
  -> (Option<u32>, Option<u32>)
{
  let mut set_num = None;
  let mut binding_num = None;
  legionella_attrs(tcx, did, |item| {
    let (set, binding) = find_descriptor_set_binding_nums(tcx, item);
    if let Some(set) = set {
      set_num = Some(set);
    }
    if let Some(binding) = binding {
      binding_num = Some(binding);
    }
  });

  (set_num, binding_num)
}
pub fn require_descriptor_set_binding_nums<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                                     did: DefId)
  -> (u32, u32)
{
  let (set_num, binding_num) =
    optional_descriptor_set_binding_nums(tcx, did);

  let span = tcx.def_span(did);
  if set_num.is_none() {
    let msg = "missing #[legionella(set = \"<integer>\")]";
    tcx.sess.span_fatal(span, &msg);
  }
  if binding_num.is_none() {
    let msg = "missing #[legionella(binding = \"<integer>\")]";
    tcx.sess.span_fatal(span, &msg);
  }
  let set_num = set_num.unwrap();
  let binding_num = binding_num.unwrap();

  (set_num, binding_num)
}

#[derive(Clone, Debug, Default)]
pub struct GlobalAttrs {
  /// requirement on the use of this global.
  pub capabilities: Option<ConditionalExpr<Capability>>,
  pub exe_model: Option<ConditionalExpr<ExecutionModel>>,
  pub spirv_builtin: Option<Builtin>,
  pub storage_class_if: Vec<(ConditionalExpr<Condition>, StorageClass)>,
  pub storage_class: Option<StorageClass>,
  pub descriptor_set_desc: Option<DescriptorSetBinding>,
}

impl GlobalAttrs {
  pub fn capabilities(&self) -> Cow<ConditionalExpr<Capability>> {
    if let Some(ref caps) = self.capabilities {
      Cow::Borrowed(caps)
    } else {
      Cow::Owned(Default::default())
    }
  }
  pub fn exe_model(&self) -> Cow<ConditionalExpr<ExecutionModel>> {
    if let Some(ref caps) = self.exe_model {
      Cow::Borrowed(caps)
    } else {
      Cow::Owned(Default::default())
    }
  }

  fn check_capabilities<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                  did: DefId)
  {
    if let Some(ref builtin) = self.spirv_builtin {
      let caps = builtin.required_capabilities();
      for cap in caps.iter() {
        if !self.capabilities().eval(&|enabled| enabled == cap ) {
          let msg = format!("capability `{:?}` required by the `{:?}` builtin, \
                             but is not implicitly or explicitly declared",
                            cap, builtin);
          tcx.sess.span_warn(tcx.def_span(did), &msg);
        }
      }
    }

    if let Some(ref class) = self.storage_class {
      let caps = class.required_capabilities();
      for cap in caps.iter() {
        if !self.capabilities().eval(&|enabled| enabled == cap ) {
          let msg = format!("capability `{:?}` required by the `{:?}` storage class, \
                             but is not implicitly or explicitly declared",
                            cap, class);
          tcx.sess.span_warn(tcx.def_span(did), &msg);
        }
      }
    }
  }

  pub fn storage_class(&self, tests: &[Condition]) -> Option<StorageClass> {
    for &(ref condition, class) in self.storage_class_if.iter() {
      if condition.eval(&|lhs| tests.iter().any(|rhs| lhs == rhs ) ) {
        return Some(class);
      }
    }

    self.storage_class
  }
}

#[derive(Debug)]
pub struct Root {
  pub did: DefId,
  /// what capabilities are we allowed to use or need?
  pub capabilities: BTreeSet<Capability>,
  /// what execution model are we using?
  pub execution_model: ExecutionModel,
  pub execution_modes: Vec<ExecutionMode>,
}

impl Root {
  pub fn exe_model(&self) -> ExecutionModel {
    self.execution_model
  }

  fn parse_cap<'a, 'tcx>(&mut self, tcx: TyCtxt<'a, 'tcx, 'tcx>,
                         span: Span, name: &str) {
    match Capability::from_str(name) {
      Ok(cap) => {
        if !self.capabilities.insert(cap) {
          let msg = format!("duplicate capability `{}`", name);
          tcx.sess.span_warn(span, &msg);
        }
      },
      Err(_) => {
        let msg = format!("unknown capability `{}`", name);
        tcx.sess.span_err(span, &msg);
      },
    }
  }
  pub fn parse_caps<'a, 'tcx>(&mut self, tcx: TyCtxt<'a, 'tcx, 'tcx>,
                              item: &MetaItem)
  {
    let mut span = item.span;
    'error: loop {
      match item.node {
        MetaItemKind::List(ref list) => {
          for mi in list.iter() {
            if !mi.is_word() {
              span = mi.span;
              break 'error;
            }

            self.parse_cap(tcx, mi.span, &*mi.name().unwrap().as_str());
          }
        },
        MetaItemKind::Word => {
          break 'error;
        },
        MetaItemKind::NameValue(..) => {
          self.parse_cap(tcx, item.span,
                         &*item.value_str().unwrap().as_str());
        },
      }

      return;
    }

    let msg = format!("#[legionella(capabilities(..))] expects a list of words, \
                      found {:?}", item);
    tcx.sess.span_err(span, &msg);
  }

  pub fn check_capabilities<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>) {
    let caps = self.execution_model.required_capabilities();
    for cap in caps.iter() {
      if !self.capabilities.contains(cap) {
        let msg = format!("capability `{:?}` required by the `{:?}` exe model, \
                             but is not implicitly or explicitly declared",
                          cap, self.exe_model());
        tcx.sess.span_warn(tcx.def_span(self.did), &msg);
      }
    }

    for mode in self.execution_modes.iter() {
      let caps = mode.required_capabilities();
      for cap in caps.iter() {
        if !self.capabilities.contains(cap) {
          let msg = format!("capability `{:?}` required by the `{:?}` exe mode, \
                             but is not implicitly or explicitly declared",
                            cap, mode);
          tcx.sess.span_warn(tcx.def_span(self.did), &msg);
        }
      }
    }
  }

  pub fn required_extensions(&self) -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();

    let caps = self.capabilities
      .iter()
      .flat_map(|cap| {
        cap.required_extensions()
          .iter()
          .map(|&ext| ext )
      });
    out.extend(caps);

    let exe_model = self.execution_model
      .required_extensions()
      .iter()
      .map(|&ext| ext );
    out.extend(exe_model);

    let modes = self.execution_modes
      .iter()
      .flat_map(|mode| {
        mode.required_extensions()
          .iter()
          .map(|&ext| ext )
      });
    out.extend(modes);

    out
  }
}

pub fn legionella_attrs<'a, 'tcx, F>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                     did: DefId,
                                     mut f: F)
  where F: FnMut(&NestedMetaItem),
{
  let attrs = tcx.get_attrs(did);
  for attr in attrs.iter() {
    if !attr.check_name("legionella") { continue; }

    let list = match attr.meta_item_list() {
      Some(list) => list,
      None => {
        let msg = "#[legionella] attribute must be of the form \
                         #[legionella(..)]";
        tcx.sess.span_err(attr.span, &msg);
        continue;
      }
    };

    for item in list.iter() {
      f(item);
    }
  }
}

/// extracts the marker function from a lang item's constructor function.
/// The marker function will be the last parameter.
/// TODO currently this will always use the last static it visits.
/// Instead, it should create a sub visitor starting at the respective
/// data arg when it finds the marker function.
pub struct LangItemTypeCtorVisitor<'a, 'tcx>
  where 'tcx: 'a,
{
  tcx: TyCtxt<'a, 'tcx, 'tcx>,
  mir: &'a mir::Mir<'tcx>,
  global: Instance<'tcx>,

  data_instance: Option<mir::Static<'tcx>>,
  marker_instance: Option<Instance<'tcx>>,
}

impl<'a, 'tcx> mir::visit::Visitor<'tcx> for LangItemTypeCtorVisitor<'a, 'tcx>
  where 'tcx: 'a,
{
  fn visit_static(&mut self,
                  static_: &mir::Static<'tcx>,
                  context: mir::visit::PlaceContext<'tcx>,
                  location: Location)
  {
    if let Some(ref prev) = self.data_instance {
      let tcx = self.tcx;
      let msg = "found duplicate static; this is possibly a bug in the compiler";
      tcx.sess.span_warn(tcx.def_span(static_.def_id),
                         &msg);

      let msg = "previous static found here";
      tcx.sess.span_note_without_error(tcx.def_span(prev.def_id),
                                       &msg);

      // Don't return. This lets us print more errors regarding duplicate
      // markers if we for some reason find more.
    }
    info!("found static for lang type: {:?}", static_.def_id);
    self.data_instance = Some(static_.clone());
    self.super_static(static_, context, location);
  }
  fn visit_terminator(&mut self,
                      block: mir::BasicBlock,
                      term: &mir::Terminator<'tcx>,
                      location: Location)
  {
    use crate::rustc::mir::*;

    let tcx = self.tcx;
    match term.kind {
      TerminatorKind::Call {
        ref func, ..
      } => {
        let reveal_all = ParamEnv::reveal_all();

        let callee_ty = func.ty(self.mir, tcx);
        let callee_ty = tcx
          .subst_and_normalize_erasing_regions(self.global.substs,
                                               reveal_all,
                                               &callee_ty);
        let sig = callee_ty.fn_sig(tcx);
        let sig = tcx.normalize_erasing_late_bound_regions(reveal_all, &sig);
        if let Some(marker_ty) = sig.inputs().last() {
          info!("found marker fn: {:#?}", marker_ty);
          let marker = extract_fn_instance(tcx, self.global,
                                           marker_ty);
          if let Some(prev) = self.marker_instance {
            let msg = "found duplicate marker; this is possibly a bug \
                      in the compiler";
            tcx.sess.span_warn(tcx.def_span(marker.def_id()),
                               &msg);

            let msg = "previous marker found here";
            tcx.sess.span_note_without_error(tcx.def_span(prev.def_id()),
                                             &msg);

            // Don't return. This lets us print more warning regarding duplicate
            // markers if we for some reason find more.
          }
          self.marker_instance = Some(marker);
        }
      },
      _ => { },
    }
    self.super_terminator(block, term, location);
  }
}

#[derive(Clone, Eq, PartialEq)]
pub struct RustVkLangDesc<'tcx> {
  lang: LegionellaLangItemTypes,
  global: mir::Static<'tcx>,
  marker: Instance<'tcx>
}

impl<'tcx> RustVkLangDesc<'tcx> {
}

pub fn extract_rust_vk_lang_desc<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                           instance: Instance<'tcx>,
                                           adt_def: &'tcx AdtDef)
  -> Option<RustVkLangDesc<'tcx>>
{
  let adt_did = adt_def.did;
  let mut lang_item = None;
  legionella_attrs(tcx, adt_did, |item| {
    if item.check_name("lang_item") {
      let s = match item.value_str() {
        Some(str) => str,
        None => {
          let msg = "#[legionella(lang_item)] attribute must be of the form \
                             #[legionella(lang_item = \"..\")]";
          tcx.sess.span_err(item.span, &msg);
          return;
        },
      };

      match LegionellaLangItemTypes::from_str(&*s.as_str()) {
        Ok(li) => {
          lang_item = Some(li);
        },
        Err(_) => {
          // user code should not be using this, so no need to
          // be descriptive.
          let msg = "unknown lang item type";
          tcx.sess.span_err(item.span, &msg);
          return;
        },
      }
    }
  });

  if let Some(lang_item) = lang_item {
    // this will be a static variable used in a shader/kernel.
    // we need to inspect the MIR used to initialize the static's
    // memory contents, and extract the marker function.
    // then we inspect the attributes on the marker function to
    // discover the descriptor set and bindings indices.

    let mir = tcx.instance_mir(instance);
    let mut search = LangItemTypeCtorVisitor {
      tcx,
      mir: &mir,
      global: instance,

      data_instance: None,
      marker_instance: None,
    };

    search.visit_mir(&mir);

    search.data_instance.clone()
      .and_then(|global| {
        search.marker_instance.clone()
          .map(move |marker| {
            (global, marker)
          })
      })
      .map(|(global, marker)| {
        RustVkLangDesc {
          lang: lang_item,
          global,
          marker,
        }
      })
  } else {
    None
  }
}

pub fn legionella_global_attrs<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                         _root_model: ExecutionModel,
                                         instance: Instance<'tcx>,
                                         perform_checking: bool)
  -> GlobalAttrs
{
  let id = instance.def_id();

  let mut out = GlobalAttrs::default();

  let mut required_storage_class = None;

  match tcx.type_of(id).sty {
    Adt(adt_def, _substs) => {
      let rust_vk_lang =
        extract_rust_vk_lang_desc(tcx, instance,
                                  adt_def);
      if let Some(&RustVkLangDesc { lang, marker, .. }) = rust_vk_lang.as_ref() {
        let (set_num, binding_num) =
          require_descriptor_set_binding_nums(tcx, marker.def_id());

        let desc = match lang {
          LegionellaLangItemTypes::Buffer => {
            let buf_desc = DescriptorBufferDesc {
              dynamic: Some(false),
              storage: true,
            };

            required_storage_class = Some(StorageClass::StorageBuffer);

            DescriptorDescTy::Buffer(buf_desc)
          },
          _ => {
            unimplemented!("Legionella lang item type: {:?}", lang);
          },
        };

        out.descriptor_set_desc = Some(DescriptorSetBinding {
          set: set_num,
          binding: binding_num,
          desc_ty: desc,
          array_count: 1,
          // TODO inspect the types and possibly the kernel MIR
          // to see if a descriptor is actually modified.
          read_only: false,
        });
      }
    },
    _ => { },
  }

  // TODO check that storage_class isn't used on a function.

  legionella_attrs(tcx, id, |item| {
    if item.check_name("capabilities") {
      if out.capabilities.is_some() {
        let msg = "duplicate #[legionella(capabilities(..))]";
        tcx.sess.span_err(item.span, &msg);
        return;
      }
      let caps = ConditionalExpr::parse_from_attrs(tcx, item.meta_item().unwrap(),
                                                   &unknown_capability);
      out.capabilities = caps;
    } else if item.check_name("spirv_builtin") {
      let value = match item.value_str() {
        Some(value) => value,
        None => {
          let msg = "#[legionella(spirv_builtin)] attribute must be of the form \
                             #[legionella(spirv_builtin = \"..\")]";
          tcx.sess.span_err(item.span, &msg);
          return;
        }
      };
      let builtin = match Builtin::from_str(&*value.as_str()) {
        Ok(b) => b,
        Err(_) => {
          let msg = "unknown SPIRV builtin";
          tcx.sess.span_err(item.span, &msg);
          return;
        },
      };

      if out.spirv_builtin.is_some() {
        let msg = "#[legionella(spirv_builtin = \"..\")] specified more \
                           than once";
        tcx.sess.span_err(item.span, &msg);
        return;
      }

      out.spirv_builtin = Some(builtin);

      // TODO check builtin type
    } else if item.check_name("storage_class_if") {
      // TODO check condition against require_storage_class
      match item.meta_item_list() {
        Some(list) if list.len() == 2 => {
          let cond = match list[0].meta_item() {
            Some(v) => v,
            None => {
              tcx.sess.span_err(list[0].span, "unsupported literal");
              return;
            },
          };
          let class = &*(match list[1].name() {
            Some(name) => name,
            None => {
              let msg = "expected a word";
              tcx.sess.span_err(list[1].span, &msg);
              return;
            }
          }).as_str();

          let expr = ConditionalExpr::parse_cond_from_attrs(tcx, cond)
            .and_then(|cond| {
              storage_class_from_str(tcx, list[1].span,
                                     class)
                .map(move |class| (cond, class) )
            });
          if let Some(expr) = expr {
            out.storage_class_if.push(expr);
          }
        },
        _ => {
          let msg = "#[legionella(storage_class_if(..))] expects two elements \
                            a condition and a storage class";
          tcx.sess.span_err(item.span, &msg);
        }
      }
    } else if item.check_name("storage_class") {
      let value = match item.value_str() {
        Some(value) => value,
        None => {
          let msg = "#[legionella(storage_class)] attribute must be of the form \
                            #[legionella(storage_class = \"..\")]";
          tcx.sess.span_err(item.span, &msg);
          return;
        }
      };
      let class = match StorageClass::from_str(&*value.as_str()) {
        Ok(b) => b,
        Err(_) => {
          let msg = "unknown SPIRV storage class";
          tcx.sess.span_err(item.span, &msg);
          return;
        },
      };

      if out.storage_class.is_some() {
        let msg = "#[legionella(storage_class = \"..\")] specified more \
                           than once";
        tcx.sess.span_err(item.span, &msg);
        return;
      }

      if perform_checking {
        if let Some(required) = required_storage_class {
          if required != class {
            let msg = format!("#[legionella(storage_class = \"..\")] required to be {:?}",
                              required);
            tcx.sess.span_err(item.span, &msg);
            return;
          }
        }
      }

      out.storage_class = Some(class);

      if out.storage_class == Some(StorageClass::StorageBuffer) {
        let (set_num, binding_num) =
          require_descriptor_set_binding_nums(tcx, id);
        out.descriptor_set_desc = Some(DescriptorSetBinding {
          set: set_num,
          binding: binding_num,
          desc_ty: DescriptorDescTy::Buffer(DescriptorBufferDesc {
            dynamic: Some(false),
            storage: true,
          }),
          array_count: 1,
          // TODO inspect the types and possibly the kernel MIR
          // to see if a descriptor is actually modified.
          read_only: false,
        });
      }
    } else if item.check_name("exe_model") {
      match item.meta_item_list() {
        Some(list) if list.len() == 1 => {
          let mi = &list[0];
          if !mi.is_meta_item() {
            tcx.sess.span_err(mi.span, "unsupported literal");
            return;
          }

          let model = ConditionalExpr::parse_from_attrs(tcx, mi.meta_item().unwrap(),
                                                        &unknown_exe_model);
          if let Some(_) = model {
            if out.exe_model.is_some() {
              let msg = "duplicate #[legionella(exe_model(..))]";
              tcx.sess.span_err(item.span, &msg);
              return;
            }
          }

          out.exe_model = model;
        },
        Some(_) => {
          let msg = "#[legionella(exe_model(..))] expects a single condition";
          tcx.sess.span_err(item.span, &msg);
          return;
        },
        None => {
          let msg = "#[legionella(exe_model(..))] expects a list with one condition";
          tcx.sess.span_err(item.span, &msg);
          return;
        },
      }
    } else if item.check_name("set") || item.check_name("binding") {
      // ignore these two, they are parsed contextually elsewhere.
    } else {
      let msg = "unknown Legionella attribute";
      tcx.sess.span_err(item.span, &msg);
      return;
    }
  });

  if perform_checking {
    out.check_capabilities(tcx, id);
  }

  if let Some(class) = required_storage_class {
    // set in case not specified.
    out.storage_class = Some(class);
  }

  out
}

pub fn legionella_root_attrs<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       id: DefId,
                                       model: ExecutionModel,
                                       perform_checking: bool)
  -> Root
{
  let mut out = Root {
    did: id,
    capabilities: BTreeSet::new(),
    execution_model: model,
    execution_modes: Vec::new(),
  };

  legionella_attrs(tcx, id, |item| {
    if item.check_name("capabilities") {
      out.parse_caps(tcx, item.meta_item().unwrap());
    } else if item.check_name("local_size") {
      if !item.is_meta_item_list() {
        let msg = "#[legionella(local_size(..))] expects a list \
                          of name/value pairs: `x`, `y`, and `z`";
        tcx.sess.span_err(item.span, &msg);
      }

      fn check_missing<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                 span: Span,
                                 value: Option<u32>,
                                 name: &str)
      {
        if value.is_some() { return; }

        let msg = format!("#[legionella(local_size(..))] missing dim `{}`",
                          name);
        tcx.sess.span_err(span, &msg);
      }

      let mut x = None;
      let mut y = None;
      let mut z = None;
      for dim in item.meta_item_list().unwrap().iter() {
        if !dim.is_meta_item() {
          let msg = "unsupported literal";
          tcx.sess.span_err(dim.span, &msg);
          return;
        }
        let dim = dim.meta_item().unwrap();
        let dim_name = &*dim.name().as_str();
        let dim_opt = match dim_name {
          "x" => &mut x,
          "y" => &mut y,
          "z" => &mut z,
          _ => {
            let msg = format!("unknown local_size dim `{}`; expected \
                                 one of `x`, `y`, or `z`", dim_name);
            tcx.sess.span_err(dim.span, &msg);
            return;
          },
        };

        let (size, span) = match dim.name_value_literal() {
          Some(&Spanned {
            node: LitKind::Int(v, ..),
            span,
          }) => (v, span),
          _ => {
            let msg = "expected integer literal";
            tcx.sess.span_err(dim.span, &msg);
            return;
          },
        };

        if let Some(size) = u32_from(tcx, span, size) {
          *dim_opt = Some(size);
        }
      }

      check_missing(tcx, item.span, x, "x");
      check_missing(tcx, item.span, y, "y");
      check_missing(tcx, item.span, z, "z");

      if x.is_none() || y.is_none() || z.is_none() { return; }

      for mode in out.execution_modes.iter() {
        match mode {
          &ExecutionMode::LocalSize { .. } => {
            tcx.sess.span_err(item.span, "#[legionella(local_size(..))] given twice")
          },
          _ => {},
        }
      }

      let mode = ExecutionMode::LocalSize {
        x: x.unwrap(),
        y: y.unwrap(),
        z: z.unwrap(),
      };
      out.execution_modes.push(mode);
    } else {
      let msg = "unknown Legionella attribute";
      tcx.sess.span_err(item.span, &msg);
    }
  });

  /// Take our starting capabilities and transitively add all dep
  /// capabilities

  fn recurse(into: &mut BTreeSet<Capability>, cap: Capability) {
    for &dep in cap.implicitly_declares().iter() {
      if into.insert(dep) {
        recurse(into, dep);
      }
    }
  }
  for cap in out.capabilities.clone().into_iter() {
    recurse(&mut out.capabilities, cap);
  }

  if perform_checking {
    trace!("attrs for {:?}: {:#?}", id, out);
    out.check_capabilities(tcx);
  }

  out
}

/// Processes `legionella_attr` attributes, which are similar to `cfg_attr`,
/// returning the attributes which either aren't `legionella_attr` or
/// have at least one condition which passes.
/// This function accepts a list of attributes instead of a DefId because
/// the list of attributes will probably need to originate from the unmodified
/// providers.
pub fn legionella_cfg_attrs<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                      previous: &[ast::Attribute],
                                      root_conditions: &[Condition])
  -> Lrc<[ast::Attribute]>
{
  let mut out = Vec::with_capacity(previous.len());

  for item in previous.iter() {
    if !item.check_name("legionella_attr") {
      out.push(item.clone());
      continue;
    }

    match item.meta_item_list() {
      Some(ref list) if list.len() == 2 => {
        let cond = match list[0].meta_item() {
          Some(v) => v,
          None => {
            tcx.sess.span_err(list[0].span, "condition must be a meta item");
            continue;
          },
        };

        // TODO eval these conditions in place instead of building this tree.
        let expr = ConditionalExpr::parse_cond_from_attrs(tcx, cond);
        if let Some(expr) = expr {
          if expr.eval(&|cond| root_conditions.iter().any(|root_cond| root_cond == cond )) {
            let id = mk_attr_id();
            let sp = list[1].span;

            let attr = match list[1].node {
              NestedMetaItemKind::MetaItem(ref item) => {
                mk_attr_outer(sp, id, item.clone())
              },
              _ => {
                let msg = "expected a meta item";
                tcx.sess.span_err(list[1].span, &msg);
                continue;
              },
            };

            out.push(attr);
          }
        }
      },
      _ => {
        let msg = "#[legionella_attr(condition, attribute)] expects two elements: \
                         an `cfg_attr`-esk condition and an attribute";
        tcx.sess.span_err(item.span, &msg);
      },
    }
  }

  Lrc::from(out)
}
