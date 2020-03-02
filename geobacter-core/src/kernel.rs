
use std::ops::*;
use std::sync::atomic::AtomicUsize;

pub use shared_defs::kernel::*;

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
    let instance = unsafe {
      intrinsics::kernel_instance(self)
    };

    instance.get(0)
      .map(|(name, instance)| {
        KernelInstance {
          name,
          instance,
        }
      })
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
