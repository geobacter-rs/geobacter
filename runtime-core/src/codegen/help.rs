//! Helpers for platform specific codegen steps.

use std::env::{var_os, };
use std::path::{Path, PathBuf, };

use rustc_session::config::host_triple;

const MESSAGE: &'static str =
  "Please provide `RUST_BUILD_ROOT` or `LLVM_BUILD` so I can use the LLVM \
   tools contained within.";

/// Helper for finding LLVM tools.
pub struct LlvmBuildRoot(PathBuf);
impl LlvmBuildRoot {
  pub fn llvm_root(&self) -> &PathBuf { &self.0 }
  pub fn llvm_tool<T>(&self, tool: T) -> PathBuf
    where T: AsRef<Path>,
  {
    self.0
      .join("bin")
      .join(tool)
  }
  pub fn llc(&self) -> PathBuf {
    self.llvm_tool("llc")
  }
  pub fn lld(&self) -> PathBuf {
    self.llvm_tool("ld.lld")
  }
  pub fn clang(&self) -> PathBuf {
    self.llvm_tool("clang")
  }
  pub fn clangxx(&self) -> PathBuf {
    self.llvm_tool("clang++")
  }
  pub fn ar(&self) -> PathBuf {
    self.llvm_tool("llvm-ar")
  }
  pub fn ranlib(&self) -> PathBuf {
    self.llvm_tool("llvm-ranlib")
  }
}
impl Default for LlvmBuildRoot {
  fn default() -> Self {
    if let Some(root) = var_os("RUST_BUILD_ROOT") {
      let llvm = PathBuf::from(root)
        .join(host_triple())
        .join("llvm");

      return LlvmBuildRoot(llvm);
    }

    let root = var_os("LLVM_BUILD")
      .expect(MESSAGE);
    LlvmBuildRoot(root.into())
  }
}
