
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
extern crate rustc_const_math;
extern crate rustc_driver;
extern crate rustc_mir;
extern crate rustc_plugin;
extern crate rustc_data_structures;
extern crate syntax;
extern crate syntax_pos;
extern crate indexvec;
extern crate serde;
extern crate serde_json;
//extern crate kernel;

use std::rc::Rc;
use std::ops::Deref;

use rustc::hir::def_id::{DefId};
use rustc::mir::{self, SourceInfo};
use rustc_mir::transform::{MirPass};
use rustc::ty::{self, TyCtxt, subst, TyFnDef, TyRef, TypeAndMut, FnSig};
use rustc::traits::MirPluginIntrinsicTrans;
use rustc_plugin::Registry;
use syntax::feature_gate::AttributeType;
//use syntax::ast::NodeId;

//use kernel_info::KernelInfo;

use context::GlobalCtx;
use context::init::ContextLoader;

pub mod context;

pub mod debug;
//pub mod kernel_info;

#[plugin_registrar]
pub fn plugin_registrar(reg: &mut Registry) {
  reg.register_attribute("hsa_lang_item".into(),
                         AttributeType::Normal);

  let ctxt = GlobalCtx::new();

  let kernel_id = Box::new(KernelIdIntrinsicPass {
    ctx: ctxt.clone(),
  });
  reg.register_intrinsic("kernel_id_for".to_string(),
                         kernel_id);

  let kernel_upvars = Box::new(KernelUpvarsIntrinsicPass {
    ctx: ctxt.clone(),
  });
  reg.register_intrinsic("kernel_upvars".to_string(),
                         kernel_upvars);
}

pub enum LangItem {
  Real,
  UInt,
  Int,
  FuncMir,
  BinOpFunction,
}

