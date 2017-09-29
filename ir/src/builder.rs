
use std::collections::HashMap;
use std::collections::hash_map::{Entry};
use std::ops::{Deref, DerefMut};

use utils::HashableFloat;

use indexvec::IndexVec;

use syntax_pos::symbol::Symbol;

use rustc::hir::def_id::{CrateNum, DefIndex, DefId};
use rustc::middle::const_val;
use rustc::mir::{self, SourceInfo};
use rustc::ty::{self, TyS, subst, TyCtxt};
use rustc_data_structures::indexed_vec::{Idx as RustcIdx};
use rustc_const_math::{ConstInt, ConstFloat, ConstUsize, ConstIsize};

pub use syntax_pos::{Span, BytePos, SyntaxContext};

pub use tys::{Mutability, Ty, TyData, PrimTy, Signedness, TypeAndMut};
use rustc_wrappers::{SymbolDef, SpanDef, SourceInfoDef, CrateNumDef,
                     DefIndexDef, DefIdDef, ConstFloatDef,
                     ConstIntDef, ConstIsizeDef, ConstUsizeDef};

use super::{Field, Local, Substs, ConstVal};

pub struct Function<'tcx> {
  pub fun: super::Function,

  types: HashMap<&'tcx TyS<'tcx>, Ty>,
  substs: HashMap<&'tcx subst::Substs<'tcx>, Substs>,
  const_vals: HashMap<const_val::ConstVal<'tcx>, ConstVal>,
  adt_ids: HashMap<DefIdDef, Ty>
}

