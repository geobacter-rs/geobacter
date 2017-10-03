#![feature(rustc_private)]
#![feature(i128_type)]
#![feature(compiler_builtins_lib)]

extern crate spirv_headers;
extern crate rspirv;
extern crate num_traits;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate indexvec;

#[macro_use]
extern crate rustc;
extern crate rustc_data_structures;
extern crate rustc_const_math;
extern crate syntax;
extern crate syntax_pos;
extern crate compiler_builtins;

use utils::HashableFloat;

use indexvec::IndexVec;

use syntax_pos::symbol::Symbol;

use rustc::hir::def_id::{CrateNum, DefIndex, DefId};
use rustc::mir::{SourceInfo};
use rustc::ty::{TyS};
use rustc_data_structures::indexed_vec::{Idx as RustcIdx};
use rustc_const_math::{ConstInt, ConstFloat, ConstUsize, ConstIsize};

pub use syntax_pos::{Span, BytePos, SyntaxContext};

pub use tys::{Mutability, Ty, TyData, PrimTy};
use rustc_wrappers::{SymbolDef, SpanDef, SourceInfoDef, CrateNumDef,
                     DefIndexDef, DefIdDef, ConstFloatDef,
                     ConstIntDef, ConstIsizeDef, ConstUsizeDef};

pub mod tys;
pub mod utils;
pub mod rustc_wrappers;
pub mod builder;

macro_rules! duplicate_enum_for_serde {
  (pub enum $name:ident as $rustc_ty:path {
    $($variant:ident = $rustc_expr:path,)*
  }) => {
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub enum $name {
      $($variant),*
    }
    impl From<$rustc_ty> for $name {
      fn from(v: $rustc_ty) -> $name {
        match v {
          $($rustc_expr => $name::$variant,)*
        }
      }
    }
    impl Into<$rustc_ty> for $name {
      fn into(self) -> $rustc_ty {
        match self {
          $($name::$variant => $rustc_expr,)*
        }
      }
    }
  };
}

