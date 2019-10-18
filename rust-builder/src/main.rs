
//! This bin builds a vanilla Rust toolchain suitable for use with
//! Geobacter, then installs the full Geobacter specific `rustc`
//! driver into it.
//!
//! It is intended that development of the Rust Geobacter patches
//! is done in a separate checkout, which is passed to us via `--repo-url`
//!

use std::env::{current_dir, var, };
use std::fs::{copy, File, read_dir, };
use std::io::*;
use std::path::{Path, PathBuf, };
use std::process::Command;

use clap::*;

use which::which;

mod git;

const RUST_REPO_URL: &str = "https://github.com/geobacter-rs/rust.git";
const RUST_REPO_BRANCH: &str = "merge-head";

pub fn main() {
  let repo_url = Arg::with_name("repo-url")
    .long("repo-url")
    .help("override the Geobacter Rust url")
    .takes_value(true)
    .default_value(RUST_REPO_URL);
  let repo_branch = Arg::with_name("repo-branch")
    .long("repo-branch")
    .help("override the Geobacter Rust branch")
    .takes_value(true)
    .default_value(RUST_REPO_BRANCH)
    .requires("repo-url");

  // If "build-config" is not given, we manage our own.
  let config_path = Arg::with_name("build-config")
    .long("config")
    .help("path to custom config.toml")
    .takes_value(true);

  let build_docs = Arg::with_name("docs")
    .long("docs")
    .help("build docs, including compiler docs");

  let cdir = current_dir().unwrap();
  let target_dir = Arg::with_name("target-dir")
    .long("target-dir")
    .help("specific where we will put the rust sources and build artifacts")
    .takes_value(true)
    .required(true)
    .default_value(cdir.to_str().unwrap());

  let rustup = Arg::with_name("rustup")
    .long("rustup")
    .help("setup Rustup with the built toolchain");
  let rustup_name = Arg::with_name("rustup-toolchain")
    .long("rustup-toolchain")
    .help("Rustup toolchain name")
    .takes_value(true);

  let matches = App::new("Geobacter Rust Toolchain Builder")
    .version("0.0.0")
    .author("Richard Diamond <wichard@vitalitystudios.com>")
    .arg(repo_url)
    .arg(repo_branch)
    .arg(config_path)
    .arg(build_docs)
    .arg(target_dir)
    .arg(rustup)
    .arg(rustup_name)
    .get_matches();

  matches.run();
}

const CRATE_ROOT: &'static str = env!("CARGO_MANIFEST_DIR");
fn get_framework_dir() -> PathBuf {
  Path::new(CRATE_ROOT).join("../")
}
fn get_framework_target() -> PathBuf {
  get_framework_dir()
    .join("target")
}
fn get_crate_manifest(krate: &str) -> PathBuf {
  get_framework_dir()
    .join(krate)
    .join("Cargo.toml")
}

const RUST_FLAGS: &str = "-Z always-encode-mir -Z always-emit-metadata";
fn get_rust_flags() -> String {
  let mut prev = var("RUSTFLAGS").unwrap_or_default();
  if prev.len() != 0 {
    prev.push_str(" ");
  }

  prev.push_str(RUST_FLAGS);
  prev
}
trait Builder {
  fn repo_url(&self) -> &str;
  fn repo_branch(&self) -> &str;
  fn rustup_enabled(&self) -> bool;
  fn rustup_toolchain(&self) -> &str;
  fn rustup(&self) -> Option<&str> {
    if self.rustup_enabled() {
      Some(self.rustup_toolchain())
    } else {
      None
    }
  }

  fn given_config_path(&self) -> Option<&str>;
  fn target_dir(&self) -> &Path;
  fn config_path(&self) -> PathBuf {
    if let Some(p) = self.given_config_path() {
      return p.into();
    }

    self.target_dir().join("config.toml")
  }
  fn build_dir(&self) -> PathBuf {
    self.target_dir().join("build")
  }
  fn src_dir(&self) -> PathBuf {
    self.target_dir().join("src")
  }
  fn x_py(&self) -> PathBuf {
    self.src_dir().join("x.py")
  }
  fn x_py_command(&self, task: &str) -> Command {
    let mut cmd = Command::new(self.x_py());
    cmd.current_dir(self.target_dir())
      .arg(task)
      .arg("--config").arg(self.config_path())
      .env("RUSTFLAGS_NOT_STAGE_0", RUST_FLAGS);

    cmd
  }
  fn run_cargo_command(&self, cargo_cmd: &str, krate: PathBuf,
                       wrapper: Option<&Path>)
  {
    let mut cmd = Command::new("cargo");
    cmd.arg(cargo_cmd)
      .arg("--release")
      .arg("--manifest-path").arg(krate);

    if let Some(wrapper) = wrapper {
      cmd.env("RUSTC", wrapper);
    }

    cmd.env("RUSTFLAGS", get_rust_flags());

    run_unlogged_cmd("cargo", cmd);
  }
  fn framework_output_dir(&self) -> PathBuf {
    get_framework_target()
      .join("release")
  }
  fn framework_binary(&self, binary: &str,
                      krate: Option<&str>,
                      wrapper: Option<&Path>) -> PathBuf
  {
    let krate = get_crate_manifest(krate.unwrap_or(binary));
    self.run_cargo_command("build", krate, wrapper);

    self.framework_output_dir().join(binary)
  }
  fn install_driver(&self, driver: &str, binary: &str,
                    krate: Option<&str>,
                    wrapper: Option<&Path>) -> PathBuf
  {
    let default_wrapper = self.find_stage2_dir()
      .join("bin/rustc");
    let wrapper = wrapper.unwrap_or(&default_wrapper);
    let output = self.framework_binary(binary, krate, Some(wrapper));
    let dest = self.find_stage2_dir().join("bin").join(driver);
    copy(output, &dest).unwrap();

    let mut cmd = Command::new("chrpath");
    cmd.arg("-r")
      .arg("'$ORIGIN/../lib'")
      .arg(&dest);
    run_unlogged_cmd("chrpath", cmd);

    dest
  }
  fn install_bootstrap_driver(&self) -> PathBuf {
    self.install_driver("bootstrap-rustc",
                        "bootstrap-rustc-driver",
                        None, None)
  }