impl<'tcx> Function<'tcx> {
  pub fn new<'a, 'gcx>(src: SpanDef,
                       _tcx: TyCtxt<'a, 'gcx, 'tcx>)
    -> Function<'tcx>
    where 'gcx: 'a + 'tcx,
          'tcx: 'a,
  {
    Function {
      fun: super::Function::new(src),

      types: Default::default(),
      substs: Default::default(),
      const_vals: Default::default(),
      adt_ids: Default::default(),
    }
  }

  pub fn convert<T>(&mut self, t: &T) -> T::Target
    where T: RustCConvert<'tcx>,
  {
    t.convert(self)
  }

  pub fn convert_ty(&mut self, ty: &'tcx TyS<'tcx>) -> Ty {
    use rustc::ty::*;
    use syntax::ast::IntTy;
    use syntax::ast::UintTy;
    use syntax::ast::FloatTy;

    if let Some(id) = self.types.get(ty) {
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
      _ => {
        println!("type: {:?}", ty.sty);
        unimplemented!();
      },
    };

    let idx = self.fun.tys
      .push(data.clone());
    self.types.insert(ty, idx);
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
      &mir::Literal::Item {
        def_id,
        substs,
      } => Literal::Item {
        def_id: def_id.into(),
        substs: substs.convert(self),
      },
      &mir::Literal::Value {
        ref value,
      } => Literal::Value {
        value: self.convert(value),
      },
      &mir::Literal::Promoted {
        index,
      } => Literal::Promoted(index.into()),
    }
  }
  pub fn convert_constval(&mut self,
                          val: &const_val::ConstVal<'tcx>) -> super::ConstVal {
    use super::ConstValData;

    if let Some(id) = self.const_vals.get(val) {
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
        ConstValData::ByteStr(b.to_vec()),
      &const_val::ConstVal::Bool(b) =>
        ConstValData::Bool(b),
      &const_val::ConstVal::Char(c) =>
        ConstValData::Char(c),
      &const_val::ConstVal::Variant(v) =>
        ConstValData::Variant(v.into()),
      &const_val::ConstVal::Function(d, substs) => {
        ConstValData::Function(d.into(),
                               substs.convert(self))
      },
      &const_val::ConstVal::Struct(ref map) => {
        let v: Vec<_> = map.iter()
          .map(|(name, val)| {
            (name.to_string(),
             val.convert(self))
          })
          .collect();

        ConstValData::Struct(v)
      },
      &const_val::ConstVal::Tuple(ref t) => {
        let v = t.iter()
          .map(|v| v.convert(self) )
          .collect();
        ConstValData::Tuple(v)
      },
      &const_val::ConstVal::Array(ref a) => {
        let v = a.iter()
          .map(|v| v.convert(self) )
          .collect();
        ConstValData::Array(v)
      },
      &const_val::ConstVal::Repeat(ref r, count) => {
        ConstValData::Repeat(r.convert(self), count)
      },
    };

    let id = self.fun.const_vals
      .push(data);
    self.const_vals.insert(val.clone(), id);
    id
  }
  pub fn convert_subst(&mut self, subs: &'tcx subst::Substs<'tcx>) -> super::Substs {
    use rustc::ty::subst::Kind;
    use rustc::ty::RegionKind;

    if let Some(&id) = self.substs.get(subs) {
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

    let id = self.fun.substs
      .push(kinds);
    self.substs.insert(subs, id);
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
  pub fn convert_projection<B, V, T>(&mut self,
                                     proj: &mir::Projection<'tcx, B, V, T>)
    -> super::Projection<B::Target, V::Target, T::Target>
  where B: RustCConvert<'tcx>,
        V: RustCConvert<'tcx>,
        T: RustCConvert<'tcx>,
  {
    super::Projection {
      base: proj.base.convert(self),
      elem: proj.elem.convert(self),
    }
  }
  pub fn convert_projection_elem<V, T>(&mut self,
                                       elem: &mir::ProjectionElem<'tcx, V, T>)
    -> super::ProjectionElem<V::Target, T::Target>
    where V: RustCConvert<'tcx>,
          T: RustCConvert<'tcx>,
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

  pub fn finish(self) -> super::Function {
    self.into()
  }
}

pub trait RustCConvert<'tcx> {
  type Target;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target;
}
impl<'tcx> RustCConvert<'tcx> for mir::Lvalue<'tcx> {
  type Target = super::Lvalue;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target {
    builder.convert_lvalue(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Operand<'tcx> {
  type Target = super::Operand;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target {
    builder.convert_operand(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for &'tcx TyS<'tcx> {
  type Target = Ty;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target {
    builder.convert_ty(self)
  }
}

impl<'tcx, B, V, T> RustCConvert<'tcx> for mir::Projection<'tcx, B, V, T>
  where B: RustCConvert<'tcx>,
        V: RustCConvert<'tcx>,
        T: RustCConvert<'tcx>,
{
  type Target = super::Projection<B::Target, V::Target, T::Target>;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target {
    builder.convert_projection(self)
  }
}
impl<'tcx, V, T> RustCConvert<'tcx> for mir::ProjectionElem<'tcx, V, T>
  where V: RustCConvert<'tcx>,
        T: RustCConvert<'tcx>,
{
  type Target = super::ProjectionElem<V::Target, T::Target>;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target {
    builder.convert_projection_elem(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::AggregateKind<'tcx> {
  type Target = super::AggregateKind;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target {
    builder.convert_aggregate_kind(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Rvalue<'tcx> {
  type Target = super::Rvalue;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target {
    builder.convert_rvalue(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for const_val::ConstVal<'tcx> {
  type Target = super::ConstVal;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target {
    builder.convert_constval(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Constant<'tcx> {
  type Target = super::Constant;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target {
    builder.convert_constant(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Literal<'tcx> {
  type Target = super::Literal;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target {
    builder.convert_literal(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Static<'tcx> {
  type Target = super::Static;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target {
    builder.convert_static(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for &'tcx subst::Substs<'tcx> {
  type Target = super::Substs;
  fn convert(&self, builder: &mut Function<'tcx>) -> Self::Target {
    builder.convert_subst(*self)
  }
}

impl<'tcx> Into<super::Function> for Function<'tcx> {
  fn into(self) -> super::Function {
    let Function {
      fun,
      ..
    } = self;

    fun
  }
}
impl<'tcx> Deref for Function<'tcx> {
  type Target = super::Function;
  fn deref(&self) -> &Self::Target {
    &self.fun
  }
}
impl<'tcx> DerefMut for Function<'tcx> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.fun
  }
}
