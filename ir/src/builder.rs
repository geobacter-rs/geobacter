
use std::collections::HashMap;
use std::collections::hash_map::{Entry};
use std::marker::{Sized, PhantomData};
use std::ops::{Deref, DerefMut};

use utils::HashableFloat;

use indexvec::IndexVec;
use indexvec::Idx;

use syntax_pos::symbol::Symbol;

use rustc::hir::def_id::{CrateNum, DefIndex, DefId};
use rustc::infer::TransNormalize;
use rustc::middle::const_val::{self, ConstAggregate};
use rustc::mir::{self, SourceInfo};
use rustc::ty::{self, TyS, subst, TyCtxt, Instance, InstanceDef};
use rustc_data_structures::indexed_vec::{Idx as RustcIdx};
use rustc_const_math::{ConstInt, ConstFloat, ConstUsize, ConstIsize};
use rustc_trans::monomorphize;

pub use syntax_pos::{Span, BytePos, SyntaxContext};

pub use tys::{Mutability, Ty, TyData, PrimTy, Signedness, TypeAndMut};
use rustc_wrappers::{SymbolDef, SpanDef, SourceInfoDef, CrateNumDef,
                     DefIndexDef, DefIdDef, ConstFloatDef,
                     ConstIntDef, ConstIsizeDef, ConstUsizeDef};

use super::{Field, Local, Substs, ConstVal};

pub struct Module<'a, 'tcx>
  where 'tcx: 'a,
{
  pub mod_: super::Module,
  extern_substs: &'tcx subst::Substs<'tcx>,
  tcx: TyCtxt<'a, 'tcx, 'tcx>,

  types: HashMap<&'tcx TyS<'tcx>, Ty>,
  substs: HashMap<&'tcx subst::Substs<'tcx>, Substs>,
  const_vals: HashMap<const_val::ConstVal<'tcx>, ConstVal>,
  adt_ids: HashMap<DefIdDef, Ty>,
  funcs: HashMap<ty::Instance<'tcx>, super::Function>,
}

impl<'a, 'tcx> Module<'a, 'tcx> {
  pub fn new(tcx: TyCtxt<'a, 'tcx, 'tcx>,
             substs: &'tcx subst::Substs<'tcx>)
    -> Module<'a, 'tcx>
  {
    let mut m = super::Module::new();

    Module {
      mod_: m,
      extern_substs: substs,
      tcx: tcx,

      types: Default::default(),
      substs: Default::default(),
      const_vals: Default::default(),
      adt_ids: Default::default(),
      funcs: Default::default(),
    }
  }
  pub fn build(&mut self, item: DefId) {
    let instance = InstanceDef::Item(item);
    let instance = Instance {
      def: instance,
      substs: self.extern_substs,
    };

    self.enter_function(instance, |mut main| {
      main.build();
    });
  }

  pub fn finish(self) -> super::Module {
    let Module {
      mod_,
      ..
    } = self;
    mod_
  }
}

pub trait ModuleCtxt<'a, 'tcx>
  where 'tcx: 'a,
{
  fn tcx(&self) -> TyCtxt<'a, 'tcx, 'tcx>;
  fn module(&mut self) -> &mut Module<'a, 'tcx>;
  fn module_ref(&self) -> &Module<'a, 'tcx>;

  fn param_substs(&self) -> &'tcx subst::Substs<'tcx>;
}
impl<'a, 'tcx> ModuleCtxt<'a, 'tcx> for Module<'a, 'tcx> {
  fn tcx(&self) -> TyCtxt<'a, 'tcx, 'tcx> { self.tcx }
  fn module(&mut self) -> &mut Module<'a, 'tcx> { self }
  fn module_ref(&self) -> &Module<'a, 'tcx> { self }
  fn param_substs(&self) -> &'tcx subst::Substs<'tcx> { self.extern_substs }
}
impl<'parent, 'a, 'tcx> ModuleCtxt<'a, 'tcx> for Function<'parent, 'a, 'tcx>
{
  fn tcx(&self) -> TyCtxt<'a, 'tcx, 'tcx> { self.parent.tcx() }
  fn module(&mut self) -> &mut Module<'a, 'tcx> { self.parent.module() }
  fn module_ref(&self) -> &Module<'a, 'tcx> { self.parent.module_ref() }

