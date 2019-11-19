
use std::fmt;
use std::hash::*;
use std::ops::*;
use std::sync::atomic::AtomicUsize;

use shared_defs::kernel::KernelDesc;

/// roughly corresponds to a `ty::Instance` in `rustc`.
#[derive(Clone, Copy)]
pub struct KernelInstance {
  /// A debug friendly name
  name: &'static str,
  /// The serialized `ty::Instance<'tcx>`.
  pub instance: &'static [u8],
}
impl KernelInstance {
  pub fn get_opt<F, Args, Ret>(f: &F) -> Option<Self>
    where F: OptionalFn<Args, Output=Ret>,
  {
    let instance = unsafe {
      intrinsics::kernel_instance(f)
    };

    instance.get(0)
      .map(|(name, instance)| {
        KernelInstance {
          name,
          instance,
        }
      })
  }
  pub fn get<F, Args, Ret>(f: &F) -> Self
    where F: Fn<Args, Output=Ret> + OptionalFn<Args, Output=Ret>,
  {
    Self::get_opt(f).unwrap()
  }

  /// At some point in the future, we may not always create this const data
  pub fn name(&self) -> Option<&'static str> {
    if self.name.len() != 0 {
      Some(self.name)
    } else {
      None
    }
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
impl PartialEq for KernelInstance {
  fn eq(&self, rhs: &Self) -> bool {
    self.instance.eq(rhs.instance)
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
  fn instance_name(&self) -> Option<&str> { self.name() }
  fn instance_data(&self) -> &[u8] { self.instance }
}

pub trait OptionalFn<Args> {
  type Output;
  fn call_optionally(&self, args: Args) -> Self::Output;

  fn is_some(&self) -> bool;
  fn is_none(&self) -> bool { !self.is_some() }

  fn kernel_instance(&self) -> Option<KernelInstance>;
}
impl OptionalFn<()> for () {
  type Output = ();
  fn call_optionally(&self, _args: ()) -> Self::Output {
    ()
  }

  fn is_some(&self) -> bool { false }

  fn kernel_instance(&self) -> Option<KernelInstance> { None }
}
impl<F, Args> OptionalFn<Args> for F
  where F: Fn<Args>,
{
  type Output = <F as FnOnce<Args>>::Output;
  fn call_optionally(&self, args: Args) -> <F as FnOnce<Args>>::Output {
    self.call(args)
  }

  fn is_some(&self) -> bool { true }

  fn kernel_instance(&self) -> Option<KernelInstance> {
    KernelInstance::get_opt(self)
  }
}

mod intrinsics {
  use super::{OptionalFn, };

  extern "rust-intrinsic" {
    pub fn kernel_instance<'upvar, F, Args, Ret>(f: &'upvar F)
      -> &'static [(&'static str, &'static [u8])]
      where F: OptionalFn<Args, Output=Ret>;
    pub fn kernel_context_data_id<'upvar, F, Args, Ret>(f: &'upvar F)
      -> &'static usize
      where F: Fn<Args, Output=Ret>;
  }
}

/// Internal use only!
/// Used for storing codegen-ed kernels without having to lookup a
/// `KernelId` in a map
#[doc = "hidden"]
pub fn kernel_context_data_id<F, Args, Ret>(f: &F) -> &'static AtomicUsize
  where F: Fn<Args, Output=Ret>
{
  let addr = unsafe {
    intrinsics::kernel_context_data_id(f)
  };
  unsafe {
    ::std::mem::transmute(addr)
  }
}
