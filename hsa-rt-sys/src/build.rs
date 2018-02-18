
use std::env::{var_os};
use std::path::{PathBuf};

extern crate bindgen;

pub fn main() {
  let out_dir: PathBuf = From::from(var_os("OUT_DIR").unwrap());

  let bindings = bindgen::Builder::default()
    .header("src/lib/wrapper.hpp")
    .rust_target(bindgen::RustTarget::Nightly)
    .clang_arg("-I/opt/rocm/include/hsa")
    .derive_debug(true)
    .derive_copy(true)
    .derive_hash(true)
    .derive_eq(true)
    .derive_ord(true)
    // XXX linux only
    //.link("hsa-runtime64")
    .generate()
    .unwrap();

  bindings.write_to_file(out_dir.join("bindings.rs"))
    .unwrap();

  // XXX linux only
  println!("cargo:rustc-link-search=native=/opt/rocm/lib");
  println!("cargo:rustc-link-lib=dylib=hsa-runtime64");
}
