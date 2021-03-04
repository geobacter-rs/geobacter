
use std::env::{var_os};
use std::path::{PathBuf};

extern crate bindgen;

pub fn main() {
  let out_dir: PathBuf = From::from(var_os("OUT_DIR").unwrap());

  // XXX linux only
  let bindings = bindgen::Builder::default()
    .header("src/wrapper.h")
    .rust_target(bindgen::RustTarget::Nightly)
    .clang_arg("-I/opt/rocm/include")
    .clang_arg("-I/opt/rocm/hcc/lib/clang/9.0.0/include")
    .derive_debug(true)
    .derive_copy(true)
    .derive_hash(true)
    .derive_eq(true)
    .derive_ord(true)
    .rustfmt_bindings(true)
    .prepend_enum_name(false)
    .generate()
    .unwrap();

  bindings.write_to_file(out_dir.join("bindings.rs"))
    .unwrap();

  println!("cargo:rustc-link-search=native=/opt/rocm/lib");
  println!("cargo:rustc-link-lib=dylib=amd_comgr");
}
