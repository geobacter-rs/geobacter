#![allow(dead_code)]

use super::{run_unlogged_cmd};

use git2::{Repository, };

// XXX technically, no function here even returns an error.
use std::error::Error;
use std::fs::remove_dir_all;
use std::path::{Path, PathBuf};
use std::process::{Command};

pub fn checkout_repo(task: &str, dest: &Path,
                     repo_url: &str, branch: &str,
                     thin: bool) {
  let mut cmd = Command::new("git");
  let mut clone = false;
  if !dest.exists() {
    clone = true
  } else {
    loop {
      let repo = Repository::open(dest);
      if repo.is_err() {
        remove_dir_all(dest).unwrap();
        clone = true;
      }
      let repo = repo.unwrap();

      let _ = if let Ok(remote) = repo.find_remote("origin") {
        if remote.url() != Some(repo_url) {
          repo.remote_set_url("origin", repo_url).unwrap();
          repo.find_remote("origin").unwrap()
        } else {
          remote
        }
      } else {
        repo.remote("origin", repo_url).unwrap()
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
    {
      // save changes so we don't overwrite on accident.
      let mut cmd = Command::new("git");
      cmd.current_dir(dest)
        .arg("stash");
      run_unlogged_cmd(task, cmd);
    }
    cmd.current_dir(dest)
      .arg("fetch")
      .arg("--no-tags");
    if thin {
      cmd.arg("--depth").arg("1");
    }
    cmd
      .arg("origin")
      .arg(branch);

    run_unlogged_cmd(task, cmd);

    cmd = Command::new("git");
    cmd.current_dir(dest)
      .arg("checkout")
      .arg("-B")
      .arg(branch);
    run_unlogged_cmd(task, cmd);

    cmd = Command::new("git");
    cmd.current_dir(dest)
      .arg("reset")
      .arg("--hard")
      .arg("FETCH_HEAD");
  }
  run_unlogged_cmd(task, cmd);

  let mut cmd = Command::new("git");
  cmd.current_dir(dest)
    .arg("submodule")
    .arg("update")
    .arg("--init");
  run_unlogged_cmd(task, cmd);
}

pub fn checkout_or_override(name: &str,
                            dest_path: &Path,
                            over: Option<&PathBuf>,
                            repo_url: &str,
                            repo_branch: &str,
                            thin: bool)
  -> Result<(), Box<dyn Error>>
{
  checkout_or_override_raw(name, dest_path,
                           over, repo_url, repo_branch,
                           thin, checkout_repo)
}
pub fn checkout_or_override_commit(name: &str,
                                   dest_path: &Path,
                                   over: Option<&PathBuf>,
                                   repo_url: &str,
                                   repo_commit: &str,
                                   thin: bool)
  -> Result<(), Box<dyn Error>>
{
  checkout_or_override_raw(name, dest_path,
                           over, repo_url, repo_commit,
                           thin, checkout_repo_commit)
}
pub fn checkout_or_override_raw<F>(name: &str,
                                   dest_path: &Path,
                                   over: Option<&PathBuf>,
                                   repo_url: &str,
                                   repo_branch: &str,
                                   thin: bool,
                                   checkout_repo: F)
  -> Result<(), Box<dyn Error>>
  where F: FnOnce(&str, &Path, &str, &str, bool)
{
  let repo_url = if let Some(dir) = over {
    if dir == dest_path {
      return Ok(());
    } else {
      println!("{} != {}", dir.display(),
               dest_path.display());
    }
    dir.to_str().expect("non-utf8 in path")
  } else {
    repo_url
  };

  let task = format!("checkout-{}", name);
  checkout_repo(&task[..], &dest_path,
                repo_url, repo_branch, thin);

  Ok(())
}

pub fn checkout_repo_commit(task: &str, dest: &Path,
                            repo_url: &str, commit: &str,
                            _thin: bool) {
  let mut cmd = Command::new("git");
  let mut clone = false;
  if !dest.exists() {
    clone = true
  } else {
    loop {
      let repo = Repository::open(dest);
      if repo.is_err() {
        remove_dir_all(dest).unwrap();
        clone = true;
      }
      let repo = repo.unwrap();

      let _ = if let Ok(remote) = repo.find_remote("origin") {
        if remote.url() != Some(repo_url) {
          repo.remote_set_url("origin", repo_url).unwrap();
          repo.find_remote("origin").unwrap()
        } else {
          remote
        }
      } else {
        repo.remote("origin", repo_url).unwrap()
      };

      break;
    }
  };

  if clone {
    cmd.arg("clone")
      .arg(repo_url)
      .arg(dest);

    run_unlogged_cmd(task, cmd);
    cmd = Command::new("git");
    cmd.current_dir(dest)
      .arg("fetch")
      .arg("--all");
    run_unlogged_cmd(task, cmd);

    cmd = Command::new("git");
    cmd.current_dir(dest)
      .arg("reset")
      .arg("--hard")
      .arg(commit);
  } else {
    {
      // save changes so we don't overwrite on accident.
      let mut cmd = Command::new("git");
      cmd.current_dir(dest)
        .arg("stash");
      run_unlogged_cmd(task, cmd);
    }
    cmd.current_dir(dest)
      .arg("fetch")
      .arg("--no-tags")
      .arg("origin");
    run_unlogged_cmd(task, cmd);

    cmd = Command::new("git");
    cmd.current_dir(dest)
      .arg("reset")
      .arg("--hard")
      .arg(commit);
  }
  run_unlogged_cmd(task, cmd);

  let mut cmd = Command::new("git");
  cmd.current_dir(dest)
    .arg("submodule")
    .arg("update")
    .arg("--init");
  run_unlogged_cmd(task, cmd);
}
