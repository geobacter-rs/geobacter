
pub fn main() {
  // we need the current target triple so we can
  // setup icecc.
  println!("cargo:rustc-env=TARGET={}",
           std::env::var("TARGET").unwrap());
}
