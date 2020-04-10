
#![allow(dead_code)]

use std::collections::{HashMap as StdHashMap, HashSet as StdHashSet};
use std::fs::create_dir_all;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use seahash::SeaHasher;

pub mod env;
#[cfg(test)]
pub mod test;

pub type HashMap<K, V> = StdHashMap<K, V, BuildHasherDefault<SeaHasher>>;
pub type HashSet<K> = StdHashSet<K, BuildHasherDefault<SeaHasher>>;

/// HashSet doesn't implement `Default` w/ custom hashers, for some reason.
pub fn new_hash_set<K>() -> HashSet<K>
  where K: Eq + Hash,
{
  HashSet::with_hasher(BuildHasherDefault::default())
}
pub fn new_hash_map<K, V>() -> HashMap<K, V>
  where K: Eq + Hash,
{
  HashMap::with_hasher(BuildHasherDefault::default())
}

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
    let mut hasher = SeaHasher::new();
    Hash::hash(self, &mut hasher);
    hasher.finish()
  }
}
impl<T> StableHash for T
  where T: Hash,
{ }
