
//! The results of a successful codegen.

use std::collections::{BTreeMap, };

use rustc::session::config::{OutputType, };

#[derive(Debug, Clone)]
pub struct CodegenResults {
  pub kernel_symbol: String,
  pub outputs: BTreeMap<OutputType, Vec<u8>>,
}
