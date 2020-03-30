
use std::io::{Error as IoError, };
use std::path::{PathBuf};

pub fn self_exe_path() -> Result<PathBuf, IoError> {
  use std::fs::read_link;

  const P: &'static str = "/proc/self/exe";

  Ok(read_link(P)?)
}

pub fn dylib_search_paths() -> Vec<PathBuf> {
  use std::env::{var_os, split_paths};

  let paths = var_os("LD_LIBRARY_PATH").unwrap_or("".into());
  split_paths(&paths).collect()
}
