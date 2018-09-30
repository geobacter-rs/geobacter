
use std::path::Path;

pub use self::amd::*;

pub mod host;
pub mod amd;


pub struct DeviceLibsStaging<'a> {
  staging_path: &'a Path,

  bc_obj_libs: Vec<PathBuf>,
}

impl<'a> DeviceLibsStaging<'a> {
  pub fn for_accel<'b, T>(&'b mut self, name: T) -> DeviceLibs<'b>
    where T: AsRef<Path>,
  {
    DeviceLibs
  }
}

pub struct DeviceLibs<'a> {
  staging_path: PathBuf,

  bc_obj_libs: &'a mut Vec<PathBuf>,
}
