
use rustc;
use rustc::ty::{RegionKind};
pub use rustc::ty::TypeFlags;

use super::{Substs};
use rustc_wrappers::{DefIdDef, CtorKind, };
use syntax;

pub mod traits;

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

  Int(IntTy),
}


#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntTy {
  Is(Signedness),
  I8(Signedness),
  I16(Signedness),
  I32(Signedness),
  I64(Signedness),
  I128(Signedness),
}
impl From<syntax::ast::UintTy> for IntTy {
  fn from(v: syntax::ast::UintTy) -> Self {
    use syntax::ast::UintTy::*;

    let u = Signedness::Unsigned;
    match v {
      Us => IntTy::Is(u),
      U8 => IntTy::I8(u),
      U16 => IntTy::I16(u),
      U32 => IntTy::I32(u),
      U64 => IntTy::I64(u),
      U128 => IntTy::I128(u),
    }
  }
}
impl From<syntax::ast::IntTy> for IntTy {
  fn from(v: syntax::ast::IntTy) -> Self {
    use syntax::ast::IntTy::*;

    let s = Signedness::Signed;
    match v {
      Is => IntTy::Is(s),
      I8 => IntTy::I8(s),
      I16 => IntTy::I16(s),
      I32 => IntTy::I32(s),
      I64 => IntTy::I64(s),
      I128 => IntTy::I128(s),
    }
  }
}
impl From<syntax::attr::IntType> for IntTy {
  fn from(v: syntax::attr::IntType) -> IntTy {
    use syntax::attr::IntType::*;
    use syntax::ast::IntTy::*;
    use syntax::ast::UintTy::*;

    let s = Signedness::Signed;
    let u = Signedness::Unsigned;
    match v {
      SignedInt(Is) => IntTy::Is(s),
      SignedInt(I8) => IntTy::I8(s),
      SignedInt(I16) => IntTy::I16(s),
      SignedInt(I32) => IntTy::I32(s),
      SignedInt(I64) => IntTy::I64(s),
      SignedInt(I128) => IntTy::I128(s),

      UnsignedInt(Us) => IntTy::Is(u),
      UnsignedInt(U8) => IntTy::I8(u),
      UnsignedInt(U16) => IntTy::I16(u),
      UnsignedInt(U32) => IntTy::I32(u),
      UnsignedInt(U64) => IntTy::I64(u),
      UnsignedInt(U128) => IntTy::I128(u),
    }
  }
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum VariantDiscr {
  /// Explicit value for this variant, i.e. `X = 123`.
  /// The `DefId` corresponds to the embedded constant.
  Explicit(DefIdDef),

  /// The previous variant's discriminant plus one.
  /// For efficiency reasons, the distance from the
  /// last `Explicit` discriminant is being stored,
  /// or `0` for the first variant, if it has none.
  Relative(usize),
}
impl From<rustc::ty::VariantDiscr> for VariantDiscr {
  fn from(v: rustc::ty::VariantDiscr) -> VariantDiscr {
    match v {
      rustc::ty::VariantDiscr::Explicit(did) =>
        VariantDiscr::Explicit(did.into()),
      rustc::ty::VariantDiscr::Relative(v) =>
        VariantDiscr::Relative(v),
    }
  }
}
impl Into<rustc::ty::VariantDiscr> for VariantDiscr {
  fn into(self) -> rustc::ty::VariantDiscr {
    match self {
      VariantDiscr::Explicit(did) =>
        rustc::ty::VariantDiscr::Explicit(did.into()),
      VariantDiscr::Relative(v) =>
        rustc::ty::VariantDiscr::Relative(v),
    }
  }
}


#[derive(Debug, Clone, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub enum Visibility {
  /// Visible everywhere (including in other crates).
  Public,
  /// Visible only in the given crate-local module.
  Restricted(DefIdDef),
  /// Not visible anywhere in the local crate. This is the visibility of private external items.
  Invisible,
}
impl From<rustc::ty::Visibility> for Visibility {
  fn from(v: rustc::ty::Visibility) -> Self {
    match v {
      rustc::ty::Visibility::Public => Visibility::Public,
      rustc::ty::Visibility::Restricted(did) =>
        Visibility::Restricted(did.into()),
      rustc::ty::Visibility::Invisible => Visibility::Invisible,
    }
  }
}
impl Into<rustc::ty::Visibility> for Visibility {
  fn into(self) -> rustc::ty::Visibility {
    match self {
      Visibility::Public => rustc::ty::Visibility::Public,
      Visibility::Restricted(did) =>
        rustc::ty::Visibility::Restricted(did.into()),
      Visibility::Invisible => rustc::ty::Visibility::Invisible,
    }
  }
}

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct AdtDefDataFlags: u32 {
        const NO_ADT_FLAGS        = 0;
        const IS_ENUM             = 1 << 0;
        const IS_PHANTOM_DATA     = 1 << 1;
        const IS_FUNDAMENTAL      = 1 << 2;
        const IS_UNION            = 1 << 3;
        const IS_BOX              = 1 << 4;
    }
}