  fn param_substs(&self) -> &'tcx subst::Substs<'tcx> { self.def.substs }
}
pub trait ExtraModuleCtxt<'a, 'tcx> {
  fn monomorphize<T>(&self, v: &T) -> T
    where T: TransNormalize<'tcx>;
  fn enter_function<'s, F>(&'s mut self,
                           instance: ty::Instance<'tcx>,
                           f: F) -> super::Function
    where F: FnOnce(Function<'s, 'a, 'tcx>),
          'tcx: 's + 'a,
          'a: 's;
}
impl<'a, 'tcx, TT> ExtraModuleCtxt<'a, 'tcx> for TT
  where TT: ModuleCtxt<'a, 'tcx>,
        'tcx: 'a,
{
  fn monomorphize<T>(&self, v: &T) -> T
    where T: TransNormalize<'tcx>,
  {
    self.tcx()
      .trans_apply_param_substs(self.param_substs(), v)
  }

  fn enter_function<'s, F>(&'s mut self,
                           instance: ty::Instance<'tcx>,
                           f: F) -> super::Function
    where F: FnOnce(Function<'s, 'a, 'tcx>),
          'tcx: 's + 'a,
          'a: 's,
  {
    use std::collections::hash_map::Entry;
    let id = {
      let m = self.module();

      match m.funcs.entry(instance) {
        Entry::Vacant(mut v) => {
          let f = super::FunctionData::new();
          let id = m.mod_.funcs
            .push(f);
          v.insert(id);
          id
        },
        Entry::Occupied(o) => {
          return *o.get();
        },
      }
    };

    let fun = Function {
      parent: self,
      func: id,
      def: instance,
    };
    f(fun);

    id
  }
}

pub struct Function<'parent, 'a, 'tcx>
  where 'tcx: 'parent + 'a,
        'a: 'parent,
{
  parent: &'parent mut ModuleCtxt<'a, 'tcx>,
  func: super::Function,
  def: ty::Instance<'tcx>,
}
impl<'parent, 'a, 'tcx> Deref for Function<'parent, 'a, 'tcx> {
  type Target = super::FunctionKind;
  fn deref(&self) -> &Self::Target {
    let m = self.module_ref();
    m.mod_.funcs[self.func].fkr()
  }
}
impl<'parent, 'a, 'tcx> DerefMut for Function<'parent, 'a, 'tcx> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    let id = self.func;
    let m = self.module();
    m.mod_.funcs[id].fkm()
  }
}

impl<'parent, 'a, 'tcx> Function<'parent, 'a, 'tcx> {
  pub fn id(&self) -> super::Function { self.func }