newtype_idx!(#[derive(Deserialize, Serialize)] pub struct VisibilityScope => "visibility-scope");
newtype_idx!(#[derive(Deserialize, Serialize)] pub struct BasicBlock => "basic-block");
newtype_idx!(#[derive(Deserialize, Serialize)] pub struct Local => "local");
newtype_idx!(#[derive(Deserialize, Serialize)] pub struct Promoted => "promoted");
newtype_idx!(#[derive(Deserialize, Serialize)] pub struct ConstVal => "constant-value");
newtype_idx!(#[derive(Deserialize, Serialize)] pub struct Substs => "substitution");
newtype_idx!(#[derive(Deserialize, Serialize)] pub struct Field => "field");
newtype_idx!(#[derive(Deserialize, Serialize)] pub struct Function => "function");

pub type Functions   = IndexVec<Function, FunctionData>;
pub type BasicBlocks = IndexVec<BasicBlock, BasicBlockData>;
pub type VisibilityScopes = IndexVec<VisibilityScope, VisibilityScopeData>;
pub type Promoteds = IndexVec<Promoted, FunctionData>; // Sorry..
pub type LocalDecls = IndexVec<Local, LocalDecl>;
pub type Types = IndexVec<Ty, TyData>;
pub type Substss = IndexVec<Substs, Vec<TsKind>>;
pub type ConstVals = IndexVec<ConstVal, ConstValData>;

pub const RETURN_POINTER: Local = Local(0);
pub const MAIN_FUNCTION: Function = Function(0);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Module {
  pub funcs: Functions,
  pub tys: Types,
  pub substs: Substss,
  pub const_vals: ConstVals,
}
impl Module {
  pub fn new() -> Module {
    Module {
      funcs: Default::default(),
      tys: Default::default(),
      substs: Default::default(),
      const_vals: Default::default(),
    }
  }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FunctionKind {
  pub basic_blocks: BasicBlocks,
  pub visibility_scopes: VisibilityScopes,
  pub promoted: Promoteds,
  pub local_decls: LocalDecls,
  pub upvar_decls: Vec<UpvarDecl>,
  pub spread_arg: Option<Local>,
  // None during construction.
  pub return_ty: Option<Ty>,
  pub span: SpanDef,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum FunctionData {
  Function(FunctionKind),
  Intrinsic(String, Vec<Ty>),
}

impl FunctionData {
  pub fn new(span: SpanDef) -> FunctionData {
    FunctionData::Function(FunctionKind {
      basic_blocks: Default::default(),
      visibility_scopes: Default::default(),
      promoted: Default::default(),
      local_decls: Default::default(),
      upvar_decls: Default::default(),
      spread_arg: None,
      return_ty: None,
      span,
    })
  }
  pub fn intrinsic(name: String, sig: Vec<Ty>) -> FunctionData {
    FunctionData::Intrinsic(name, sig)
  }

  pub fn fkr(&self) -> &FunctionKind {
    match self {
      &FunctionData::Function(ref f) => f,
      &FunctionData::Intrinsic(..) => bug!("not a function: {:?}", self),
    }
  }
  pub fn fkm(&mut self) -> &mut FunctionKind {
    match self {
      &mut FunctionData::Function(ref mut f) => f,
      &mut FunctionData::Intrinsic(..) => bug!("not a function: {:?}", self),
    }
  }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BasicBlockData {
  pub statements: Vec<Statement>,
  pub terminator: Terminator,
  pub is_cleanup: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Terminator {
  pub source_info: SourceInfoDef,
  pub kind: TerminatorKind,
}
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum TerminatorKind {
  Goto {
    target: BasicBlock,
  },
  SwitchInt {
    discr: Operand,
    switch_ty: Ty,
    values: Vec<ConstIntDef>,
    targets: Vec<BasicBlock>,
  },
  Resume,
  Return,
  Unreachable,
  Drop {
    location: Lvalue,
    target: BasicBlock,
    unwind: Option<BasicBlock>
  },
  DropAndReplace {
    location: Lvalue,
    value: Operand,
    target: BasicBlock,
    unwind: Option<BasicBlock>,
  },
  Call {
    func: Operand,
    args: Vec<Operand>,
    destination: Option<(Lvalue, BasicBlock)>,
    cleanup: Option<BasicBlock>
  },
  Assert {
    cond: Operand,
    expected: bool,
    msg: AssertMessage,
    target: BasicBlock,
    cleanup: Option<BasicBlock>
  }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub enum ConstOp {
  Add,
  Sub,
  Mul,
  Div,
  Rem,
  Shr,
  Shl,
  Neg,
  BitAnd,
  BitOr,
  BitXor,
}
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum ConstMathErr {
  NotInRange,
  CmpBetweenUnequalTypes,
  UnequalTypes(ConstOp),
  Overflow(ConstOp),
  ShiftNegative,
  DivisionByZero,
  RemainderByZero,
  UnsignedNegation,
  ULitOutOfRange(PrimTy),
  LitOutOfRange(PrimTy),
}
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum AssertMessage {
  BoundsCheck {
    len: Operand,
    index: Operand,
  },
  Math(ConstMathErr)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Statement {
  pub source_info: SourceInfoDef,
  pub kind: StatementKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StatementKind {
  Assign(Lvalue, Rvalue),
  SetDiscriminant { lvalue: Lvalue, variant_index: usize },

  StorageLive(Local),
  StorageDead(Local),

  // Not allowed.
  /*InlineAsm {
    asm: Box<InlineAsm>,
    outputs: Vec<Lvalue<'tcx>>,
    inputs: Vec<Operand<'tcx>>
  },*/

  /*Validate(ValidationOp, Vec<ValidationOperand<'tcx, Lvalue<'tcx>>>),*/

  // Removed from MIR before we run.
  /*EndRegion(CodeExtent),*/
  Nop,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash)]
pub enum TsKind {
  Type(Ty),
  Region(RegionKind),
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum ConstValData {
  Float(ConstFloatDef),
  Integral(ConstIntDef),
  Str(String),
  ByteStr(Vec<u8>),
  Bool(bool),
  Char(char),
  Variant(DefIdDef),
  Function(Function),
  Struct(Vec<(String, ConstVal)>),
  Tuple(Vec<ConstVal>),
  Array(Vec<ConstVal>),
  Repeat(ConstVal, u64),
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Constant {
  pub span: SpanDef,
  pub ty: Ty,
  pub literal: Literal,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum Literal {
  Value {
    value: ConstVal,
  },
  Promoted(Promoted),
}
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum Operand {
  Consume(Lvalue),
  Constant(Box<Constant>),
}
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum Lvalue {
  Local(Local),
  Static(Static),
  Projection(Box<LvalueProjection>),
}
pub type LvalueProjection = Projection<Lvalue, Local, Ty>;
pub type LvaleElem = ProjectionElem<Local, Ty>;

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Projection<B, V, T> {
  pub base: B,
  pub elem: ProjectionElem<V, T>,
}
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum ProjectionElem<V, T> {
  Deref,
  Field(Field, T),
  Index(V),
  ConstantIdx {
    offset: u32,
    min_length: u32,
    from_end: bool,
  },
  Subslice {
    from: u32,
    to: u32,
  },
  Downcast,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum Rvalue {
  /// x (either a move or copy, depending on type of x)
  Use(Operand),

  /// [x; 32]
  Repeat(Operand, ConstUsizeDef),

  /// &x or &mut x
  Ref(RegionKind, BorrowKind, Lvalue),

  /// length of a [X] or [X;n] value
  Len(Lvalue),

  Cast(CastKind, Operand, Ty),

  BinaryOp(BinOp, Operand, Operand),
  CheckedBinaryOp(BinOp, Operand, Operand),

  NullaryOp(NullOp, Ty),
  UnaryOp(UnOp, Operand),

  /// Read the discriminant of an ADT.
  ///
  /// Undefined (i.e. no effort is made to make it defined, but there’s no reason why it cannot
  /// be defined to return, say, a 0) if ADT is not an enum.
  Discriminant(Lvalue),

  /// Create an aggregate value, like a tuple or struct.  This is
  /// only needed because we want to distinguish `dest = Foo { x:
  /// ..., y: ... }` from `dest.x = ...; dest.y = ...;` in the case
  /// that `Foo` has a destructor. These rvalues can be optimized
  /// away after type-checking and before lowering.
  Aggregate(Box<AggregateKind>, Vec<Operand>),
}
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum AggregateKind {
  Array(Ty),
  Tuple,
  Adt,
  Closure(DefIdDef, Substs),
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash)]
pub enum RegionKind {
  Erased,
}

#[derive(Clone, Serialize, Deserialize, Hash, Eq, PartialEq, Debug)]
pub struct LocalDecl {
  pub mutability: Mutability,
  pub is_user_variable: bool,
  pub ty: Ty,
  pub name: Option<SymbolDef>,
  pub source_info: SourceInfoDef,
}
#[derive(Clone, Serialize, Deserialize, Hash, Eq, PartialEq, Debug)]
pub struct UpvarDecl {
  pub debug_name: SymbolDef,
  pub by_ref: bool,
}
#[derive(Clone, Serialize, Deserialize, Hash, Eq, PartialEq, Debug)]
pub struct Static {
  pub def_id: DefIdDef,
  pub ty: Ty,
}
#[derive(Clone, Serialize, Deserialize, Hash, Eq, PartialEq, Debug)]
pub struct VisibilityScopeData {
  pub span: SpanDef,
  pub parent_scope: Option<VisibilityScope>,
}

duplicate_enum_for_serde! {
pub enum CastKind as rustc::mir::CastKind {
  Misc = rustc::mir::CastKind::Misc,
  ReifyFnPointer = rustc::mir::CastKind::ReifyFnPointer,
  ClosureFnPointer = rustc::mir::CastKind::ClosureFnPointer,
  UnsafeFnPointer = rustc::mir::CastKind::UnsafeFnPointer,
  Unsize = rustc::mir::CastKind::Unsize,
}
}
duplicate_enum_for_serde! {
pub enum BorrowKind as rustc::mir::BorrowKind {
  Shared = rustc::mir::BorrowKind::Shared,
  Unique = rustc::mir::BorrowKind::Unique,
  Mutable = rustc::mir::BorrowKind::Mut,
}
}
duplicate_enum_for_serde! {
pub enum LocalKind as rustc::mir::LocalKind {
  Var = rustc::mir::LocalKind::Var,
  Temp = rustc::mir::LocalKind::Temp,
  Arg = rustc::mir::LocalKind::Arg,
  ReturnPointer = rustc::mir::LocalKind::ReturnPointer,
}
}
duplicate_enum_for_serde! {
pub enum BinOp as rustc::mir::BinOp {
  Add = rustc::mir::BinOp::Add,
  Sub = rustc::mir::BinOp::Sub,
  Mul = rustc::mir::BinOp::Mul,
  Div = rustc::mir::BinOp::Div,
  Rem = rustc::mir::BinOp::Rem,
  BitXor = rustc::mir::BinOp::BitXor,
  BitAnd = rustc::mir::BinOp::BitAnd,
  BitOr = rustc::mir::BinOp::BitOr,
  Shl = rustc::mir::BinOp::Shl,
  Shr = rustc::mir::BinOp::Shr,
  Eq = rustc::mir::BinOp::Eq,
  Lt = rustc::mir::BinOp::Lt,
  Le = rustc::mir::BinOp::Le,
  Ne = rustc::mir::BinOp::Ne,
  Ge = rustc::mir::BinOp::Ge,
  Gt = rustc::mir::BinOp::Gt,
  Offset = rustc::mir::BinOp::Offset,
}
}
duplicate_enum_for_serde! {
pub enum NullOp as rustc::mir::NullOp {
  SizeOf = rustc::mir::NullOp::SizeOf,
  Box = rustc::mir::NullOp::Box,
}
}
duplicate_enum_for_serde! {
pub enum UnOp as rustc::mir::UnOp {
  Not = rustc::mir::UnOp::Not,
  Neg = rustc::mir::UnOp::Neg,
}
}

impl From<rustc::mir::VisibilityScope> for VisibilityScope {
  fn from(v: rustc::mir::VisibilityScope) -> Self {
    VisibilityScope(v.index())
  }
}
impl Into<rustc::mir::VisibilityScope> for VisibilityScope {
  fn into(self) -> rustc::mir::VisibilityScope {
    rustc::mir::VisibilityScope::new(self.0 as _)
  }
}
impl From<rustc::mir::BasicBlock> for BasicBlock {
  fn from(v: rustc::mir::BasicBlock) -> Self {
    BasicBlock(v.index())
  }
}
impl From<rustc::mir::Promoted> for Promoted {
  fn from(v: rustc::mir::Promoted) -> Promoted {
    Promoted(v.index())
  }
}
impl From<rustc::mir::Local> for Local {
  fn from(v: rustc::mir::Local) -> Local {
    Local(v.index())
  }
}
impl From<rustc::mir::Field> for Field {
  fn from(v: rustc::mir::Field) -> Field {
    Field(v.index())
  }
}
