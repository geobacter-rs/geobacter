
use std::fmt;

use hsa_core::kernel::{KernelId, kernel_id_for, };

use crate::rustc::hir::def_id::{DefId, };
use crate::rustc::middle::lang_items::{self, LangItem, };
use crate::rustc::mir::{Constant, Operand, Rvalue, Statement,
                 StatementKind, AggregateKind, };
use crate::rustc::mir::interpret::{ConstValue, Scalar, };
use crate::rustc::mir::{self, LocalDecl, Place, };
use crate::rustc::ty::{self, TyCtxt, Instance, InstanceDef, layout::Size, };
use crate::rustc::ty::{Const, };
use crate::rustc_data_structures::indexed_vec::*;
use crate::syntax_pos::{DUMMY_SP, Span, };

use super::{DefIdFromKernelId, LegionellaCustomIntrinsicMirGen, };

#[derive(Debug, Clone, Copy)]
pub enum Arch {
  AmdGpu,
}

#[derive(Debug, Clone, Copy)]
pub enum Dim {
  X,
  Y,
  Z,
}
impl Dim {
  fn name(&self) -> &'static str {
    match self {
      &Dim::X => "x",
      &Dim::Y => "y",
      &Dim::Z => "z",
    }
  }
}
impl fmt::Display for Dim {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.write_str(self.name())
  }
}
#[derive(Debug, Clone, Copy)]
pub enum BlockLevel {
  Item,
  Group,
}
impl BlockLevel {
  fn name(&self) -> &'static str {
    match self {
      &BlockLevel::Item => "workitem",
      &BlockLevel::Group => "workgroup",
    }
  }
}
impl fmt::Display for BlockLevel {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.write_str(self.name())
  }
}

#[derive(Debug, Clone, Copy)]
pub struct AxisId {
  block: BlockLevel,
  dim: Dim,
}
impl AxisId {
  pub fn permutations() -> Vec<Self> {
    let mut out = vec![];
    for &block in [BlockLevel::Group, BlockLevel::Item, ].into_iter() {
      for &dim in [Dim::X, Dim::Y, Dim::Z, ].into_iter() {
        out.push(AxisId {
          block,
          dim,
        });
      }
    }

    out
  }
  fn kernel_id(&self, arch: Option<Arch>) -> Option<KernelId> {
    match arch {
      Some(Arch::AmdGpu) => Some(match self {
        &AxisId {
          block: BlockLevel::Item,
          dim: Dim::X,
        } => {
          kernel_id_for(&amdgcn_intrinsics::amdgcn_workitem_id_x)
        },
        &AxisId {
          block: BlockLevel::Item,
          dim: Dim::Y,
        } => {
          kernel_id_for(&amdgcn_intrinsics::amdgcn_workitem_id_y)
        },
        &AxisId {
          block: BlockLevel::Item,
          dim: Dim::Z,
        } => {
          kernel_id_for(&amdgcn_intrinsics::amdgcn_workitem_id_z)
        },
        &AxisId {
          block: BlockLevel::Group,
          dim: Dim::X,
        } => {
          kernel_id_for(&amdgcn_intrinsics::amdgcn_workgroup_id_x)
        },
        &AxisId {
          block: BlockLevel::Group,
          dim: Dim::Y,
        } => {
          kernel_id_for(&amdgcn_intrinsics::amdgcn_workgroup_id_y)
        },
        &AxisId {
          block: BlockLevel::Group,
          dim: Dim::Z,
        } => {
          kernel_id_for(&amdgcn_intrinsics::amdgcn_workgroup_id_z)
        },
      }),
      None => None,
    }
  }
  fn instance<'tcx>(&self,
                    kid_did: &dyn DefIdFromKernelId,
                    arch: Option<Arch>,
                    tcx: TyCtxt<'_, 'tcx, 'tcx>)
                    -> Option<Instance<'tcx>>
  {
    let id = self.kernel_id(arch);
    if id.is_none() { return None; }
    let id = id.unwrap();

    let def_id = kid_did.convert_kernel_id(id)
      .expect("failed to convert kernel id to def id");

    let def = InstanceDef::Intrinsic(def_id);
    let substs = tcx.intern_substs(&[]);

    Some(Instance {
      def,
      substs,
    })
  }
}
impl LegionellaCustomIntrinsicMirGen for AxisId {
  fn mirgen_simple_intrinsic<'a, 'tcx>(&self,
                                       kid_did: &dyn DefIdFromKernelId,
                                       tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       _instance: ty::Instance<'tcx>,
                                       mir: &mut mir::Mir<'tcx>)
    where 'tcx: 'a,
  {
    info!("mirgen intrinsic {}", self);

    redirect_or_panic(tcx, mir, move |arch| {
      self.instance(kid_did, arch, tcx)
    });
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
    return tcx.types.u32;
  }
}

impl fmt::Display for AxisId {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__legionella_{}_{}_id", self.block, self.dim)
  }
}

