
use std::ops::{Deref};

use compiler_builtins::int::LargeInt;

use syntax::ast::FloatTy;
use syntax_pos::{Span, BytePos, NO_EXPANSION,
                 SyntaxContext};
use syntax_pos::symbol::Symbol;
use rustc::hir::def_id::{CrateNum, DefIndex, DefId};
use rustc::mir::{SourceInfo};
use rustc_const_math::{ConstFloat, ConstInt, ConstIsize, ConstUsize};

use serde::{Serialize, Serializer, Deserialize, Deserializer};

use super::VisibilityScope;
use super::tys::PrimTy;

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash)]
pub struct BytePosDef(pub u32);
impl From<BytePosDef> for BytePos {
  fn from(v: BytePosDef) -> BytePos {
    BytePos(v.0)
  }
}
impl From<BytePos> for BytePosDef {
  fn from(v: BytePos) -> BytePosDef {
    BytePosDef(v.0)
  }
}

fn ctxt_default() -> SyntaxContext {
  NO_EXPANSION
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash)]
pub struct SpanDef {
  pub lo: BytePosDef,
  pub hi: BytePosDef,
  #[serde(skip, default = "ctxt_default")]
  pub ctxt: SyntaxContext,
}
impl From<SpanDef> for Span {
  fn from(def: SpanDef) -> Span {
    Span::new(def.lo.into(),
              def.hi.into(),
              def.ctxt)
  }
}
impl From<Span> for SpanDef {
  fn from(def_: Span) -> SpanDef {
    let def = def_.data();
    SpanDef {
      lo: def.lo.into(),
      hi: def.hi.into(),
      ctxt: def.ctxt.into(),
    }
  }
}
impl Default for SpanDef {
  fn default() -> SpanDef {
    From::from(Span::default())
  }
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash)]
pub struct SourceInfoDef {
  pub span: SpanDef,
  pub scope: VisibilityScope,
}
impl From<SourceInfo> for SourceInfoDef {
  fn from(v: SourceInfo) -> SourceInfoDef {
    SourceInfoDef {
      span: v.span.into(),
      scope: v.scope.into(),
    }
  }
}
impl Into<SourceInfo> for SourceInfoDef {
  fn into(self) -> SourceInfo {
    SourceInfo {
      span: self.span.into(),
      scope: self.scope.into(),
    }
  }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Hash, Debug)]
pub struct SymbolDef(String);
impl From<Symbol> for SymbolDef {
  fn from(v: Symbol) -> SymbolDef {
    SymbolDef(v.as_str().to_string())
  }
}
impl Into<Symbol> for SymbolDef {
  fn into(self) -> Symbol {
    Symbol::intern(&self.0[..])
  }
}
impl Deref for SymbolDef {
  type Target = str;
  fn deref(&self) -> &Self::Target {
    &self.0[..]
  }
}

#[derive(Clone, Copy, Eq, Ord, PartialOrd, PartialEq,
         Hash, Debug, Serialize, Deserialize)]
pub struct CrateNumDef(u32);
impl From<CrateNum> for CrateNumDef {
  fn from(v: CrateNum) -> CrateNumDef {
    CrateNumDef(v.as_u32())
  }
}
impl Into<CrateNum> for CrateNumDef {
  fn into(self) -> CrateNum {
    CrateNum::from_u32(self.0)
  }
}
#[derive(Clone, Copy, Eq, Ord, PartialOrd, PartialEq,
         Hash, Debug, Serialize, Deserialize)]
pub struct DefIndexDef(u32);
impl From<DefIndex> for DefIndexDef {
  fn from(v: DefIndex) -> DefIndexDef {
    DefIndexDef(v.as_u32())
  }
}
impl Into<DefIndex> for DefIndexDef {
  fn into(self) -> DefIndex {
    DefIndex::from_u32(self.0)
  }
}

#[derive(Clone, Eq, Ord, PartialOrd, PartialEq, Hash, Copy, Debug)]
#[derive(Serialize, Deserialize)]
pub struct DefIdDef {
  pub krate: CrateNumDef,
  pub index: DefIndexDef,
}
impl From<DefId> for DefIdDef {
  fn from(v: DefId) -> DefIdDef {
    DefIdDef {
      krate: v.krate.into(),
      index: v.index.into(),
    }
  }
}
impl Into<DefId> for DefIdDef {
  fn into(self) -> DefId {
    DefId {
      krate: self.krate.into(),
      index: self.index.into(),
    }
  }
}

