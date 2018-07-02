#![feature(rustc_private)]

/// A build script that forces references to the rustc private crate files,
/// so that we can get rebuilt if any rustc crate changes. Cargo doesn't know
/// how to track these crates on its own.

// import all rustc crates, then read the links in /proc/self/map_files
// to get the paths to the dylibs.
// Note that we don't need to reference rustc_trans.

extern crate rustc;
extern crate rustc_back;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_incremental;
extern crate rustc_lint;
extern crate rustc_metadata;
extern crate rustc_mir;
extern crate rustc_passes;
extern crate rustc_platform_intrinsics;
extern crate rustc_resolve;
extern crate rustc_trans_utils;
extern crate syntax;
extern crate syntax_pos;

use std::env::{var_os};
use std::error::Error;
use std::path::{Path, PathBuf};

#[inline(never)]
fn force_rustc_link() -> rustc::session::Session {
  let opts = rustc::session::config::basic_options();
  let registry = rustc_driver::diagnostics_registry();

  rustc::session::build_session(opts, None, registry)
}
#[inline(never)]
fn force_rustc_deps_link() -> rustc::ty::maps::Providers<'static> {
  // this will pull in the rustc deps:
  let mut providers = rustc::ty::maps::Providers::default();
  rustc_driver::driver::default_provide(&mut providers);
  rustc_driver::driver::default_provide_extern(&mut providers);
  providers
}

#[cfg(unix)]
pub fn get_mapped_files() -> Result<Vec<PathBuf>, Box<Error>> {
  use std::collections::HashSet;
  use std::fs::read_dir;
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
  let mut ordered = vec![];
  ordered.extend(out.into_iter());

  Ok(ordered)
}

pub fn main() {
  use std::fs::File;
  use std::io::{Write};
  use std::time::{SystemTime, UNIX_EPOCH};

  let _ = force_rustc_link();
  let _ = force_rustc_deps_link();

  let out = Path::new(&var_os("OUT_DIR").unwrap()).join("timestamp.rs");

  let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

  let out_str = format!(r#"#![allow(dead_code)]
pub const BUILD_SCRIPT_TIMESTAMP: (u64, u32) = ({}, {});
"#, ts.as_secs(), ts.subsec_nanos());

  let mut out = File::create(out).unwrap();
  out.write_all(out_str.as_ref()).unwrap();

  let host = var_os("HOST").unwrap();
  let target = var_os("TARGET").unwrap();
  if host != target { return; }

  println!("cargo:rerun-if-changed=build.rs");

  let files = get_mapped_files().expect("get_mapped_files");
  for file in files.into_iter() {
    if file.components().any(|c| c.as_os_str() == host ) {
      println!("cargo:rerun-if-changed={}", file.display());
    }
  }
}
