use std::env::{var_os, };
use std::error::Error;
use std::hash::{Hash, Hasher, };
use std::io;
use std::path::{Path, PathBuf, };
use std::sync::{Arc, };

use seahash::SeaHasher;

use {AcceleratorTargetDesc, DeviceLibsBuilder, };
use utils::CreateIfNotExists;

use rustc::session::config::host_triple;

pub use self::amd::*;

pub mod host;
pub mod amd;

pub struct DeviceLibsStaging<'a> {
  staging_path: &'a Path,
  target_desc: &'a Arc<AcceleratorTargetDesc>,
  builder: Box<DeviceLibsBuilder>,

  bc_obj_libs: Vec<PathBuf>,
}

impl<'a> DeviceLibsStaging<'a> {
  pub fn build(&mut self) -> io::Result<DeviceLibsBuild> {
    let mut hasher = SeaHasher::default();

    self.target_desc.hash(&mut hasher);

    let hash = hasher.finish();

    let build_path = self.staging_path
      .join(format!("{}", hash));

    build_path.create_if_not_exists()?;

    Ok(DeviceLibsBuild {
      build_path,
      target_desc: &self.target_desc,
      builder: Some(&self.builder),
      bc_obj_libs: &mut self.bc_obj_libs,
    })
  }
}

pub struct DeviceLibsBuild<'a> {
  build_path: PathBuf,

  target_desc: &'a Arc<AcceleratorTargetDesc>,
  builder: Option<&'a Box<DeviceLibsBuilder>>,

  bc_obj_libs: &'a mut Vec<PathBuf>,
}

impl<'a> DeviceLibsBuild<'a> {
  pub fn build(&mut self) -> Result<(), Box<Error>> {
    let builder = self.builder.take().unwrap();

    let r = builder.run_build(self);

    self.builder = Some(builder);

    r
  }

  pub fn build_path(&self) -> &Path {
    self.build_path.as_ref()
  }
  pub fn target_desc(&self) -> &AcceleratorTargetDesc {
    &**self.target_desc
  }

  pub fn add_bc_obj(&mut self, bc: PathBuf) {
    self.bc_obj_libs.push(bc);
  }
}

pub struct RustBuildRoot(PathBuf);
impl RustBuildRoot {
  pub fn llvm_root(&self) -> PathBuf {
    self.0
      .join(host_triple())
      .join("llvm")
  }
  pub fn llvm_tool<T>(&self, tool: T) -> PathBuf
    where T: AsRef<Path> + for<'a> PartialEq<&'a str>,
  {
    self.0
      .join(host_triple())
      .join("llvm/bin")
      .join(tool)
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
impl Default for RustBuildRoot {
  fn default() -> Self {
    let root = var_os("RUST_BUILD_ROOT")
      .expect("Please provide `RUST_BUILD_ROOT` so I can use the LLVM tools contained within.");
    RustBuildRoot(root.into())
  }
}
