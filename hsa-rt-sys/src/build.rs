
use std::env::{var_os};
use std::path::{PathBuf};

extern crate bindgen;

pub fn main() {
  let out_dir: PathBuf = From::from(var_os("OUT_DIR").unwrap());

  let bindings = bindgen::Builder::default()
    .header("src/lib/wrapper.hpp")
    .rust_target(bindgen::RustTarget::Nightly)
    .clang_arg("-I/opt/rocm/include/hsa")
    .clang_arg("-I/opt/rocm/hcc/lib/clang/9.0.0/include")
    .derive_debug(true)
    .derive_copy(true)
    .derive_hash(true)
    .derive_eq(true)
    .derive_partialeq(false)
    .impl_partialeq(true)
    .rustfmt_bindings(true)
    .no_copy("hsa_amd_image_descriptor_s") // actually unsized.
    .generate()
    .unwrap();

  bindings.write_to_file(out_dir.join("bindings.rs"))
    .unwrap();

  // XXX linux only
  println!("cargo:rustc-link-search=native=/opt/rocm/lib");
  println!("cargo:rustc-link-lib=dylib=hsa-runtime64");
}
