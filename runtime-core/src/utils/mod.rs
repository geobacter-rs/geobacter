
use std::fs::{create_dir_all, };
use std::hash::{Hash, Hasher, };
use std::io;
use std::path::{Path, PathBuf};

pub use gintrinsics::hash::*;

pub mod env;

pub trait CreateIfNotExists: AsRef<Path> {
  fn create_if_not_exists(&self) -> io::Result<()> {
    let p = self.as_ref();
    if !p.exists() {
      create_dir_all(p)?;
    }

    Ok(())
  }
}

impl CreateIfNotExists for PathBuf { }
impl<'a> CreateIfNotExists for &'a Path { }

pub trait StableHash: Hash {
  fn stable_hash(&self) -> u64 {
    let mut hasher = crate::seahash::SeaHasher::new();
    Hash::hash(self, &mut hasher);
    hasher.finish()
  }
}
impl<T> StableHash for T
  where T: Hash,
{ }
