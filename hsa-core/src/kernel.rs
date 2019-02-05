
use std::sync::atomic::AtomicUsize;

use traits::NumaSend;

// roughly corresponds to a DefId in `rustc`.
#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub struct KernelId {
  pub crate_name: &'static str,
  pub crate_hash_hi: u64,
  pub crate_hash_lo: u64,
  pub index: u64,
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
  use super::{KernelId, OptionalFn, };
  use traits::NumaSend;

  extern "rust-intrinsic" {

    pub fn kernel_id_for<'upvar, F, Args, Ret>(f: &'upvar F)
      -> &'static [(&'static str, u64, u64, u64)]
      where F: OptionalFn<Args, Output=Ret>;
    // note: only returns mut so that it isn't place in the
    // readonly section.
    pub fn kernel_context_data_id<'upvar, F, Args, Ret>(f: &'upvar F)
      -> &'static usize
      where F: Fn<Args, Output=Ret>;
  }
}

pub fn opt_kernel_id_for<F, Args, Ret>(f: &F) -> Option<KernelId>
  where F: OptionalFn<Args, Output=Ret>,
{
  let def_id = unsafe {
    intrinsics::kernel_id_for(f)
  };

  def_id.get(0)
    .map(|def_id| {
      KernelId {
        crate_name: def_id.0,
        crate_hash_hi: def_id.1,
        crate_hash_lo: def_id.2,
        index: def_id.3,
      }
    })
}
pub fn kernel_id_for<F, Args, Ret>(f: &F) -> KernelId
  where F: Fn<Args, Output=Ret> + OptionalFn<Args, Output=Ret>,
{
  opt_kernel_id_for(f).unwrap()
}
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