  fn build(&mut self) {
    use super::{StatementKind, TerminatorKind,
                Statement, Terminator, BasicBlockData,
                VisibilityScopeData, LocalDecl,
                UpvarDecl, BasicBlock, };

    let mir = match self.def.def {
      InstanceDef::Item(def_id) => {
        self.tcx().optimized_mir(def_id)
      },
      _ => bug!("can't build hsa ir for {:?}", self.def),
    };

    let tcx = self.tcx();

    self.tcx().sess
      .span_note_without_error(mir.span,
                               "building this");

    let return_ty = self.monomorphize(&mir.return_ty);
    let return_ty = self.convert(&return_ty);
    self.return_ty = Some(return_ty);

    for (bbid, bb) in mir.basic_blocks().iter_enumerated() {
      let mut kstmts = Vec::new();

      for stmt in bb.statements.iter() {
        let kkind = match stmt.kind {
          mir::StatementKind::Assign(ref l, ref r) => {
            StatementKind::Assign(self.convert(l),
                                  self.convert(r))
          },
          mir::StatementKind::SetDiscriminant {
            ref lvalue, variant_index,
          } => StatementKind::SetDiscriminant {
            lvalue: self.convert(lvalue),
            variant_index: variant_index,
          },
          mir::StatementKind::StorageLive(ref l) => {
            StatementKind::StorageLive(self.convert(l))
          },
          mir::StatementKind::StorageDead(ref l) => {
            StatementKind::StorageDead(self.convert(l))
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
          discr: self.convert(discr),
          switch_ty: self.convert(switch_ty),
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
          location: self.convert(location),
          target: target.into(),
          unwind: unwind.map(|v| v.into() ),
        },
        &mir::TerminatorKind::DropAndReplace {
          ref location,
          ref value,
          target,
          ref unwind,
        } => TerminatorKind::DropAndReplace {
          location: self.convert(location),
          value: self.convert(value),
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
                                  "no function pointer calls allowed");
            },
            _ => bug!("{} is not callable", fty),
          };

          self.enter_function(finstance, |mut nested_fn| {
            match finstance.def {
              InstanceDef::Item(_) => {
                nested_fn.build()
              },
              InstanceDef::Intrinsic(idef_id) => {
                let name = (&nested_fn.tcx().item_name(idef_id)[..])
                  .to_string();

                let sig = func.ty(mir, nested_fn.tcx())
                  .fn_sig(nested_fn.tcx());
                let sig = nested_fn.tcx()
                  .erase_late_bound_regions_and_normalize(&sig);
                let sig = sig.inputs_and_output
                  .iter()
                  .map(|ty| nested_fn.convert(ty))
                  .collect();
                let data =
                  super::FunctionData::Intrinsic(name, sig);
                let id = nested_fn.func;
                nested_fn.module()
                  .mod_
                  .funcs[id] = data;
              },
              _ => {
                bug!("TODO: function instance: {:?}", finstance);
              },
            }
          });

          TerminatorKind::Call {
            func: self.convert(func),
            args: args.iter()
              .map(|v| self.convert(v))
              .collect(),
            destination: destination.as_ref()
              .map(|&(ref dest, destbb)| {
                (self.convert(dest), destbb.into())
              }),
            cleanup: cleanup.as_ref()
              .map(|&bb| bb.into()),
          }
        },
        &mir::TerminatorKind::Assert { target, .. } => {
          span_bug!(terminator.source_info.span,
                    "terminator assert unimplemented");
          TerminatorKind::Goto { target: target.into(), }
        },
        &mir::TerminatorKind::Yield {
          ref value, ref resume, ref drop,
        } => {
          TerminatorKind::Yield {
            value: self.convert(value),
            resume: BasicBlock::new(resume.index()),
            drop: drop.map(|v| BasicBlock::new(v.index()) ),
          }
        },
        &mir::TerminatorKind::GeneratorDrop => TerminatorKind::GeneratorDrop,
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

      let new_idx = self.basic_blocks.push(kbb);
      assert_eq!(new_idx.index(), bbid.index());
    }

    for (id, vis) in mir.visibility_scopes.iter_enumerated() {
      let kvis = VisibilityScopeData {
        span: vis.span.into(),
        parent_scope: vis.parent_scope
          .map(|v| v.into() ),
      };

      let kid = self.visibility_scopes.push(kvis);
      assert_eq!(kid.index(), id.index());
    }

    for (id, promoted) in mir.promoted.iter_enumerated() {
      unimplemented!();
      /*let f = self.build_hsa_ir(tcx, &promoted,
                                subst::Substs::empty());
      if let Some(f) = f {
        let kid = self.promoted.push(f);
        assert_eq!(kid.index(), id.index());
      } else {
        return None;
      }*/
    }

    for (id, ldecl) in mir.local_decls.iter_enumerated() {
      let ldecl_ty = ldecl.ty;
      let ldecl_ty = self.monomorphize(&ldecl_ty);
      let kldecl = LocalDecl {
        mutability: ldecl.mutability.into(),
        is_user_variable: ldecl.is_user_variable,
        ty: self.convert(&ldecl_ty),
        name: ldecl.name.map(|v| v.into()),
        source_info: ldecl.source_info.into(),
      };

      let kid = self.local_decls.push(kldecl);
      assert_eq!(kid.index(), id.index());
    }

