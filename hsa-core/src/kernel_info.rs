
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

    pub fn kernel_upvars<'upvar, F, Args, Ret>(f: &'upvar F)
      -> &'upvar [&'upvar NumaSend]
      where F: Fn<Args, Output=Ret>;

    // Returns global static crate kernels structure. This structure
    // is stored in every crate, under an implementation defined name.
    //pub fn crate_kernels() -> &'static StaticCrateKernels;
  }
}

#[derive(Debug, Copy, Clone)]
pub struct KernelInfo<'upvar> {
  pub id: KernelId,
  pub upvars: &'upvar [&'upvar NumaSend],
}

pub fn kernel_info_for<'upvar, F, Args, Ret>(f: &'upvar F) -> KernelInfo<'upvar>
  where F: Fn<Args, Output=Ret>
{
  let (crate_name, crate_hash_hi,
    crate_hash_lo, index) = unsafe {
    intrinsics::kernel_id_for(f)
  };

  const UPVARS: &'static [&'static NumaSend] = &[];

  let id = KernelId {
    crate_name,
    crate_hash_hi,
    crate_hash_lo,
    index,
  };

  KernelInfo {
    id,
    upvars: UPVARS,
  }
}