pub struct DispatchPtr;

impl DispatchPtr {
  // WOWIE. Okay, so the intrinsic wrappers can't be referenced in
  // anything other than "trivial" functions.
  // For example, if the body of this function is placed in the closure
  // passed to `redirect_or_panic` in `DispatchPtr::mirgen_simple_intrinsic`,
  // rustc tries to codegen `amdgcn_intrinsics::amdgcn_dispatch_ptr`, which
  // contains a call to the platform specific intrinsic `amdgcn_dispatch_ptr`
  // which can't be defined correctly, as on, eg, x86_64, the readonly address
  // space is not 4. LLVM then reports a fatal error b/c the intrinsic has an
  // incorrect type.
  // When referenced in a function like this, the wrapper function isn't
  // codegenned.
  fn amdgcn_kernel_id(&self) -> KernelId {
    kernel_id_for(&amdgcn_intrinsics::amdgcn_dispatch_ptr)
  }
}

impl LegionellaCustomIntrinsicMirGen for DispatchPtr {
  fn mirgen_simple_intrinsic<'a, 'tcx>(&self,
                                       kid_did: &dyn DefIdFromKernelId,
                                       tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       _instance: ty::Instance<'tcx>,
                                       mir: &mut mir::Mir<'tcx>)
    where 'tcx: 'a,
  {
    info!("mirgen intrinsic {}", self);

    redirect_or_panic(tcx, mir, move |arch| {
      let id = match arch {
        Some(Arch::AmdGpu) => self.amdgcn_kernel_id(),
        None => { return None; },
      };

      let def_id = kid_did.convert_kernel_id(id)
        .expect("failed to convert kernel id to def id");
      let def = InstanceDef::Intrinsic(def_id);
      let substs = tcx.intern_substs(&[]);

      Some(Instance {
        def,
        substs,
      })
    });
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
    tcx.mk_imm_ptr(tcx.types.u8)
  }
}

impl fmt::Display for DispatchPtr {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__legionella_dispatch_ptr")
  }
}
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
                                  str.len() as u64,
                                  tcx);
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
  let fn_ty = tcx.type_of(real_instance.def_id());
  bb.terminator.as_mut()
    .unwrap()
    .kind = mir::TerminatorKind::Call {
    func: Operand::Constant(Box::new(Constant {
      span: DUMMY_SP,
      ty: real_instance.ty(tcx),
      user_ty: None,
      literal: ty::Const::zero_sized(tcx, fn_ty),
    })),
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

mod amdgcn_intrinsics {
  // unsafe functions don't implement `std::opts::Fn`.
  macro_rules! def_id_intrinsic {
    ($name:ident -> $ty:ty) => (
      pub(crate) fn $name() -> $ty {
        extern "platform-intrinsic" {
          fn $name() -> $ty;
        }
        unsafe { $name() }
      }
    )
  }

  def_id_intrinsic!(amdgcn_workitem_id_x -> u32);
  def_id_intrinsic!(amdgcn_workitem_id_y -> u32);
  def_id_intrinsic!(amdgcn_workitem_id_z -> u32);
  def_id_intrinsic!(amdgcn_workgroup_id_x -> u32);
  def_id_intrinsic!(amdgcn_workgroup_id_y -> u32);
  def_id_intrinsic!(amdgcn_workgroup_id_z -> u32);

  def_id_intrinsic!(amdgcn_dispatch_ptr -> *const u8);
}
