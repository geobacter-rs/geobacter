
//! The results of a successful codegen.

use std::collections::{BTreeMap, };

use rustc::session::config::{OutputType, };

use codegen::worker::CodegenDesc;

#[derive(Debug, Clone)]
pub struct CodegenResults {
  /// Rust uses a hash of a bunch of different values in `Session`
  /// in all symbols, which means we'll have different symbol names
  /// for different accelerators.
  pub symbol: String,
  pub desc: CodegenDesc,
  pub outputs: BTreeMap<OutputType, Vec<u8>>,
}
