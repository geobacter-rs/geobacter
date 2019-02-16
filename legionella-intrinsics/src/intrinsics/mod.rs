
use std::fmt;

use crate::rustc::hir::def_id::{DefId, };
use crate::rustc::middle::lang_items::{self, LangItem, };
use crate::rustc::mir::{Constant, Operand, Rvalue, Statement,
                 StatementKind, AggregateKind, };
use crate::rustc::mir::interpret::{ConstValue, Scalar, };
use crate::rustc::mir::{self, LocalDecl, Place, };
use crate::rustc::ty::{self, TyCtxt, Instance, layout::Size, };
use crate::rustc::ty::{Const, };
use crate::rustc_data_structures::indexed_vec::*;
use crate::syntax_pos::{DUMMY_SP, Span, };

use super::{DefIdFromKernelId, LegionellaCustomIntrinsicMirGen, };

use rustc_intrinsics::help::LegionellaTyCtxtHelp;

use crate::lcore::*;
use crate::stubbing;

pub mod shader;
pub mod vk;

#[derive(Debug, Clone, Copy)]
pub enum Arch {
  AmdGpu,
}
// allow dead_code, for now. Might start using this again.
#[allow(dead_code)]
fn redirect_or_panic<'a, 'tcx, F>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                  mir: &mut mir::Mir<'tcx>,
                                  f: F)
  where F: FnOnce(Option<Arch>) -> Option<Instance<'tcx>>,
{
  pub fn langcall(tcx: TyCtxt,
                  span: Option<Span>,
                  msg: &str,
                  li: LangItem)
                  -> DefId
  {
    tcx.lang_items().require(li).unwrap_or_else(|s| {
      let msg = format!("{} {}", msg, s);
      match span {
        Some(span) => tcx.sess.span_fatal(span, &msg[..]),
        None => tcx.sess.fatal(&msg[..]),
      }
    })
  }

  fn static_str_operand<'tcx, T>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                                 source_info: mir::SourceInfo,
                                 str: T) -> Operand<'tcx>
    where T: fmt::Display,
  {
    let str = format!("{}", str);
    let id = tcx.allocate_bytes(str.as_bytes());
    let v = ConstValue::new_slice(Scalar::Ptr(id.into()),
                                  str.len() as u64);
    let v = tcx.mk_const(Const {
      ty: tcx.mk_static_str(),
      val: v,
    });
    let v = Constant {
      span: source_info.span,
      ty: tcx.mk_static_str(),
      literal: v,
      user_ty: None,
    };
    let v = Box::new(v);
    Operand::Constant(v)
  }

  let source_info = mir::SourceInfo {
    span: DUMMY_SP,
    scope: mir::OUTERMOST_SOURCE_SCOPE,
  };

  let mk_u32 = |v: u32| {
    let v = Scalar::from_uint(v, Size::from_bytes(4));
    let v = ConstValue::Scalar(v);
    let v = tcx.mk_const(Const {
      ty: tcx.types.u32,
      val: v,
    });
    let v = Constant {
      span: source_info.span,
      ty: tcx.types.u32,
      literal: v,
      user_ty: None,
    };
    let v = Box::new(v);
    Operand::Constant(v)
  };

  let mut bb = mir::BasicBlockData {
    statements: Vec::new(),
    terminator: Some(mir::Terminator {
      source_info: source_info.clone(),
      kind: mir::TerminatorKind::Return,
    }),

    is_cleanup: false,
  };

  let arch = match &tcx.sess.target.target.arch[..] {
    "amdgpu" => Some(Arch::AmdGpu),
    _ => None,
  };

  let (real_instance, args, term_kind) = match f(arch) {
    Some(instance) => {
      info!("intrinsic: {:?}", instance);
      (instance, vec![], mir::TerminatorKind::Return)
    },
    None => {
      // call `panic` from `libcore`
      // `fn panic(expr_file_line_col: &(&'static str, &'static str, u32, u32)) -> !`
      let lang_item = lang_items::PanicFnLangItem;

      let expr = static_str_operand(tcx, source_info.clone(),
                                    "TODO");
      let file = static_str_operand(tcx, source_info.clone(),
                                    "TODO");
      let line = mk_u32(0); // TODO
      let col  = mk_u32(0); // TODO
      let rvalue = Rvalue::Aggregate(Box::new(AggregateKind::Tuple),
                                     vec![expr, file, line, col]);
      let arg_ty = tcx.intern_tup(&[
        tcx.mk_static_str(),
        tcx.mk_static_str(),
        tcx.types.u32,
        tcx.types.u32,
      ]);
      let arg_local = LocalDecl::new_temp(arg_ty, DUMMY_SP);
      let arg_local_id = Place::Local(mir.local_decls.next_index());
      mir.local_decls.push(arg_local);
      let stmt_kind = StatementKind::Assign(arg_local_id.clone(),
                                            Box::new(rvalue));
      let stmt = Statement {
        source_info: source_info.clone(),
        kind: stmt_kind,
      };
      bb.statements.push(stmt);

      let arg_ref_ty = tcx.mk_imm_ref(tcx.types.re_erased, arg_ty);
      let arg_ref_local = LocalDecl::new_temp(arg_ref_ty, DUMMY_SP);
      let arg_ref_local_id = Place::Local(mir.local_decls.next_index());
      mir.local_decls.push(arg_ref_local);
      let rvalue = Rvalue::Ref(tcx.types.re_erased,
                               mir::BorrowKind::Shared,
                               arg_local_id);
      let stmt_kind = StatementKind::Assign(arg_ref_local_id.clone(),
                                            Box::new(rvalue));
      let stmt = Statement {
        source_info: source_info.clone(),
        kind: stmt_kind,
      };
      bb.statements.push(stmt);

      let def_id = langcall(tcx, None, "", lang_item);
      let instance = Instance::mono(tcx, def_id);

      (instance,
       vec![Operand::Copy(arg_ref_local_id), ],
       mir::TerminatorKind::Unreachable)
    },
  };
  info!("mirgen intrinsic into {}", real_instance);
  let success = mir::BasicBlock::new(mir.basic_blocks().next_index().index() + 1);
  let fn_ty = real_instance.ty(tcx);
  bb.terminator.as_mut()
    .unwrap()
    .kind = mir::TerminatorKind::Call {
    func: tcx.mk_const_op(source_info.clone(),
                          ty::Const::zero_sized(fn_ty)),
    args,
    destination: Some((Place::Local(mir::RETURN_PLACE),
                       success)),
    cleanup: None,
    from_hir_call: false,
  };
  mir.basic_blocks_mut().push(bb);
  let bb = mir::BasicBlockData {
    statements: Vec::new(),
    terminator: Some(mir::Terminator {
      source_info: source_info.clone(),
      kind: term_kind,
    }),

    is_cleanup: false,
  };
  mir.basic_blocks_mut().push(bb);
}

