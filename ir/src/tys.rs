
use std::cell::Cell;
use std::rc::Rc;

use rustc;
use rustc::ty::{RegionKind};
pub use rustc::ty::TypeFlags;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signedness {
  Signed,
  Unsigned,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimTy {
  Bool,
  Char,

  F8,
  F16,
  F24,
  F32,
  F48,
  F64,

  Is(Signedness),
  I8(Signedness),
  I16(Signedness),
  I32(Signedness),
  I64(Signedness),
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum Region {
  /// I don't like this. I'd like to empower use of region
  /// analysis in polyhedral models.
  Erased,
}
impl From<RegionKind> for Region {
  fn from(v: RegionKind) -> Region {
    match v {
      RegionKind::ReErased => Region::Erased,
      _ => {
        unimplemented!()
      },
    }
  }
}

newtype_idx!(#[derive(Deserialize, Serialize)] pub struct Ty => "type");
#[derive(Debug, Clone, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub enum TyData {
  Primitive(PrimTy),
  Str,
  Array(Ty, usize),
  Slice(Ty),
  Tuple(Vec<Ty>, bool),
  RawPtr(TypeAndMut),
  Ref(Region, TypeAndMut),
}

#[derive(Debug, Clone, Copy, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub struct TypeAndMut {
  pub ty: Ty,
  pub mutbl: Mutability,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub enum Mutability {
  Mutable,
  Immutable,
}

impl From<rustc::hir::Mutability> for Mutability {
  fn from(v: rustc::hir::Mutability) -> Mutability {
    match v {
      rustc::hir::Mutability::MutMutable => Mutability::Mutable,
      rustc::hir::Mutability::MutImmutable => Mutability::Immutable,
    }
  }
}
impl Into<rustc::hir::Mutability> for Mutability {
  fn into(self) -> rustc::hir::Mutability {
    match self {
      Mutability::Mutable => rustc::hir::Mutability::MutMutable,
      Mutability::Immutable => rustc::hir::Mutability::MutImmutable,
    }
  }
}
impl From<rustc::mir::Mutability> for Mutability {
  fn from(v: rustc::mir::Mutability) -> Mutability {
    match v {
      rustc::mir::Mutability::Mut => Mutability::Mutable,
      rustc::mir::Mutability::Not => Mutability::Immutable,
    }
  }
}
impl Into<rustc::mir::Mutability> for Mutability {
  fn into(self) -> rustc::mir::Mutability {
    match self {
      Mutability::Mutable => rustc::mir::Mutability::Mut,
      Mutability::Immutable => rustc::mir::Mutability::Not,
    }
  }
}