
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

mod intrinsics {
  use super::KernelId;
  use traits::NumaSend;

  extern "rust-intrinsic" {

    pub fn kernel_id_for<'upvar, F, Args, Ret>(f: &'upvar F)
      -> (&'static str, u64, u64, u64)
      where F: Fn<Args, Output=Ret>;
    // note: only returns mut so that it isn't place in the
    // readonly section.
    pub fn kernel_context_data_id<'upvar, F, Args, Ret>(f: &'upvar F)
      -> &'static usize
      where F: Fn<Args, Output=Ret>;

    /*pub fn kernel_upvar<'upvar, F, Args, Ret>(f: &'upvar F,
                                              idx: usize)
      -> Option<&'upvar NumaSend>
      where F: Fn<Args, Output=Ret>;*/
  }
}

pub fn kernel_id_for<F, Args, Ret>(f: &F) -> KernelId
  where F: Fn<Args, Output=Ret>
{
  let def_id = unsafe {
    intrinsics::kernel_id_for(f)
  };

  unsafe {
    ::std::mem::transmute(def_id)
  }
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