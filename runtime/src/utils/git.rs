
use super::run_cmd;

use git2;

use std::error::Error;
use std::fs::remove_dir_all;
use std::path::{Path, PathBuf};
use std::process::{Command};

pub fn checkout_repo(dest: &Path, repo_url: &str,
                     branch: &str, thin: bool)
  -> Result<(), Box<Error>>
{
  let mut cmd = Command::new("git");
  let mut clone = false;
  if !dest.exists() {
    clone = true
  } else {
    loop {
      let repo = git2::Repository::open(dest);
      if repo.is_err() {
        remove_dir_all(dest)?;
        clone = true;
      }
      let repo = repo?;

      let _ = if let Ok(remote) = repo.find_remote("origin") {
        if remote.url() != Some(repo_url) {
          repo.remote_set_url("origin", repo_url)?;
          repo.find_remote("origin")?
        } else {
          remote
        }
      } else {
        repo.remote("origin", repo_url)?
      };

      break;
    }
  };

  if clone {
    cmd.arg("clone")
      .arg(repo_url);
    if thin {
      cmd.arg("--depth").arg("1");
    }
    cmd
      .arg("--single-branch")
      .arg("-b").arg(branch)
      .arg(dest);
  } else {
    cmd.current_dir(dest)
      .arg("fetch")
      .arg("--no-tags");
    if thin {
      cmd.arg("--depth").arg("1");
    }
    cmd
      .arg("origin")
      .arg(branch);

    run_cmd(cmd)?;

    cmd = Command::new("git");
    cmd.current_dir(dest)
      .arg("checkout")
      .arg("-B")
      .arg(branch);
    run_cmd(cmd)?;

    cmd = Command::new("git");
    cmd.current_dir(dest)
      .arg("reset")
      .arg("--hard")
      .arg("FETCH_HEAD");
  }
  run_cmd(cmd)?;

  let mut cmd = Command::new("git");
  cmd.current_dir(dest)
    .arg("submodule")
    .arg("update")
    .arg("--init")
    .arg("--jobs")
    .arg("4");
  run_cmd(cmd)?;

  Ok(())
}

pub fn checkout_or_override(name: &str,
                            dest_path: &Path,
                            over: Option<&PathBuf>,
                            repo_url: &str,
                            repo_branch: &str,
                            thin: bool)
                            -> Result<(), Box<Error>>
{
  let repo_url = if let Some(dir) = over {
    if dir == dest_path {
      return Ok(());
    }
    dir.to_str().expect("non-utf8 in path")
  } else {
    repo_url
  };

  checkout_repo(&dest_path, repo_url,
                repo_branch, thin)?;

  Ok(())
}

pub fn checkout_repo_commit(dest: &Path,
                            repo_url: &str, commit: &str)
  -> Result<(), Box<Error>>
{
  let mut cmd = Command::new("git");
  let mut clone = false;
  if !dest.exists() {
    clone = true
  } else {
    loop {
      let repo = git2::Repository::open(dest);
      if repo.is_err() {
        remove_dir_all(dest)?;
        clone = true;
      }
      let repo = repo?;

      let _ = if let Ok(remote) = repo.find_remote("origin") {
        if remote.url() != Some(repo_url) {
          repo.remote_set_url("origin", repo_url)?;
          repo.find_remote("origin")?
        } else {
          remote
        }
      } else {
        repo.remote("origin", repo_url)?
      };

      break;
    }
  };

  if clone {
    cmd.arg("clone")
      .arg(repo_url)
      .arg(dest);

    run_cmd(cmd)?;
    cmd = Command::new("git");
    cmd.current_dir(dest)
      .arg("fetch")
      .arg("--all");
    run_cmd(cmd)?;

    cmd = Command::new("git");
    cmd.current_dir(dest)
      .arg("reset")
      .arg("--hard")
      .arg(commit);
  } else {
    cmd.current_dir(dest)
      .arg("fetch")
      .arg("--no-tags")
      .arg("origin");
    run_cmd(cmd)?;

    cmd = Command::new("git");
    cmd.current_dir(dest)
      .arg("reset")
      .arg("--hard")
      .arg(commit);
  }
  run_cmd(cmd)?;

  let mut cmd = Command::new("git");
  cmd.current_dir(dest)
    .arg("submodule")
    .arg("update")
    .arg("--init");
  run_cmd(cmd)?;

  Ok(())
}