pub struct ExeModel(pub Option<ExecutionModel>);
impl LegionellaCustomIntrinsicMirGen for ExeModel {
  fn mirgen_simple_intrinsic<'a, 'tcx>(&self,
                                       _stubs: &stubbing::Stubber,
                                       _kid_did: &dyn DefIdFromKernelId,
                                       tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       _instance: ty::Instance<'tcx>,
                                       mir: &mut mir::Mir<'tcx>)
    where 'tcx: 'a,
  {
    info!("mirgen intrinsic {}", self);

    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let mk_u32 = |v: u32| {
      let v = Scalar::from_uint(v, Size::from_bytes(4));
      let v = ConstValue::Scalar(v);
      tcx.mk_const_op(source_info.clone(), Const {
        ty: tcx.types.u32,
        val: v,
      })
    };

    let ret = mir::Place::Local(mir::RETURN_PLACE);
    let rvalue = match self.0 {
      None => 0,
      Some(v) => (v as u32) + 1,
    };
    let rvalue = Rvalue::Use(mk_u32(rvalue));

    let stmt_kind = StatementKind::Assign(ret, Box::new(rvalue));
    let stmt = Statement {
      source_info: source_info.clone(),
      kind: stmt_kind,
    };

    let bb = mir::BasicBlockData {
      statements: vec![stmt],
      terminator: Some(mir::Terminator {
        source_info,
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };
    mir.basic_blocks_mut().push(bb);
  }

  fn generic_parameter_count<'a, 'tcx>(&self, _tcx: TyCtxt<'a, 'tcx, 'tcx>)
    -> usize
  {
    0
  }
  /// The types of the input args.
  fn inputs<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    tcx.intern_type_list(&[])
  }
  /// The return type.
  fn output<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>) -> ty::Ty<'tcx> {
    tcx.types.u32
  }
}

impl fmt::Display for ExeModel {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__legionella_exe_model")
  }
}
