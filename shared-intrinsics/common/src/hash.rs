
use std::collections::{HashMap as StdHashMap, HashSet as StdHashSet, };
use std::hash::{BuildHasherDefault, Hash, };

use seahash::{SeaHasher, };

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
