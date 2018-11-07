
//! The results of a successful codegen.

use std::collections::{BTreeMap, };

use rustc::session::config::{OutputType, };

#[derive(Debug, Clone)]
pub struct CodegenResults {
  /// Rust uses a hash of a bunch of different values in `Session`
  /// in all symbols, which means we'll have different symbol names
  /// for different accelerators.
  pub kernel_symbol: String,
  pub outputs: BTreeMap<OutputType, Vec<u8>>,
}
