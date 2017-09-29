
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
                TyParam, };
use rustc_plugin::Registry;
use syntax::feature_gate::AttributeType;
use syntax_pos::Span;
//use syntax::ast::NodeId;

//use kernel_info::KernelInfo;

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

  reg.register_intrinsic("json_kernel_info_for".to_string());

  //let p = Rc::new(debug::Debug::new());
  //reg.register_post_optimization_mir_pass(p as Rc<MirPass>);

  let ctxt = GlobalCtx::new();
  let init = Rc::new(ContextLoader::new(&ctxt));
  reg.register_post_optimization_mir_pass(init as Rc<MirPass>);

  let compiletime = Rc::new(MirToHsaIrPass {
    ctx: ctxt.clone(),
  });
  reg.register_post_optimization_mir_pass(compiletime as Rc<MirPass>);
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
                            subs: &subst::Substs<'tcx>)
    -> Option<ir::Function>
  {
    use ir::*;
    use ir::builder::RustCConvert;

    use rustc_data_structures::indexed_vec::Idx;

    assert!(subs.len() == 0, "generics TODO");

    tcx.sess
      .span_note_without_error(mir.span,
                               "here");

    let mut fun =
      builder::Function::new(mir.span.into(), tcx);
    let return_ty = fun.convert(&mir.return_ty);
    fun.fun.return_ty = Some(return_ty);

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
        } => TerminatorKind::Call {
          func: fun.convert(func),
          args: args.iter()
            .map(|v| fun.convert(v) )
            .collect(),
          destination: destination.as_ref()
            .map(|&(ref dest, destbb)| {
              (fun.convert(dest), destbb.into())
            }),
          cleanup: cleanup.as_ref()
            .map(|&bb| bb.into() ),
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

      let new_idx = fun.basic_blocks.push(kbb);
      assert_eq!(new_idx.index(), bbid.index());
    }

    for (id, vis) in mir.visibility_scopes.iter_enumerated() {
      let kvis = VisibilityScopeData {
        span: vis.span.into(),
        parent_scope: vis.parent_scope
          .map(|v| v.into() ),
      };

      let kid = fun.visibility_scopes
        .push(kvis);
      assert_eq!(kid.index(), id.index());
    }

    for (id, promoted) in mir.promoted.iter_enumerated() {
      let f = self.build_hsa_ir(tcx, &promoted,
                                subst::Substs::empty());
      if let Some(f) = f {
        let kid = fun.promoted
          .push(f);
        assert_eq!(kid.index(), id.index());
      } else {
        return None;
      }
    }

    for (id, ldecl) in mir.local_decls.iter_enumerated() {
      let kldecl = LocalDecl {
        mutability: ldecl.mutability.into(),
        is_user_variable: ldecl.is_user_variable,
        ty: fun.convert(&ldecl.ty),
        name: ldecl.name.map(|v| v.into()),
        source_info: ldecl.source_info.into(),
      };

      let kid = fun.local_decls
        .push(kldecl);
      assert_eq!(kid.index(), id.index());
    }

    for upvar in mir.upvar_decls.iter() {
      let kupvar = UpvarDecl {
        debug_name: upvar.debug_name.into(),
        by_ref: upvar.by_ref,
      };

      fun.upvar_decls.push(kupvar);
    }

    fun.spread_arg = mir.spread_arg.map(|v| v.into() );
    fun.span = mir.span.into();

    Some(fun.finish())
  }
}
impl MirPass for MirToHsaIrPass {
  fn run_pass<'a, 'tcx>(&self,
                        tcx: TyCtxt<'a, 'tcx, 'tcx>,
                        src: MirSource,
                        mir: &mut Mir<'tcx>)
  {
    use serde_json::{to_string_pretty};

    use syntax::symbol::{Symbol};
    use rustc::middle::const_val::{ConstVal};
    use rustc::mir::{Literal, Constant, Operand, Rvalue,
                     StatementKind, Statement, Terminator};
    use rustc_data_structures::indexed_vec::Idx;
    use rustc_mir::util::{write_mir_fn};

    tcx.sess.span_note_without_error(mir.span,
                                     "passing over this");
    println!("source: {:?}", src);
    let def_id = match src {
      MirSource::Fn(node_id) => {
        tcx.hir.local_def_id(node_id)
      },
      _ => { return; }
    };
    let self_ty = tcx.type_of(def_id);
    let (fnty_def_id, fnty_subs) = match self_ty.sty {
      TyFnDef(def_id, subs) => (def_id, subs),
      _ => unreachable!(),
    };

    println!("ty: {:?}", self_ty);
    println!("substs: {:?}", fnty_subs);
    if fnty_subs.len() > 0 {
      let ty = fnty_subs.type_at(0);
      println!("F: {:?}", ty);
    }
    let generics   = tcx.generics_of(def_id);
    let predicates = tcx.predicates_of(def_id);
    let inst_predicates = predicates.instantiate(tcx, fnty_subs);
    println!("inst_generics: {:?}", inst_predicates);

    let expanded: Vec<_> = mir.basic_blocks()
      .iter_enumerated()
      .filter_map(|(id, bb)| {
        let terminator = bb.terminator.as_ref()
          .unwrap();
        terminator.kind
          .expand_kernel_info(tcx, &generics, &inst_predicates,
                              mir, &self.ctx)
          .map(|v| (id, v) )
      })
      .filter_map(|(id, expanded)| {
        self.build_hsa_ir(tcx,
                          expanded.kernel,
                          expanded.substs)
          .map(|v| (id, (expanded, v)) )
      })
      .collect();

    let span = mir.span;

    for (bb_id, (expanded, kernel_info)) in expanded {
      let bb = &mut mir[bb_id];
      let terminator = bb.terminator.take().unwrap();

      match expanded.format {
        KernelInfoKind::Json => {
          let s = match to_string_pretty(&kernel_info) {
            Ok(s) => s,
            Err(e) => {
              tcx.sess.span_fatal(span,
                                  &format!("serialization error: {}",
                                          e)[..]);
              continue;
            },
          };

          println!("kernal json: {}", s);

          let sym = Symbol::intern(s.as_str());
          let interned = sym.as_str();
          let cv = ConstVal::Str(interned);
          let literal = Literal::Value {
            value: cv,
          };
          let constant = Constant {
            span: terminator.source_info.span,
            ty: tcx.mk_static_str(),
            literal: literal,
          };
          let constant = Box::new(constant);
          let operand = Operand::Constant(constant);
          let rvalue = Rvalue::Use(operand);
          let stmt_kind = StatementKind::Assign(expanded.dest,
                                                rvalue);
          let stmt = Statement {
            source_info: terminator.source_info,
            kind: stmt_kind,
          };
          bb.statements
            .push(stmt);

          let new_term = Terminator {
            source_info: terminator.source_info,
            kind: TerminatorKind::Goto {
              target: expanded.target,
            },
          };
          bb.terminator = Some(new_term);
        },
      }
    }

    let mut stdout = std::io::stdout();
    write_mir_fn(tcx, src, mir, &mut stdout).unwrap();
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
  target: mir::BasicBlock,
  kernel: &'tcx mir::Mir<'tcx>,
  substs: &'tcx subst::Substs<'tcx>,
}
pub trait IsTerminatorTheKernelOne<'tcx> {
  fn expand_kernel_info<'a>(&self,
                            tcx: TyCtxt<'a, 'tcx, 'tcx>,
                            generics: &'tcx ty::Generics,
                            preds: &ty::InstantiatedPredicates<'tcx>,
                            mir: &mir::Mir<'tcx>,
                            gctxt: &GlobalCtx)
    -> Option<ExpandedKernelInfo<'tcx>>;
}
impl<'tcx> IsTerminatorTheKernelOne<'tcx> for rustc::mir::TerminatorKind<'tcx> {
  fn expand_kernel_info<'a>(&self,
                            tcx: TyCtxt<'a, 'tcx, 'tcx>,
                            generics: &'tcx ty::Generics,
                            preds: &ty::InstantiatedPredicates<'tcx>,
                            mir: &mir::Mir<'tcx>,
                            gctxt: &GlobalCtx)
    -> Option<ExpandedKernelInfo<'tcx>>
  {
    use rustc::middle::const_val::ConstVal;

    match self {
      &TerminatorKind::Call {
        ref func,
        ref args,
        ref destination,
        ..
      } => {
        match func {
          &mir::Operand::Constant(box mir::Constant {
            literal: mir::Literal::Value {
              value: ConstVal::Function(ref def_id, substs),
            },
            span: sp,
            ..
          }) if def_id.krate == gctxt.compiletime_crate_num() => {
            let attrs = tcx.item_attrs(*def_id);
            let kfmt = kernel_info_attribute(tcx.sess,
                                             attrs.as_ref());
            let fmt = if let Some(kfmt) = kfmt {
              kfmt
            } else {
              return None;
            };

            if args.len() == 0 {
              tcx.sess
                .span_fatal(sp, "incorrect kernel info intrinsic call");
              return None;
            }

            let local = match &args[0] {
              &mir::Operand::Consume(mir::Lvalue::Local(ref l)) => {
                &mir.local_decls[*l]
              },
              _ => {
                tcx.sess
                  .span_fatal(sp, "incorrect kernel info intrinsic call");
                return None;
              }
            };
            match local.ty.sty {
              TyRef(_, TypeAndMut {
                ty: &ty::TyS {
                  sty: TyFnDef(def_id, subs),
                  ..
                },
                ..
              }) |
              TyFnDef(def_id, subs) => {
                let mir = tcx.optimized_mir(def_id);
                let (lvalue, target) = destination.clone().unwrap();
                let expanded = ExpandedKernelInfo {
                  format: fmt,
                  dest: lvalue,
                  target: target,
                  kernel: mir,
                  substs: subs,
                };
                Some(expanded)
              },
              TyRef(_, TypeAndMut {
                ty: &ty::TyS {
                  sty: TyParam(ref param_ty),
                  ..
                },
                ..
              }) => {
                let ty_param = generics.type_param(param_ty);
                println!("ty_param.index: {}", ty_param.index);
                let fn_pred = &preds
                  .predicates[ty_param.index as usize];
                let str = format!("fn_pred {:?}", fn_pred);
                tcx.sess.span_note_without_error(sp, &str[..]);
                None
              },
              _ => {
                println!("{:?}", local.ty.sty);
                None
              }
            }
          },
          _ => None,
        }


      },
      _ => None
    }
  }
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
enum KernelInfoKind {
  Json,
}
fn kernel_info_attribute(sess: &Session,
                         attrs: &[syntax::ast::Attribute])
  -> Option<KernelInfoKind>
{
  use syntax::tokenstream::{TokenStream,
                            TokenTree, Delimited};
  use syntax::parse::token::{DelimToken, Token, Lit};
  'outer: for attr in attrs.iter() {
    if attr.path != "hsa_lang_item" { continue; }

    let mut trees = attr.tokens.trees();
    match trees.next() {
      None => { continue; }
      Some(TokenTree::Delimited(sp1, Delimited {
        delim: DelimToken::Paren,
        tts,
      })) => {
        let tts: TokenStream = tts.into();
        let mut ttss = tts.into_trees();
        loop {
          match ttss.next() {
            None => { continue 'outer; },

            Some(TokenTree::Token(sp2, Token::Ident(ident)))
            if ident.name == "kernel_info" => {
              let tt = ttss.next();
              if tt.is_none() || match tt.unwrap() {
                TokenTree::Token(_, Token::Eq) => false,
                _ => true,
              } {
                sess.span_fatal(sp2, "expected `=`");
                return None;
              }

              match ttss.next() {
                Some(TokenTree::Token(_, Token::Literal(Lit::Str_(fmt), _)))
                if fmt == "json" => {
                  return Some(KernelInfoKind::Json);
                },

                ref tt if tt.is_some() => {
                  println!("{:?}", tt);
                  sess.span_fatal(sp2, "unknown kernel info format");
                  return None;
                },
                _ => {
                  sess.span_fatal(sp2, "missing kernel info format");
                  return None;
                },
              }
            },

            Some(_) => {},
          }
        }
      },
      _ => {}
    }
  }

  None
}