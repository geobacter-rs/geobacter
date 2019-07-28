
use std::sync::atomic::AtomicUsize;

/// roughly corresponds to a DefId in `rustc`.
#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub struct KernelId {
  pub crate_name: &'static str,
  pub crate_hash_hi: u64,
  pub crate_hash_lo: u64,
  pub index: u64,
}
/// roughly corresponds to a `ty::Instance` in `rustc`.
#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub struct KernelInstance {
  pub kernel_id: KernelId,
  pub substs: &'static [u8],
}
impl KernelInstance {
  pub fn get_opt<F, Args, Ret>(f: &F) -> Option<Self>
    where F: OptionalFn<Args, Output=Ret>,
  {
    let def_id = unsafe {
      intrinsics::kernel_instance(f)
    };

    def_id.get(0)
      .map(|def_id| {
        let id = KernelId {
          crate_name: def_id.0,
          crate_hash_hi: def_id.1,
          crate_hash_lo: def_id.2,
          index: def_id.3,
        };

        KernelInstance {
          kernel_id: id,
          substs: def_id.4,
        }
      })
  }
  pub fn get<F, Args, Ret>(f: &F) -> Self
    where F: Fn<Args, Output=Ret> + OptionalFn<Args, Output=Ret>,
  {
    Self::get_opt(f).unwrap()
  }
}

pub trait OptionalFn<Args> {
  type Output;
  fn call_optionally(&self, args: Args) -> Self::Output;

  fn is_some(&self) -> bool;
  fn is_none(&self) -> bool { !self.is_some() }
}
impl OptionalFn<()> for () {
  type Output = ();
  fn call_optionally(&self, _args: ()) -> Self::Output {
    ()
  }

  fn is_some(&self) -> bool { false }
}
impl<F, Args> OptionalFn<Args> for F
  where F: Fn<Args>,
{
  type Output = <F as FnOnce<Args>>::Output;
  fn call_optionally(&self, args: Args) -> <F as FnOnce<Args>>::Output {
    self.call(args)
  }

  fn is_some(&self) -> bool { true }
}

mod intrinsics {
  use super::{OptionalFn, };

  extern "rust-intrinsic" {
    pub fn kernel_instance<'upvar, F, Args, Ret>(f: &'upvar F)
      -> &'static [(&'static str, u64, u64, u64, &'static [u8])]
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