  /// Find the stage2 dir we give to `rustup`. This dir is inside a directory
  /// which depends on the host triple so we have to do a tiny search
  fn find_stage2_dir(&self) -> PathBuf {
    let read_dir = read_dir(self.build_dir()).unwrap();

    macro_rules! t {
      ($t:expr) => {
        match $t {
          Ok(v) => v,
          Err(_) => continue,
        }
      };
    }

    for dir in read_dir {
      let dir = t!(dir);
      if !t!(dir.file_type()).is_dir() {
        continue;
      }
      let path = dir.path().join("stage2");
      if path.exists() {
        return path;
      }
    }

    panic!("toolchain stage2 dir not found!");
  }

  fn docs_enabled(&self) -> bool;

  fn checkout_rust_sources(&self) {
    let url = self.repo_url();
    let branch = self.repo_branch();
    git::checkout_or_override("rust", &self.src_dir(),
                              None, url, branch,
                              true)
      .unwrap();
  }
  fn write_config_toml(&self) {
    if self.given_config_path().is_some() { return; }

    let config = self.config_path();
    let mut config = File::create(config).unwrap();

    write!(config, r#"
[llvm]
release-debuginfo = false
assertions = false
"#).unwrap();
    if which("ccache").is_ok() {
      writeln!(config, "ccache = true").unwrap();
    }
    if which("ninja").is_ok() {
      writeln!(config, "ninja = true").unwrap();
    }
    write!(config, r#"
targets = "X86;ARM;AMDGPU;AArch64;Mips;PowerPC;SystemZ;MSP430;Sparc;NVPTX;Hexagon"
"#).unwrap();
    write!(config, r#"
[build]
compiler-docs = true
submodules = true
low-priority = true

[rust]
debuginfo-level = 2
incremental = true
parallel-compiler = true
lld = true
llvm-tools = true
"#).unwrap();

    #[cfg(target_os = "linux")]
    fn use_icecc(config: &mut File) {
      let p: &Path = "/usr/lib/icecc/bin/gcc".as_ref();
      if p.exists() {
        write!(config, r#"
[target.{}-unknown-linux-{}]
cc = "/usr/lib/icecc/bin/gcc"
cxx = "/usr/lib/icecc/bin/g++"
ar = "ar"
"#,
               cfg!(target_arch), cfg!(target_env)).unwrap();
      }
    }
    #[cfg(not(target_os = "linux"))]
    fn use_icecc(_: &mut File) { }

    use_icecc(&mut config);
  }
  fn build_docs(&self) {
    if !self.docs_enabled() { return; }

    let cmd = self.x_py_command("doc");
    run_unlogged_cmd("build-rust-docs", cmd);
  }
  fn build_toolchain(&self) {
    let cmd = self.x_py_command("build");
    run_unlogged_cmd("build-rust", cmd);
  }
  fn install_geobacter_drivers(&self) -> PathBuf {
    let bootstrap = self.install_bootstrap_driver();
    self.install_driver("rustc", "geobacter-rustc-driver",
                        Some("rustc-driver"), Some(&bootstrap))
  }
  fn install_toolchain_into_rustup(&self) {
    if let Some(name) = self.rustup() {
      let stage2 = self.find_stage2_dir();
      let mut cmd = Command::new("rustup");
      cmd.arg("toolchain")
        .arg("link")
        .arg(name)
        .arg(stage2);
      run_unlogged_cmd("setup-rustup", cmd);
    }
  }

  fn run(&self) {
    self.checkout_rust_sources();
    self.write_config_toml();
    self.build_toolchain();
    self.build_docs();
    let driver = self.install_geobacter_drivers();
    self.install_toolchain_into_rustup();

    // verify the driver works:
    let mut cmd = Command::new(driver);
    cmd.arg("--version");
    run_unlogged_cmd("verify-rustc", cmd);

    println!("Complete! :)");
  }
}
impl<'a> Builder for ArgMatches<'a> {
  fn repo_url(&self) -> &str {
    self.value_of("repo-url").unwrap()
  }

  fn repo_branch(&self) -> &str {
    self.value_of("repo-branch").unwrap()
  }

  fn rustup_enabled(&self) -> bool {
    self.is_present("rustup")
  }

  fn rustup_toolchain(&self) -> &str {
    self.value_of("rustup-toolchain").unwrap()
  }

  fn given_config_path(&self) -> Option<&str> {
    self.value_of("build-config")
  }

  fn target_dir(&self) -> &Path {
    self.value_of("target-dir").unwrap()
      .as_ref()
  }

  fn docs_enabled(&self) -> bool {
    self.is_present("docs")
  }
}

fn run_unlogged_cmd(task: &str, mut cmd: Command) {
  println!("({}): Running: {:?}", task, cmd);
  let mut child = cmd.spawn().unwrap();
  assert!(child.wait().unwrap().success(), "{:?}", cmd);
}
