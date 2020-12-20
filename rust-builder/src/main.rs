
//! This bin builds a vanilla Rust toolchain suitable for use with
//! Geobacter, then installs the full Geobacter specific `rustc`
//! driver into it.
//!
//! It is intended that development of the Rust Geobacter patches
//! is done in a separate checkout, which is passed to us via `--repo-url`
//!

use std::env::current_dir;
use std::error::Error;
use std::fs::{File, read_dir, };
use std::io::*;
use std::path::{Path, PathBuf, };
use std::process::Command;
use std::result::Result;

use clap::*;

use which::which;

mod git;

const RUST_REPO_URL: &str = "https://github.com/geobacter-rs/rust.git";
const RUST_REPO_BRANCH: &str = "merge-head";

pub fn main() -> Result<(), Box<dyn Error>> {
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
    .help("specific dir where we will put the rust sources and build artifacts")
    .takes_value(true)
    .required(true)
    .default_value(cdir.to_str().unwrap());

  let rustup = Arg::with_name("rustup")
    .long("no-rustup")
    .help("disable Rustup setup with the built toolchain")
    .multiple(true);
  let rustup_name = Arg::with_name("rustup-toolchain")
    .long("rustup-toolchain")
    .help("Rustup toolchain name")
    .takes_value(true)
    .default_value("geobacter");

  let jobs = Arg::with_name("jobs")
    .long("jobs")
    .short("j")
    .help("Usual make-esk '-j'")
    .takes_value(true);

  let rust_dbg_level = Arg::with_name("rust-dbg-lvl")
    .long("rust-debuginfo-level")
    .help("Debug level to use for the Rust toolchain (`0`, `1`, or `2`)")
    .takes_value(true)
    .possible_values(&["0", "1", "2", "true", "false"])
    .default_value("1");

  let llvm_dbginfo = Arg::with_name("llvm-debug-info")
    .long("llvm-debug-info")
    .help("Build LLVM with debug info; default is off");

  let llvm_assertions = Arg::with_name("llvm-assertions")
    .long("llvm-assertions")
    .help("Build LLVM with assertions ON; default is OFF");

  let matches = App::new("Geobacter Rust Toolchain Builder")
    .version("0.0.0")
    .author("Richard Diamond <dick@vitalitystudios.com>")
    .arg(repo_url)
    .arg(repo_branch)
    .arg(config_path)
    .arg(build_docs)
    .arg(target_dir)
    .arg(rustup)
    .arg(rustup_name)
    .arg(jobs)
    .arg(rust_dbg_level)
    .arg(llvm_dbginfo)
    .arg(llvm_assertions)
    .get_matches();

  matches.run()
}

const RUST_FLAGS: &str = "-Z always-encode-mir -Z always-emit-metadata";
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
  fn jobs(&self) -> Option<&str>;
  fn rust_dbg_info_lvl(&self) -> &str;
  fn llvm_build_dbg_info(&self) -> bool;
  fn llvm_enable_assertions(&self) -> bool;

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
    let mut cmd = if cfg!(target_env = "msvc") {
      let mut cmd = Command::new("python");
      cmd.arg(self.x_py());
      cmd
    } else {
      Command::new(self.x_py())
    };
    cmd.current_dir(self.target_dir())
      .arg(task)
      .arg("--config").arg(self.config_path())
      .arg("--stage").arg("2")
      .env("RUSTFLAGS_NOT_BOOTSTRAP", RUST_FLAGS);

    if let Some(j) = self.jobs() {
      cmd.arg("-j")
        .arg(j);
    }

    cmd
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

  fn check_required_tools(&self) -> Result<(), Box<dyn Error>> {
    if self.rustup_enabled() && which("rustup").is_err() {
      return Err("I need `rustup` somewhere in PATH".into());
    }

    Ok(())
  }

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
release-debuginfo = {}
assertions = {}
"#,
           if self.llvm_build_dbg_info() { "true" } else { "false" },
           if self.llvm_enable_assertions() { "true" } else { "false" },
    ).unwrap();
    if which("ccache").is_ok() {
      writeln!(config, "ccache = true").unwrap();
    }
    if which("ninja").is_ok() {
      writeln!(config, "ninja = true").unwrap();
    } else {
      writeln!(config, "ninja = false").unwrap();
    }
    write!(config, r#"
targets = "X86;ARM;AMDGPU;AArch64;Mips;PowerPC;SystemZ;MSP430;Sparc;SPIRV;NVPTX;Hexagon"
"#).unwrap();
    write!(config, r#"
[build]
compiler-docs = true
submodules = true
low-priority = true
full-bootstrap = true

[rust]
debuginfo-level = {}
debuginfo-level-std = 2
incremental = true
parallel-compiler = true
lld = false
llvm-tools = true
"#,
           self.rust_dbg_info_lvl(),
    ).unwrap();

    #[cfg(target_os = "linux")]
    fn use_icecc(config: &mut File) {
      let p: &Path = "/usr/lib/icecc/bin/gcc".as_ref();
      if p.exists() {
        write!(config, r#"
[target.{}]
cc = "/usr/lib/icecc/bin/gcc"
cxx = "/usr/lib/icecc/bin/g++"
ar = "ar"
"#,
               env!("TARGET")).unwrap();
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
  fn geobacter_driver(&self) -> PathBuf {
    self.find_stage2_dir()
      .join("bin/rustc")
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

  fn run(&self) -> Result<(), Box<dyn Error>> {
    self.check_required_tools()?;
    self.checkout_rust_sources();
    self.write_config_toml();
    self.build_toolchain();
    self.build_docs();
    self.install_toolchain_into_rustup();

    let driver = if !self.rustup_enabled() {
      Some(self.geobacter_driver())
    } else {
      None
    };

    // verify the driver works:
    let mut cmd;
    if let Some(ref driver) = driver {
      cmd = Command::new(driver);
    } else {
      cmd = Command::new("rustup");
      cmd.arg("run")
        .arg(self.rustup_toolchain())
        .arg("rustc");
    }
    cmd.arg("--version");
    run_unlogged_cmd("verify-rustc", cmd);

    println!("Complete! :)");
    if let Some(ref driver) = driver {
      println!("To use your new toolchain set \"RUSTC={}\"",
               driver.display());
    } else {
      println!("To use your new toolchain via rustup:");
      println!("either invoke cargo with `+{}` or `rustup override set {}`",
               self.rustup_toolchain(), self.rustup_toolchain());
    }
    Ok(())
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
    !self.is_present("no-rustup")
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
  fn jobs(&self) -> Option<&str> {
    self.value_of("jobs")
  }

  fn rust_dbg_info_lvl(&self) -> &str {
    self.value_of("rust-dbg-lvl").unwrap()
  }
  fn llvm_build_dbg_info(&self) -> bool {
    self.is_present("llvm-debug-info")
  }
  fn llvm_enable_assertions(&self) -> bool {
    self.is_present("llvm-assertions")
  }
}

fn run_unlogged_cmd(task: &str, mut cmd: Command) {
  println!("({}): Running: {:?}", task, cmd);
  let mut child = cmd.spawn().unwrap();
  assert!(child.wait().unwrap().success(), "{:?}", cmd);
}