    for upvar in mir.upvar_decls.iter() {
      let kupvar = UpvarDecl {
        debug_name: upvar.debug_name.into(),
        by_ref: upvar.by_ref,
      };

      self.upvar_decls.push(kupvar);
    }

    self.spread_arg = mir.spread_arg.map(|v| v.into() );
    self.span = mir.span.into();
  }

  pub fn convert<TI>(&mut self, t: &TI) -> TI::Target
    where TI: RustCConvert<'tcx>,
  {
    t.convert(self)
  }

  pub fn convert_ty(&mut self, ty: &'tcx TyS<'tcx>) -> Ty {
    use rustc::ty::*;
    use syntax::ast::IntTy;
    use syntax::ast::UintTy;
    use syntax::ast::FloatTy;

    if let Some(id) = self.module_ref().types.get(ty) {
      return *id;
    }

    let data = match ty.sty {
      TyBool => {
        TyData::Primitive(PrimTy::Bool)
      },
      TyChar => {
        TyData::Primitive(PrimTy::Char)
      },
      TyInt(IntTy::Is) => {
        TyData::Primitive(PrimTy::Is(Signedness::Signed))
      },
      TyInt(IntTy::I8) => {
        TyData::Primitive(PrimTy::I8(Signedness::Signed))
      },
      TyInt(IntTy::I16) => {
        TyData::Primitive(PrimTy::I16(Signedness::Signed))
      },
      TyInt(IntTy::I32) => {
        TyData::Primitive(PrimTy::I32(Signedness::Signed))
      },
      TyInt(IntTy::I64) => {
        TyData::Primitive(PrimTy::I64(Signedness::Signed))
      },

      TyUint(UintTy::Us) => {
        TyData::Primitive(PrimTy::Is(Signedness::Unsigned))
      },
      TyUint(UintTy::U8) => {
        TyData::Primitive(PrimTy::I8(Signedness::Unsigned))
      },
      TyUint(UintTy::U16) => {
        TyData::Primitive(PrimTy::I16(Signedness::Unsigned))
      },
      TyUint(UintTy::U32) => {
        TyData::Primitive(PrimTy::I32(Signedness::Unsigned))
      },
      TyUint(UintTy::U64) => {
        TyData::Primitive(PrimTy::I64(Signedness::Unsigned))
      },

      TyFloat(FloatTy::F32) => {
        TyData::Primitive(PrimTy::F32)
      },
      TyFloat(FloatTy::F64) => {
        TyData::Primitive(PrimTy::F64)
      },

      TyStr => TyData::Str,
      TyArray(inner, count) => {
        let ity = self.convert_ty(inner);
        let count = self.convert(&count.val);
        TyData::Array(ity, count)
      },
      TySlice(inner) => {
        let ity = self.convert_ty(inner);
        TyData::Slice(ity)
      },
      TyRawPtr(inner) => {
        let ity = self.convert_ty(inner.ty);
        let inner = super::tys::TypeAndMut {
          ty: ity,
          mutbl: inner.mutbl.into(),
        };
        TyData::RawPtr(inner)
      },
      TyRef(&RegionKind::ReErased, inner) => {
        let region = super::tys::Region::Erased;
        let ity = self.convert_ty(inner.ty);
        let inner = super::tys::TypeAndMut {
          ty: ity,
          mutbl: inner.mutbl.into(),
        };
        TyData::Ref(region, inner)
      },
      TyTuple(ref inner, b) => {
        let inners: Vec<_> = inner.iter()
          .map(|v| self.convert(v) )
          .collect();
        TyData::Tuple(inners, b)
      },
      TyFnDef(def_id, subs) => {
        TyData::FnDef(def_id.into(), self.convert(&subs))
      },
      _ => {
        println!("type: {:?}", ty.sty);
        unimplemented!();
      },
    };

    let idx = self.module().mod_.tys
      .push(data.clone());
    self.module().types.insert(ty, idx);
    idx
  }

  pub fn convert_aggregate_kind(&mut self,
                                kind: &mir::AggregateKind<'tcx>) -> super::AggregateKind {
    use super::AggregateKind;
    match kind {
      &mir::AggregateKind::Array(ref ty) => {
        return AggregateKind::Array(self.convert(ty));
      },
      &mir::AggregateKind::Tuple => {
        return AggregateKind::Tuple;
      },
      &mir::AggregateKind::Adt(def, disc, substs, active_field) => { },
      &mir::AggregateKind::Closure(def, substs) => {
        //return AggregateKind::Closure(def.into(),
        //                              self.convert(substs));
      },
      &mir::AggregateKind::Generator(..) => unimplemented!(),
    }

    println!("aggregate kind: {:?}", kind);
    unimplemented!()
  }

  pub fn convert_rvalue(&mut self,
                        rvalue: &mir::Rvalue<'tcx>) -> super::Rvalue {
    use super::Rvalue;
    match rvalue {
      &mir::Rvalue::Use(ref o) => Rvalue::Use(self.convert(o)),
      &mir::Rvalue::Repeat(ref o, c) =>
        Rvalue::Repeat(self.convert(o), From::from(c)),
      &mir::Rvalue::Ref(&ty::RegionKind::ReErased,
                        borrow, ref v) =>
        Rvalue::Ref(super::RegionKind::Erased,
                    From::from(borrow),
                    self.convert(v)),
      &mir::Rvalue::Ref(..) => unreachable!(),
      &mir::Rvalue::Len(ref v) => Rvalue::Len(self.convert(v)),
      &mir::Rvalue::Cast(kind, ref v, ref ty) =>
        Rvalue::Cast(kind.into(), self.convert(v),
                     self.convert(ty)),
      &mir::Rvalue::BinaryOp(op, ref l, ref r) =>
        Rvalue::BinaryOp(op.into(), self.convert(l),
                         self.convert(r)),
      &mir::Rvalue::CheckedBinaryOp(op, ref l, ref r) =>
        Rvalue::CheckedBinaryOp(op.into(), self.convert(l),
                                self.convert(r)),
      &mir::Rvalue::NullaryOp(op, ref ty) =>
        Rvalue::NullaryOp(From::from(op), self.convert(ty)),
      &mir::Rvalue::UnaryOp(op, ref o) =>
        Rvalue::UnaryOp(From::from(op), self.convert(o)),
      &mir::Rvalue::Discriminant(ref d) =>
        Rvalue::Discriminant(self.convert(d)),
      &mir::Rvalue::Aggregate(ref kind, ref ops) => {
        let agg = self.convert(&**kind);
        let ops = ops.iter()
          .map(|v| self.convert(v) )
          .collect();
        Rvalue::Aggregate(Box::new(agg), ops)
      },
    }
  }

  pub fn convert_operand(&mut self,
                         operand: &mir::Operand<'tcx>) -> super::Operand {
    use super::Operand;
    match operand {
      &mir::Operand::Consume(ref l) => {
        Operand::Consume(l.convert(self))
      },
      &mir::Operand::Constant(ref c) => {
        Operand::Constant(Box::new(self.convert(&**c)))
      },
    }
  }

  pub fn convert_constant(&mut self,
                          constant: &mir::Constant<'tcx>) -> super::Constant {
    super::Constant {
      span: constant.span.into(),
      ty: constant.ty.convert(self),
      literal: constant.literal.convert(self),
    }
  }
  pub fn convert_literal(&mut self,
                         literal: &mir::Literal<'tcx>) -> super::Literal {
    use super::Literal;
    match literal {
      &mir::Literal::Value {
        ref value,
      } => Literal::Value {
        value: self.convert(&value.val),
      },
      &mir::Literal::Promoted {
        index,
      } => Literal::Promoted(index.into()),
    }
  }
  pub fn convert_constval(&mut self,
                          val: &const_val::ConstVal<'tcx>) -> super::ConstVal {
    use super::ConstValData;

    if let Some(id) = self.module_ref().const_vals.get(val) {
      return id.clone();
    }

    let data = match val {
      &const_val::ConstVal::Float(f) =>
        ConstValData::Float(f.into()),
      &const_val::ConstVal::Integral(i) =>
        ConstValData::Integral(i.into()),
      &const_val::ConstVal::Str(ref s) =>
        ConstValData::Str(s.to_string()),
      &const_val::ConstVal::ByteStr(ref b) =>
        ConstValData::ByteStr(b.data.to_vec()),
      &const_val::ConstVal::Bool(b) =>
        ConstValData::Bool(b),
      &const_val::ConstVal::Char(c) =>
        ConstValData::Char(c),
      &const_val::ConstVal::Variant(v) =>
        ConstValData::Variant(v.into()),
      &const_val::ConstVal::Function(d, substs) => {
        let resolved = monomorphize::resolve(self.tcx(),
                                             d, substs);
        let f = self.enter_function(resolved, |mut nested_fn| {
          match resolved.def {
            InstanceDef::Item(_) => {
              nested_fn.build();
            },
            InstanceDef::Intrinsic(idef_id) => {
              let name = (&nested_fn.tcx().item_name(idef_id)[..])
                .to_string();
              let ty = nested_fn.tcx().type_of(idef_id);
              let sig = ty.fn_sig(nested_fn.tcx());
              let sig = nested_fn.tcx()
                .erase_late_bound_regions_and_normalize(&sig);
              let sig = sig.inputs_and_output
                .iter()
                .map(|ty| nested_fn.convert(ty))
                .collect();
              let data =
                super::FunctionData::Intrinsic(name, sig);

              let id = nested_fn.func;
              nested_fn.module()
                .mod_
                .funcs[id] = data;
            },
            _ => {
              bug!("TODO: function instance: {:?}", resolved);
            },
          }
        });
        ConstValData::Function(f)
      },
      &const_val::ConstVal::Aggregate(ConstAggregate::Struct(map)) => {
        let v: Vec<_> = map.iter()
          .map(|&(ref name, val)| {
            (name.to_string(),
             val.val.convert(self))
          })
          .collect();

        ConstValData::Struct(v)
      },
      &const_val::ConstVal::Aggregate(ConstAggregate::Tuple(t)) => {
        let v = t.iter()
          .map(|v| v.val.convert(self) )
          .collect();
        ConstValData::Tuple(v)
      },
      &const_val::ConstVal::Aggregate(ConstAggregate::Array(a)) => {
        let v = a.iter()
          .map(|v| v.val.convert(self) )
          .collect();
        ConstValData::Array(v)
      },
      &const_val::ConstVal::Aggregate(ConstAggregate::Repeat(r, count)) => {
        ConstValData::Repeat(r.val.convert(self), count)
      },
      &const_val::ConstVal::Unevaluated(..) => unimplemented!(),
    };

    let id = self.module().mod_.const_vals
      .push(data);
    self.module().const_vals.insert(val.clone(),
                                    id);
    id
  }
  pub fn convert_subst(&mut self, subs: &'tcx subst::Substs<'tcx>) -> super::Substs {
    use rustc::ty::RegionKind;

    if let Some(&id) = self.module_ref().substs.get(subs) {
      return id;
    }

    let kinds: Vec<_> = subs.iter()
      .map(|kind| {
        if let Some(ty) = kind.as_type() {
          return super::TsKind::Type(self.convert_ty(ty));
        }

        if let Some(&RegionKind::ReErased) = kind.as_region() {
          return super::TsKind::Region(super::RegionKind::Erased)
        }

        unreachable!();
      })
      .collect();

    let id = self.module().mod_.substs
      .push(kinds);
    self.module().substs.insert(subs, id);
    id
  }

  pub fn convert_lvalue(&mut self, lvalue: &mir::Lvalue<'tcx>) ->  super::Lvalue {
    use super::Lvalue;
    match lvalue {
      &mir::Lvalue::Local(l) => Lvalue::Local(l.into()),
      &mir::Lvalue::Static(ref s) => Lvalue::Static(s.convert(self)),
      &mir::Lvalue::Projection(ref proj) => {
        Lvalue::Projection(Box::new(self.convert_projection(&**proj)))
      }
    }
  }
  pub fn convert_static(&mut self, statik: &mir::Static<'tcx>) -> super::Static {
    super::Static {
      def_id: statik.def_id.into(),
      ty: self.convert(&statik.ty),
    }
  }
  pub fn convert_projection<B, V, TT>(&mut self,
                                     proj: &mir::Projection<'tcx, B, V, TT>)
    -> super::Projection<B::Target, V::Target, TT::Target>
  where B: RustCConvert<'tcx>,
        V: RustCConvert<'tcx>,
        TT: RustCConvert<'tcx>,
  {
    super::Projection {
      base: proj.base.convert(self),
      elem: proj.elem.convert(self),
    }
  }
  pub fn convert_projection_elem<V, TT>(&mut self,
                                        elem: &mir::ProjectionElem<'tcx, V, TT>)
    -> super::ProjectionElem<V::Target, TT::Target>
    where V: RustCConvert<'tcx>,
          TT: RustCConvert<'tcx>,
  {
    use super::ProjectionElem;
    match elem {
      &mir::ProjectionElem::Deref => ProjectionElem::Deref,
      &mir::ProjectionElem::Field(field, ref ty) => {
        ProjectionElem::Field(field.into(),
                              ty.convert(self))
      },
      &mir::ProjectionElem::Index(ref v) => {
        ProjectionElem::Index(v.convert(self))
      },
      &mir::ProjectionElem::ConstantIndex {
        offset,
        min_length,
        from_end,
      } => ProjectionElem::ConstantIdx {
        offset,
        min_length,
        from_end,
      },
      &mir::ProjectionElem::Subslice {
        from, to,
      } => ProjectionElem::Subslice {
        from, to,
      },
      &mir::ProjectionElem::Downcast(adt, disc) => {
        println!("downcast: {:?}", adt);
        unimplemented!();
      },
    }
  }
}

pub trait RustCConvert<'tcx> {
  type Target;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent;
}
impl<'tcx> RustCConvert<'tcx> for mir::Lvalue<'tcx> {
  type Target = super::Lvalue;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    builder.convert_lvalue(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Operand<'tcx> {
  type Target = super::Operand;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    builder.convert_operand(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for &'tcx TyS<'tcx> {
  type Target = Ty;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    builder.convert_ty(self)
  }
}

impl<'tcx, B, V, T> RustCConvert<'tcx> for mir::Projection<'tcx, B, V, T>
  where B: RustCConvert<'tcx>,
        V: RustCConvert<'tcx>,
        T: RustCConvert<'tcx>,
{
  type Target = super::Projection<B::Target, V::Target, T::Target>;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    builder.convert_projection(self)
  }
}
impl<'tcx, V, T> RustCConvert<'tcx> for mir::ProjectionElem<'tcx, V, T>
  where V: RustCConvert<'tcx>,
        T: RustCConvert<'tcx>,
{
  type Target = super::ProjectionElem<V::Target, T::Target>;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    builder.convert_projection_elem(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::AggregateKind<'tcx> {
  type Target = super::AggregateKind;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    builder.convert_aggregate_kind(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Rvalue<'tcx> {
  type Target = super::Rvalue;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    builder.convert_rvalue(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for const_val::ConstVal<'tcx> {
  type Target = super::ConstVal;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    builder.convert_constval(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Constant<'tcx> {
  type Target = super::Constant;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    builder.convert_constant(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Literal<'tcx> {
  type Target = super::Literal;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    builder.convert_literal(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Static<'tcx> {
  type Target = super::Static;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    builder.convert_static(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for &'tcx subst::Substs<'tcx> {
  type Target = super::Substs;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    builder.convert_subst(*self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Local {
  type Target = super::Local;
  fn convert<'parent, 'a>(&self, builder: &mut Function<'parent, 'a, 'tcx>) -> Self::Target
    where 'tcx: 'parent,
  {
    use indexvec::Idx;
    super::Local::new(self.index())
  }
}

impl<'a, 'tcx> Deref for Module<'a, 'tcx> {
  type Target = super::Module;
  fn deref(&self) -> &Self::Target {
    &self.mod_
  }
}
impl<'a, 'tcx> DerefMut for Module<'a, 'tcx> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.mod_
  }
}
