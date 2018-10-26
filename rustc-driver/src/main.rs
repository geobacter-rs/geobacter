#![feature(rustc_private)]

extern crate legionella_intrinsics;
extern crate hsa_core;
extern crate rustc;
extern crate rustc_codegen_utils;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_metadata;
extern crate syntax;
extern crate syntax_pos;

use std::fmt;

use hsa_core::kernel::{KernelId, kernel_id_for, };

use self::rustc_driver::{driver, Compilation, CompilerCalls, RustcDefaultCalls, };
use self::rustc::hir::def_id::{DefId, };
use self::rustc::middle::lang_items;
use self::rustc::mir::{Constant, Operand, Rvalue, Statement,
                       StatementKind, AggregateKind, Local, };
use self::rustc::mir::interpret::{ConstValue, Scalar, };
use self::rustc::mir::{self, CustomIntrinsicMirGen, };
use self::rustc::session::{config, Session, };
use self::rustc::session::config::{ErrorOutputType, Input, };
use self::rustc::ty::{self, TyCtxt, Instance, layout::Size, };
use self::rustc_codegen_utils::codegen_backend::CodegenBackend;
use self::rustc_data_structures::fx::{FxHashMap, };
use self::rustc_data_structures::sync::{Lrc, };
use self::syntax::ast;
use self::syntax_pos::DUMMY_SP;
use self::syntax_pos::symbol::{Symbol, InternedString, };

use legionella_intrinsics::*;

pub fn main() {
  legionella_intrinsics::main(|gen| {
    insert_all_intrinsics(&GeneratorDefIdKernelId,
                          |k, v| {
                            assert!(gen.intrinsics.insert(k, v).is_none());
                          });
  });
}

pub struct GeneratorDefIdKernelId;
impl legionella_intrinsics::DefIdFromKernelId for GeneratorDefIdKernelId {
  fn get_cstore(&self) -> &rustc_metadata::cstore::CStore {
    legionella_intrinsics::generators().cstore()
  }
}
impl legionella_intrinsics::GetDefIdFromKernelId for GeneratorDefIdKernelId {
  fn with_self<F, R>(f: F) -> R
    where F: FnOnce(&dyn DefIdFromKernelId) -> R,
  {
    f(&GeneratorDefIdKernelId)
  }
}
