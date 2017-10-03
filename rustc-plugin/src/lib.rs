
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
  fn build_hsa_ir<'a, 'tcx>(&self,
                            tcx: TyCtxt<'a, 'tcx, 'tcx>,
                            mir: &mir::Mir<'tcx>,
                            mut builder: builder::Function)
  {
    use ir::*;
    use ir::builder::RustCConvert;

    use rustc_data_structures::indexed_vec::Idx;

    tcx.sess
      .span_note_without_error(mir.span,
                               "here");
    let fun = builder.function_kind_mut();

    let return_ty = tcx.trans_apply_param_substs(subs,
                                                 &mir.return_ty);
    let return_ty = fun.convert(&return_ty);
    fun.mod_
      .funcs[MAIN_FUNCTION]
      .return_ty = Some(return_ty);

    for (bbid, bb) in mir.basic_blocks().iter_enumerated() {
      let mut kstmts = Vec::new();

      for stmt in bb.statements.iter() {
        let kkind = match stmt.kind {
          mir::StatementKind::Assign(ref l, ref r) => {
            StatementKind::Assign(fun.convert(l),
                                  fun.convert(r))
          },
          mir::StatementKind::SetDiscriminant {
            ref lvalue, variant_index,
          } => StatementKind::SetDiscriminant {
            lvalue: fun.convert(lvalue),
            variant_index: variant_index,
          },
          mir::StatementKind::StorageLive(ref l) => {
            StatementKind::StorageLive(fun.convert(l))
          },
          mir::StatementKind::StorageDead(ref l) => {
            StatementKind::StorageDead(fun.convert(l))
          },
          mir::StatementKind::InlineAsm { .. } => {
            tcx.sess.span_fatal(stmt.source_info.span,
                                "inline asm not allowed\
                                 for hsa functions");
            StatementKind::Nop
          },
          mir::StatementKind::Validate(..) => {
            StatementKind::Nop
          },
          mir::StatementKind::EndRegion(extent) => {
            unreachable!()
          },
          mir::StatementKind::Nop => {
            StatementKind::Nop
          },
        };

        let mut kstmt = Statement {
          source_info: stmt.source_info.into(),
          kind: kkind,
        };

        kstmts.push(kstmt);
      }

      let terminator = bb.terminator.as_ref().unwrap();
      let ktkind = match &terminator.kind {
        &mir::TerminatorKind::Goto {
          target,
        } => TerminatorKind::Goto { target: target.into() },
        &mir::TerminatorKind::SwitchInt {
          ref discr, ref switch_ty, ref values, ref targets,
        } => TerminatorKind::SwitchInt {
          discr: fun.convert(discr),
          switch_ty: fun.convert(switch_ty),
          values: values.iter()
            .map(|&c| c.into() )
            .collect(),
          targets: targets.iter()
            .map(|&bb| bb.into() )
            .collect(),
        },
        &mir::TerminatorKind::Resume => TerminatorKind::Resume,
        &mir::TerminatorKind::Return => TerminatorKind::Return,
        &mir::TerminatorKind::Unreachable => TerminatorKind::Unreachable,
        &mir::TerminatorKind::Drop {
          ref location, target, ref unwind,
        } => TerminatorKind::Drop {
          location: fun.convert(location),
          target: target.into(),
          unwind: unwind.map(|v| v.into() ),
        },
        &mir::TerminatorKind::DropAndReplace {
          ref location,
          ref value,
          target,
          ref unwind,
        } => TerminatorKind::DropAndReplace {
          location: fun.convert(location),
          value: fun.convert(value),
          target: target.into(),
          unwind: unwind.map(|v| v.into() ),
        },
        &mir::TerminatorKind::Call {
          ref func, ref args,
          ref destination,
          ref cleanup,
        } => {
          let fty = func.ty(mir, tcx);
          let finstance = match fty.sty {
            ty::TyFnDef(callee_def_id, callee_substs) => {
              monomorphize::resolve(tcx, callee_def_id, callee_substs)
            },
            ty::TyFnPtr(_) => {
              tcx.sess.span_fatal(terminator.source_info.span,
                                  "function ptr calls not implemented");
            },
            _ => bug!("{} is not callable", fty),
          };

          match finstance.def {
            ty::instance::InstanceDef::Item(fdef_id) => {

            },
            ty::instance::InstanceDef::Intrinsic(idef_id) => {
              tcx.item_name(idef_id)
            },
            _ => {
              bug!("TODO: function instance: {:?}", finstance);
            }
          }

          TerminatorKind::Call {
            func: fun.convert(func),
            args: args.iter()
              .map(|v| fun.convert(v))
              .collect(),
            destination: destination.as_ref()
              .map(|&(ref dest, destbb)| {
                (fun.convert(dest), destbb.into())
              }),
            cleanup: cleanup.as_ref()
              .map(|&bb| bb.into()),
          }
        },
        &mir::TerminatorKind::Assert { target, .. } => {
          span_bug!(terminator.source_info.span,
                    "terminator assert unimplemented");
          TerminatorKind::Goto { target: target.into(), }
        }
      };

      let kterm = Terminator {
        source_info: terminator.source_info.into(),
        kind: ktkind,
      };

      let kbb = BasicBlockData {
        statements: kstmts,
        terminator: kterm,
        is_cleanup: bb.is_cleanup,
      };

      let new_idx = fun.mod_
        .funcs[MAIN_FUNCTION]
        .basic_blocks
        .push(kbb);
      assert_eq!(new_idx.index(), bbid.index());
    }

    for (id, vis) in mir.visibility_scopes.iter_enumerated() {
      let kvis = VisibilityScopeData {
        span: vis.span.into(),
        parent_scope: vis.parent_scope
          .map(|v| v.into() ),
      };

      let kid = fun
        .funcs[MAIN_FUNCTION]
        .visibility_scopes
        .push(kvis);
      assert_eq!(kid.index(), id.index());
    }

    for (id, promoted) in mir.promoted.iter_enumerated() {
      unimplemented!();
      let f = self.build_hsa_ir(tcx, &promoted,
                                subst::Substs::empty());
      if let Some(f) = f {
        let kid = fun.mod_
          .funcs[MAIN_FUNCTION]
          .promoted
          .push(f);
        assert_eq!(kid.index(), id.index());
      } else {
        return None;
      }
    }

    for (id, ldecl) in mir.local_decls.iter_enumerated() {
      let ldecl_ty = ldecl.ty;
      let ldecl_ty = tcx
        .trans_apply_param_substs(subs, &ldecl_ty);
      let kldecl = LocalDecl {
        mutability: ldecl.mutability.into(),
        is_user_variable: ldecl.is_user_variable,
        ty: fun.convert(&ldecl_ty),
        name: ldecl.name.map(|v| v.into()),
        source_info: ldecl.source_info.into(),
      };

      let kid = fun.mod_
        .funcs[MAIN_FUNCTION]
        .local_decls
        .push(kldecl);
      assert_eq!(kid.index(), id.index());
    }

    for upvar in mir.upvar_decls.iter() {
      let kupvar = UpvarDecl {
        debug_name: upvar.debug_name.into(),
        by_ref: upvar.by_ref,
      };

      fun.mod_
        .funcs[MAIN_FUNCTION]
        .upvar_decls
        .push(kupvar);
    }

    fun.mod_
      .funcs[MAIN_FUNCTION]
      .spread_arg = mir.spread_arg.map(|v| v.into() );
    fun.mod_
      .funcs[MAIN_FUNCTION]
      .span = mir.span.into();

    Some(fun.finish())
  }
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
                                      extra_stmts: &mut Vec<mir::StatementKind<'tcx>>) {
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
        let mir = tcx.optimized_mir(def_id);
        let expanded = ExpandedKernelInfo {
          format: fmt,
          dest: dest,
          kernel: mir,
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
      .and_then(|expanded| {
        self.build_hsa_ir(tcx,
                          expanded.kernel,
                          expanded.substs)
          .map(|v| (expanded, v) )
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
            value: rustc::ty::Const {
              ty: tcx.mk_static_str(),
              val: cv,
            },
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
  kernel: &'tcx mir::Mir<'tcx>,
  substs: &'tcx subst::Substs<'tcx>,
}
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
enum KernelInfoKind {
  Json,
}