impl<'a> From<&'a rustc::ty::AdtDef> for AdtDefDataFlags {
  fn from(v: &'a rustc::ty::AdtDef) -> Self {
    let mut t = AdtDefDataFlags::empty();

    if v.is_enum() {
      t |= AdtDefDataFlags::IS_ENUM;
    }
    if v.is_phantom_data() {
      t |= AdtDefDataFlags::IS_PHANTOM_DATA;
    }
    if v.is_fundamental() {
      t |= AdtDefDataFlags::IS_FUNDAMENTAL;
    }
    if v.is_union() {
      t |= AdtDefDataFlags::IS_UNION;
    }
    if v.is_box() {
      t |= AdtDefDataFlags::IS_BOX;
    }

    t
  }
}

#[derive(Debug, Clone, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub struct FieldDef {
  pub did: DefIdDef,
  pub ty: Ty,
  pub name: String,
  pub vis: Visibility,
}
#[derive(Debug, Clone, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub struct VariantDef {
  pub did: DefIdDef,
  pub name: String,
  pub discr: VariantDiscr,
  pub fields: Vec<FieldDef>,
  pub ctor_kind: CtorKind,
}
#[derive(Debug, Clone, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub struct AdtDefData {
  pub did: DefIdDef,
  pub variants: Vec<VariantDef>,
  pub flags: AdtDefDataFlags,
  pub repr: ReprOptionsDef,
}

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct ReprFlagsDef: u8 {
        const IS_C               = 1 << 0;
        const IS_PACKED          = 1 << 1;
        const IS_SIMD            = 1 << 2;
        // Internal only for now. If true, don't reorder fields.
        const IS_LINEAR          = 1 << 3;

        // Any of these flags being set prevent field reordering optimisation.
        const IS_UNOPTIMISABLE   = ReprFlagsDef::IS_C.bits |
                                   ReprFlagsDef::IS_PACKED.bits |
                                   ReprFlagsDef::IS_SIMD.bits |
                                   ReprFlagsDef::IS_LINEAR.bits;
    }
}
impl From<rustc::ty::ReprFlags> for ReprFlagsDef {
  fn from(v: rustc::ty::ReprFlags) -> ReprFlagsDef {
    ReprFlagsDef::from_bits_truncate(v.bits())
  }
}

#[derive(Debug, Clone, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub struct ReprOptionsDef {
  pub int: Option<IntTy>,
  pub align: u32,
  pub flags: ReprFlagsDef,
}
impl From<rustc::ty::ReprOptions> for ReprOptionsDef {
  fn from(v: rustc::ty::ReprOptions) -> Self {
    let rustc::ty::ReprOptions {
      int, align, flags,
    } = v;
    ReprOptionsDef {
      int: int.map(|v| v.into() ),
      align,
      flags: flags.into(),
    }
  }
}

#[derive(Debug, Clone, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub struct ExistentialTraitRef {
  pub def_id: DefIdDef,
  pub substs: Substs,
}

#[derive(Debug, Clone, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub struct ExistentialProjection {
  pub item_def_id: DefIdDef,
  pub substs: Substs,
  pub ty: Ty,
}

#[derive(Debug, Clone, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub enum AutoTrait {
  Send,
}

#[derive(Debug, Clone, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub enum ExistentialPredicate {
  Trait(ExistentialTraitRef),
  Projection(ExistentialProjection),
  AutoTrait(AutoTrait),
}

newtype_idx!(#[derive(Deserialize, Serialize)] pub struct Dynamic => "dynamic");
#[derive(Debug, Clone, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub struct DynamicData {
  trait_data: Vec<ExistentialPredicate>
}

newtype_idx!(#[derive(Deserialize, Serialize)] pub struct Ty => "type");
#[derive(Debug, Clone, Hash, PartialEq)]
#[derive(Eq, Serialize, Deserialize)]
pub enum TyData {
  Primitive(PrimTy),
  Str,
  Array(Ty, super::ConstVal),
  Slice(Ty),
  Tuple(Vec<Ty>, bool),
  RawPtr(TypeAndMut),
  Ref(Region, TypeAndMut),
  FnDef(DefIdDef, Substs),
  Adt(super::AdtDef, Substs),
  Never,
  Dynamic(Dynamic),

  FatPtr {
    data: Ty,
    info: super::VTable,
  },
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