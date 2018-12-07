use std::env::{var_os, };
use std::error::Error;
use std::path::{Path, PathBuf, };

use {AcceleratorTargetDesc, DeviceLibsBuilder, };
use utils::{CreateIfNotExists, FileLockGuard, };

use rustc::session::config::host_triple;

pub use self::amd::*;

pub mod host;
pub mod amd;

pub struct DeviceLibsStaging<'a> {
  staging_path: &'a Path,
  target_desc: &'a AcceleratorTargetDesc,
  builder: Box<DeviceLibsBuilder>,

  bc_obj_libs: Vec<PathBuf>,
}

impl<'a> DeviceLibsStaging<'a> {
  pub fn new(device_libs_dir: &'a Path,
             target_desc: &'a AcceleratorTargetDesc,
             builder: Box<DeviceLibsBuilder>)
    -> Self
  {
    DeviceLibsStaging {
      staging_path: device_libs_dir,
      target_desc,
      builder,

      bc_obj_libs: vec![],
    }
  }
  pub fn create_build(&mut self) -> Result<DeviceLibsBuild, Box<Error>> {
    let hash = self.target_desc.get_stable_hash();

    let build_path = self.staging_path
      .join(format!("{}", hash));

    build_path.create_if_not_exists()?;

    Ok(DeviceLibsBuild {
      _lock: FileLockGuard::enter_create(build_path.join("build.lock"))?,
      build_path,
      target_desc: &self.target_desc,
      builder: Some(&self.builder),
      bc_obj_libs: &mut self.bc_obj_libs,
    })
  }
  pub fn into_bc_objs(self) -> Vec<PathBuf> {
    let DeviceLibsStaging {
      bc_obj_libs, ..
    } = self;
    bc_obj_libs
  }
}

pub struct DeviceLibsBuild<'a> {
  _lock: FileLockGuard,
  build_path: PathBuf,

  target_desc: &'a AcceleratorTargetDesc,
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
    self.target_desc
  }

  pub fn add_bc_obj(&mut self, bc: PathBuf) {
    info!("adding {} to the link", bc.display());
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
