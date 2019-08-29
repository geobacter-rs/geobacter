
use std::fmt;
use std::fs::{create_dir_all, };
use std::hash::{Hash, Hasher, };
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

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

/// DO NOT SEND ON THIS SENDER. Only send on a thread local
/// clone of the sender in this obj.
/// `Sender` doesn't implement `Sync`, but we want to avoid cross thread comms
/// just to get a local copy of the sender. So we force `Sync` here and require
/// that no attempts to send on the sender are made on the shared copy.
pub struct UnsafeSyncSender<T>(pub(crate) Sender<T>);
impl<T> UnsafeSyncSender<T> {
  /// Can't name this `clone_into`: the compiler will think we're using a
  /// new feature when we really aren't.
  pub fn clone_unsync<U>(&self) -> U
    where U: From<Self>,
  {
    U::from(self.clone())
  }
}
impl<T> Clone for UnsafeSyncSender<T> {
  fn clone(&self) -> Self {
    UnsafeSyncSender(self.0.clone())
  }
}
unsafe impl<T> Sync for UnsafeSyncSender<T> { }
impl<T> fmt::Debug for UnsafeSyncSender<T> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "<..>")
  }
}

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
