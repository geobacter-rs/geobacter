
//! WIP, mostly shit crate.
//!
//! What we do here is take the MIR from functions marked with
//! #[kernel] and translate them into a platform agnostic IR.
//! Then at runtime we translate the agnostic IR back into MIR
//! and instruct `rustc` to translate it into accelerator
//! specific code.

#![crate_name = "hsa_rustc_plugin"]
#![crate_type = "dylib"]
#![feature(plugin_registrar)]
#![feature(rustc_private)]
#![feature(box_patterns)]

#[macro_use]
extern crate rustc;
extern crate rustc_mir;
extern crate rustc_plugin;
extern crate rustc_data_structures;
extern crate rustc_trans;
extern crate syntax;
extern crate syntax_pos;
extern crate ir;
extern crate indexvec;
extern crate serde;
extern crate serde_json;
//extern crate kernel;

use std::cell::RefCell;
use std::rc::Rc;
use std::ops::Deref;

use rustc::hir::def_id::{DefId, CrateNum};
use rustc::mir::{self, Mir, TerminatorKind, SourceInfo};
use rustc::mir::transform::{MirPass, MirSource};
use rustc::session::Session;
use rustc::ty::{self, TyCtxt, subst, TyFnDef, TyRef, TypeAndMut,
                TyParam, FnSig};
use rustc::traits::MirPluginIntrinsicTrans;
use rustc_plugin::Registry;
use syntax::feature_gate::AttributeType;
use syntax_pos::Span;
//use syntax::ast::NodeId;

//use kernel_info::KernelInfo;

use ir::{MAIN_FUNCTION};

use indexvec::Idx;

use context::GlobalCtx;
use context::init::ContextLoader;

pub mod context;

pub mod debug;
//pub mod kernel_info;

#[plugin_registrar]
pub fn plugin_registrar(reg: &mut Registry) {
  reg.register_attribute("hsa_lang_item".into(),
                         AttributeType::Normal);

  //let p = Rc::new(debug::Debug::new());
  //reg.register_post_optimization_mir_pass(p as Rc<MirPass>);

  let ctxt = GlobalCtx::new();
  let init = Rc::new(ContextLoader::new(&ctxt));
  reg.register_post_optimization_mir_pass(init as Rc<MirPass>);

  let compiletime = Box::new(MirToHsaIrPass {
    ctx: ctxt.clone(),
  });
  reg.register_intrinsic("json_kernel_info_for".to_string(),
                         compiletime);
}

pub enum LangItem {
  Real,
  UInt,
  Int,
  FuncMir,
  BinOpFunction,
}

struct MirToHsaIrPass {
  ctx: GlobalCtx,
}
impl MirToHsaIrPass {

}
impl MirPluginIntrinsicTrans for MirToHsaIrPass {
  fn trans_simple_intrinsic<'a, 'tcx>(&self,
                                      tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                      name: &str,
                                      source_info: SourceInfo,
                                      sig: &FnSig<'tcx>,
                                      parent_mir: &mir::Mir<'tcx>,
                                      parent_param_substs: &'tcx subst::Substs<'tcx>,
                                      args: &Vec<mir::Operand<'tcx>>,
                                      dest: mir::Lvalue<'tcx>,
                                      extra_stmts: &mut Vec<mir::StatementKind<'tcx>>)
    where 'tcx: 'a,
  {
    use serde_json::{to_string_pretty};

    use syntax::symbol::{Symbol};
    use rustc::middle::const_val::{ConstVal};
    use rustc::mir::{Literal, Constant, Operand, Rvalue,
                     StatementKind, Statement, Terminator};
    use rustc_data_structures::indexed_vec::Idx;

    tcx.sess.span_note_without_error(source_info.span,
                                     "passing over this");

    let fmt = match name {
      "json_kernel_info_for" => KernelInfoKind::Json,
      _ => unreachable!(),
    };

    if args.len() != 1 {
      tcx.sess
        .span_fatal(source_info.span,
                    "incorrect kernel info intrinsic call");
      return;
    }

    let local = match &args[0] {
      &mir::Operand::Consume(mir::Lvalue::Local(ref l)) => {
        &parent_mir.local_decls[*l]
      },
      _ => {
        tcx.sess
          .span_fatal(source_info.span,
                      "incorrect kernel info intrinsic call");
        return;
      }
    };
    let local_ty = args[0].ty(parent_mir,
                              tcx);
    let local_ty = tcx
      .trans_apply_param_substs(parent_param_substs,
                                &local_ty);
    let expanded = match local_ty.sty {
      TyRef(_, TypeAndMut {
        ty: &ty::TyS {
          sty: TyFnDef(def_id, subs),
          ..
        },
        ..
      }) |
      TyFnDef(def_id, subs) => {
        let expanded = ExpandedKernelInfo {
          format: fmt,
          dest: dest,
          kernel: def_id,
          substs: tcx.trans_apply_param_substs(parent_param_substs,
                                               &subs),
        };
        Some(expanded)
      },
      _ => {
        tcx.sess.span_fatal(source_info.span,
                            "can't expand this type");
        return;
      },
    };

    let expanded = expanded
      .map(|expanded| {
        let mut module =
          ir::builder::Module::new(tcx,
                                   expanded.substs);

        module.build(expanded.kernel);
        (expanded, module.finish())
      });

    if let Some((expanded, kernel_info)) = expanded {
      match expanded.format {
        KernelInfoKind::Json => {
          let s = match to_string_pretty(&kernel_info) {
            Ok(s) => s,
            Err(e) => {
              tcx.sess.span_fatal(source_info.span,
                                  &format!("serialization error: {}", e)[..]);
              return;
            },
          };

          let sym = Symbol::intern(s.as_str());
          let interned = sym.as_str();
          let cv = ConstVal::Str(interned);
          let literal = Literal::Value {
            value: tcx.mk_const(rustc::ty::Const {
              ty: tcx.mk_static_str(),
              val: cv,
            }),
          };
          let constant = Constant {
            span: source_info.span,
            ty: tcx.mk_static_str(),
            literal: literal,
          };
          let constant = Box::new(constant);
          let operand = Operand::Constant(constant);
          let rvalue = Rvalue::Use(operand);
          let stmt_kind = StatementKind::Assign(expanded.dest,
                                                rvalue);
          extra_stmts.push(stmt_kind);
        },
      }
    } else {
      tcx.sess.span_fatal(source_info.span,
                          "unreachable?");
      return;
    }
  }
}
impl Deref for MirToHsaIrPass {
  type Target = GlobalCtx;
  fn deref(&self) -> &Self::Target {
    &self.ctx
  }
}

pub struct ExpandedKernelInfo<'tcx> {
  format: KernelInfoKind,
  dest: mir::Lvalue<'tcx>,
  kernel: DefId,
  substs: &'tcx subst::Substs<'tcx>,
}
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
enum KernelInfoKind {
  Json,
}