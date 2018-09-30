
use agent::Agent;
use error::Error;

pub mod amd;

pub trait AsPtrValue {
  type Target;
  fn as_ptr_value(self) -> *const Self::Target;
}
impl<'a, T> AsPtrValue for &'a T {
  type Target = T;
  fn as_ptr_value(self) -> *const T { self as *const T }
}
impl<'a, T> AsPtrValue for &'a mut T {
  type Target = T;
  fn as_ptr_value(self) -> *const T { self as *mut T as *const T }
}

pub trait ByteLen {
  fn byte_len(&self) -> usize;
}
impl<T> ByteLen for T
  where T: Sized,
{
  fn byte_len(&self) -> usize {
    unimplemented!()
  }
}

pub enum HostLockedAgentPtr<T>
  where T: AsPtrValue,
{
  /// The agent uses the same address as the host:
  Shared(T),
  Agent {
    host: T,
    agent: T,
  },
}

impl<'ptr, T> HostLockedAgentPtr<&'ptr T> {
  pub fn as_host_ptr(&self) -> *const T {
    match self {
      &HostLockedAgentPtr::Shared(v) => v as *const T,
      &HostLockedAgentPtr::Agent { host, .. } => host as *const T,
    }
  }

}


/// Locks `self` in host memory, and gives access to the specified agents.
/// If `agents` as no elements, access will be given to everyone.
pub trait HostLockedAgentMemory: Sized + AsPtrValue {
  fn host_lock<'a>(self, agents: impl Iterator<Item = &'a Agent>)
    -> Result<HostLockedAgentPtr<Self>, Error>
  {
    unimplemented!();
  }
}
