use std::error::Error;
use std::path::{Path, PathBuf};
use std::ffi::CString;

#[cfg(unix)]
pub fn path2cstr(p: &Path) -> CString {
  use std::os::unix::prelude::*;
  use std::ffi::OsStr;
  let p: &OsStr = p.as_ref();
  CString::new(p.as_bytes()).unwrap()
}
#[cfg(windows)]
pub fn path2cstr(p: &Path) -> CString {
  CString::new(p.to_str().unwrap()).unwrap()
}