struct KernelUpvarsIntrinsicPass {
  ctx: GlobalCtx,
}
impl KernelUpvarsIntrinsicPass {

}
impl MirPluginIntrinsicTrans for KernelUpvarsIntrinsicPass {
  fn trans_simple_intrinsic<'a, 'tcx>(&self,
                                      _tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                      _name: &str,
                                      _source_info: SourceInfo,
                                      _sig: &FnSig<'tcx>,
                                      _parent_mir: &mir::Mir<'tcx>,
                                      _parent_param_substs: &'tcx subst::Substs<'tcx>,
                                      _args: &Vec<mir::Operand<'tcx>>,
                                      _dest: mir::Place<'tcx>,
                                      extra_stmts: &mut Vec<mir::StatementKind<'tcx>>)
    where 'tcx: 'a,
  {
    unimplemented!();
  }
}

struct KernelIdIntrinsicPass {
  ctx: GlobalCtx,
}
impl KernelIdIntrinsicPass {

}
impl MirPluginIntrinsicTrans for KernelIdIntrinsicPass {
  fn trans_simple_intrinsic<'a, 'tcx>(&self,
                                      tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                      name: &str,
                                      source_info: SourceInfo,
                                      _sig: &FnSig<'tcx>,
                                      parent_mir: &mir::Mir<'tcx>,
                                      parent_param_substs: &'tcx subst::Substs<'tcx>,
                                      args: &Vec<mir::Operand<'tcx>>,
                                      dest: mir::Place<'tcx>,
                                      extra_stmts: &mut Vec<mir::StatementKind<'tcx>>)
    where 'tcx: 'a,
  {
    use syntax::symbol::{Symbol};
    use rustc_const_math::{ConstInt};
    use rustc::middle::const_val::{ConstVal};
    use rustc::mir::{Literal, Constant, Operand, Rvalue,
                     StatementKind, AggregateKind};
    use rustc::ty::{Const, TyClosure};

    if args.len() != 1 {
      tcx.sess
        .span_fatal(source_info.span,
                    "incorrect kernel info intrinsic call");
    }

    let local_ty = args[0].ty(parent_mir, tcx);
    let local_ty = tcx
      .trans_apply_param_substs(parent_param_substs,
                                &local_ty);

    let reveal_all = rustc::ty::ParamEnv::empty(rustc::traits::Reveal::All);
    let instance = match local_ty.sty {
      TyRef(_, TypeAndMut {
        ty: &ty::TyS {
          sty: TyFnDef(def_id, subs),
          ..
        },
        ..
      }) |
      TyFnDef(def_id, subs) => {
        let subs = tcx
          .trans_apply_param_substs(parent_param_substs,
                                    &subs);

        let msg = format!("local_ty: {:?}, substs: {:?}", tcx.type_of(def_id),
                          subs);
        tcx.sess.span_note_without_error(source_info.span,
                                         &msg[..]);
        rustc::ty::Instance::resolve(tcx, reveal_all, def_id, subs)
          .expect("must be resolvable")
      },

      TyRef(_, TypeAndMut {
        ty: &ty::TyS {
          sty: TyClosure(def_id, subs),
          ..
        },
        ..
      }) |
      TyClosure(def_id, subs) => {
        let subs = tcx
          .trans_apply_param_substs(parent_param_substs,
                                    &subs);
        let msg = format!("local_ty: {:?}, substs: {:?}", tcx.type_of(def_id),
                          subs);
        tcx.sess.span_note_without_error(source_info.span,
                                         &msg[..]);
        let env = subs.closure_kind(def_id, tcx);
        rustc_mir::monomorphize::resolve_closure(tcx, def_id, subs, env)
      },

      _ => {
        match local_ty.sty {
          TyRef(_, TypeAndMut {
            ty: &ty::TyS {
              sty: TyClosure(d, subs),
              ..
            },
            ..
          }) |
          TyClosure(d, subs) => {
            let trait_def_id = tcx.trait_of_item(d);
            let msg = format!("trait_def_id: {:?}", trait_def_id);
            tcx.sess.span_note_without_error(source_info.span,
                                             &msg[..]);
          },
          _ => {},
        }
        tcx.sess.span_fatal(source_info.span,
                            "can't expand this type");
      },
    };

    let def_id = instance.def_id();

    let crate_name = tcx.crate_name(def_id.krate);
    let crate_name = ConstVal::Str(crate_name.as_str());
    let crate_name = tcx.mk_const(Const {
      ty: tcx.mk_static_str(),
      val: crate_name,
    });
    let crate_name = Literal::Value {
      value: crate_name,
    };
    let crate_name = Constant {
      span: source_info.span,
      ty: tcx.mk_static_str(),
      literal: crate_name,
    };
    let crate_name = Box::new(crate_name);
    let crate_name = Operand::Constant(crate_name);

    let mk_u64 = |v: u64| {
      let v = ConstInt::U64(v);
      let v = ConstVal::Integral(v);
      let v = tcx.mk_const(Const {
        ty: tcx.types.u64,
        val: v,
      });
      let v = Literal::Value {
        value: v,
      };
      let v = Constant {
        span: source_info.span,
        ty: tcx.types.u64,
        literal: v,
      };
      let v = Box::new(v);
      Operand::Constant(v)
    };

    let disambiguator = tcx.crate_disambiguator(def_id.krate);
    let (d_hi, d_lo) = disambiguator.to_fingerprint().as_value();
    let d_hi = mk_u64(d_hi);
    let d_lo = mk_u64(d_lo);

    let id = mk_u64(def_id.index.as_raw_u32() as u64);

    let rvalue = Rvalue::Aggregate(Box::new(AggregateKind::Tuple),
                                   vec![crate_name, d_hi, d_lo, id]);
    let stmt_kind = StatementKind::Assign(dest, rvalue);
    extra_stmts.push(stmt_kind);
  }
}
impl Deref for KernelIdIntrinsicPass {
  type Target = GlobalCtx;
  fn deref(&self) -> &Self::Target {
    &self.ctx
  }
}
