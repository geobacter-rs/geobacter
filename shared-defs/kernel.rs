use std::borrow::Borrow;
use std::fmt;
use std::cmp::*;
use std::hash::*;

/// roughly corresponds to a `ty::Instance` in `rustc`.
#[derive(Clone, Copy)]
pub struct KernelInstanceRef<'a> {
  /// A debug friendly name
  #[doc(hidden)]
  pub name: &'a str,
  /// The serialized `ty::Instance<'tcx>`.
  #[doc(hidden)]
  pub instance: &'a [u8],
}

/// Split due to conflicting Borrow impls. Only reason.
/// This must have the exact same layout as KernelInstanceRef above.
#[derive(Clone, Copy)]
pub struct KernelInstance {
  /// A debug friendly name
  #[doc(hidden)]
  pub name: &'static str,
  /// The serialized `ty::Instance<'tcx>`.
  #[doc(hidden)]
  pub instance: &'static [u8],
}

impl<'a> KernelInstanceRef<'a> { }
impl KernelInstance { }

impl<'a> fmt::Debug for KernelInstanceRef<'a> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.debug_tuple("KernelInstance")
      .field(&self.name)
      .finish()
  }
}
impl<'a> Eq for KernelInstanceRef<'a> { }
impl<'a> Ord for KernelInstanceRef<'a> {
  fn cmp(&self, rhs: &Self) -> Ordering {
    self.instance.cmp(rhs.instance)
  }
}
impl<'a> PartialEq for KernelInstanceRef<'a> {
  fn eq(&self, rhs: &Self) -> bool {
    self.instance.eq(rhs.instance)
  }
}
impl<'a> PartialEq<KernelInstance> for KernelInstanceRef<'a> {
  fn eq(&self, rhs: &KernelInstance) -> bool {
    self.instance.eq(rhs.instance)
  }
}
impl<'a> PartialOrd for KernelInstanceRef<'a> {
  fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> {
    self.instance.partial_cmp(&rhs.instance)
  }
}
impl<'a> PartialOrd<KernelInstance> for KernelInstanceRef<'a> {
  fn partial_cmp(&self, rhs: &KernelInstance) -> Option<Ordering> {
    self.instance.partial_cmp(&rhs.instance)
  }
}
impl<'a> Hash for KernelInstanceRef<'a> {
  fn hash<H>(&self, hasher: &mut H)
    where H: Hasher,
  {
    self.instance.hash(hasher)
  }
}
impl<'a> Borrow<KernelInstanceRef<'a>> for KernelInstance {
  fn borrow(&self) -> &KernelInstanceRef<'a> {
    unsafe { ::std::mem::transmute(self) }
  }
}
impl<'a> KernelDesc for KernelInstanceRef<'a> {
  fn as_ref(&self) -> &KernelInstanceRef {
    self
  }
}

impl fmt::Debug for KernelInstance {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.debug_tuple("KernelInstance")
      .field(&self.name)
      .finish()
  }
}
impl Eq for KernelInstance { }
impl Ord for KernelInstance {
  fn cmp(&self, rhs: &Self) -> Ordering {
    self.instance.cmp(rhs.instance)
  }
}
impl PartialEq for KernelInstance {
  fn eq(&self, rhs: &Self) -> bool {
    self.instance.eq(rhs.instance)
  }
}
impl<'a> PartialEq<KernelInstanceRef<'a>> for KernelInstance {
  fn eq(&self, rhs: &KernelInstanceRef<'a>) -> bool {
    self.instance.eq(rhs.instance)
  }
}
impl PartialOrd for KernelInstance {
  fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> {
    self.instance.partial_cmp(&rhs.instance)
  }
}
impl<'a> PartialOrd<KernelInstanceRef<'a>> for KernelInstance {
  fn partial_cmp(&self, rhs: &KernelInstanceRef<'a>) -> Option<Ordering> {
    self.instance.partial_cmp(&rhs.instance)
  }
}
impl Hash for KernelInstance {
  fn hash<H>(&self, hasher: &mut H)
    where H: Hasher,
  {
    self.instance.hash(hasher)
  }
}
impl KernelDesc for KernelInstance {
  fn as_ref(&self) -> &KernelInstanceRef {
    unsafe { ::std::mem::transmute(self) }
  }
}

pub trait KernelDesc {
  fn as_ref(&self) -> &KernelInstanceRef;
  /// At some point in the future, we may not always create this const data
  fn name(&self) -> Option<&str> {
    if self.as_ref().name.len() != 0 {
      Some(self.as_ref().name)
    } else {
      None
    }
  }
  #[doc(hidden)]
  fn data(&self) -> &[u8] { self.as_ref().instance }
}