fn serialize_float_ty<S>(this: &FloatTy, out: S)
  -> Result<S::Ok, S::Error>
  where S: Serializer,
{
  let pty = match this {
    &FloatTy::F32 => PrimTy::F32,
    &FloatTy::F64 => PrimTy::F64,
  };
  pty.serialize(out)
}
fn deserialize_float_ty<'de, D>(de: D)
  -> Result<FloatTy, D::Error>
  where D: Deserializer<'de>,
{
  use serde::de::Error;
  let pty = PrimTy::deserialize(de)?;
  Ok(match pty {
    PrimTy::F32 => FloatTy::F32,
    PrimTy::F64 => FloatTy::F64,
    _ => {
      return Err(Error::custom(format!("expected float type, found {:?}",
                                       pty)));
    }
  })
}

fn serialize_large_int<S, T>(v: &T, s: S)
  -> Result<S::Ok, S::Error>
  where T: LargeInt,
        T::LowHalf: Serialize,
        T::HighHalf: Serialize,
        S: Serializer,
{
  let v = (v.low(), v.high());
  v.serialize(s)
}
fn deserialize_large_int<'de, D, T>(d: D)
  -> Result<T, D::Error>
  where T: LargeInt,
        T::LowHalf: Deserialize<'de>,
        T::HighHalf: Deserialize<'de>,
        D: Deserializer<'de>,
{
  let v: (T::LowHalf, T::HighHalf) =
    Deserialize::deserialize(d)?;
  Ok(T::from_parts(v.0, v.1))
}

#[derive(Clone, Eq, PartialEq, Hash, Copy, Debug)]
#[derive(Serialize, Deserialize)]
pub struct ConstFloatDef {
  #[serde(serialize_with = "serialize_float_ty")]
  #[serde(deserialize_with = "deserialize_float_ty")]
  pub ty: FloatTy,
  #[serde(serialize_with = "serialize_large_int")]
  #[serde(deserialize_with = "deserialize_large_int")]
  pub bits: u128,
}
impl From<ConstFloatDef> for ConstFloat {
  fn from(v: ConstFloatDef) -> ConstFloat {
    ConstFloat {
      ty: v.ty,
      bits: v.bits,
    }
  }
}
impl From<ConstFloat> for ConstFloatDef {
  fn from(v: ConstFloat) -> ConstFloatDef {
    ConstFloatDef {
      ty: v.ty,
      bits: v.bits,
    }
  }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize,
         Hash, Eq, PartialEq)]
pub enum ConstIntDef {
  I8(i8),
  I16(i16),
  I32(i32),
  I64(i64),
  I128(
    #[serde(serialize_with = "serialize_large_int")]
    #[serde(deserialize_with = "deserialize_large_int")] i128),
  Isize(ConstIsizeDef),
  U8(u8),
  U16(u16),
  U32(u32),
  U64(u64),
  U128(
    #[serde(serialize_with = "serialize_large_int")]
    #[serde(deserialize_with = "deserialize_large_int")] u128),
  Usize(ConstUsizeDef),
}
impl From<ConstInt> for ConstIntDef {
  fn from(v: ConstInt) -> ConstIntDef {
    match v {
      ConstInt::I8(v) => ConstIntDef::I8(v),
      ConstInt::I16(v) => ConstIntDef::I16(v),
      ConstInt::I32(v) => ConstIntDef::I32(v),
      ConstInt::I64(v) => ConstIntDef::I64(v),
      ConstInt::I128(v) => ConstIntDef::I128(v),
      ConstInt::Isize(v) => ConstIntDef::Isize(From::from(v)),
      ConstInt::U8(v) => ConstIntDef::U8(v),
      ConstInt::U16(v) => ConstIntDef::U16(v),
      ConstInt::U32(v) => ConstIntDef::U32(v),
      ConstInt::U64(v) => ConstIntDef::U64(v),
      ConstInt::U128(v) => ConstIntDef::U128(v),
      ConstInt::Usize(v) => ConstIntDef::Usize(From::from(v)),
    }
  }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize,
         Hash, Eq, PartialEq)]
pub enum ConstIsizeDef {
  Is16(i16),
  Is32(i32),
  Is64(i64),
}
impl From<ConstIsize> for ConstIsizeDef {
  fn from(v: ConstIsize) -> ConstIsizeDef {
    match v {
      ConstIsize::Is16(v) => ConstIsizeDef::Is16(v),
      ConstIsize::Is32(v) => ConstIsizeDef::Is32(v),
      ConstIsize::Is64(v) => ConstIsizeDef::Is64(v),
    }
  }
}
#[derive(Copy, Clone, Debug, Serialize, Deserialize,
         Hash, Eq, PartialEq)]
pub enum ConstUsizeDef {
  Us16(u16),
  Us32(u32),
  Us64(u64),
}
impl From<ConstUsize> for ConstUsizeDef {
  fn from(v: ConstUsize) -> ConstUsizeDef {
    match v {
      ConstUsize::Us16(v) => ConstUsizeDef::Us16(v),
      ConstUsize::Us32(v) => ConstUsizeDef::Us32(v),
      ConstUsize::Us64(v) => ConstUsizeDef::Us64(v),
    }
  }
}
