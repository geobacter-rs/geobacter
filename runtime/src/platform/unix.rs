
use std::collections::HashSet;
use std::error::Error;
use std::fmt::Display;
use std::fs::{read_dir};
use std::path::{PathBuf};

pub fn self_exe_path() -> Result<PathBuf, Box<Error>> {
  use std::fs::read_link;

  const P: &'static str = "/proc/self/exe";

  Ok(read_link(P)?)
}

pub fn get_mapped_files() -> Result<Vec<PathBuf>, Box<Error>> {
  // shared objects will have parts mapped in different locations
  let mut out = HashSet::new();

  for entry in read_dir("/proc/self/map_files")? {
    let entry = entry?;
    let path = entry.path();
    let metadata = path.symlink_metadata()?;
    if !metadata.file_type().is_symlink() {
      continue;
    }
    let link = path.read_link()?;
    out.insert(link);
  }

  // make sure the exe is first:
  let self_path = self_exe_path()?;
  out.remove(&self_path);
  let mut ordered = vec![self_path];
  ordered.extend(out.into_iter());

  Ok(ordered)
}

pub fn dylib_search_paths() -> Result<Vec<PathBuf>, Box<Error>> {
  use std::env::{var_os, split_paths};

  let paths = var_os("LD_LIBRARY_PATH").unwrap_or("".into());
  Ok(split_paths(&paths).collect())
}

#[allow(dead_code)]
pub fn locate_dylib<T>(name: T, hash: u64) -> Result<Option<PathBuf>, Box<Error>>
  where T: Display,
{
  use std::env::{var_os, split_paths};

  let full = format!("lib{}-{:x}.so", name, hash);

  let paths = var_os("LD_LIBRARY_PATH").unwrap_or("".into());
  for path in split_paths(&paths) {
    let candidate = path.join(&full);
    debug!("considering {}", candidate.display());
    if candidate.exists() {
      return Ok(Some(candidate));
    }
  }

  Ok(None)
}
