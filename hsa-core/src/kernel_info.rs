
use traits::NumaSend;

pub type KernelId = u64;

mod intrinsics {
  use super::KernelId;
  use traits::NumaSend;
  extern "rust-intrinsic" {
    pub fn json_kernel_info_for<'upvar, F, Args, Ret>(f: &'upvar F) -> (KernelId, &'static str)
                                                                        //&'upvar [&'upvar NumaSend])
      where F: Fn<Args, Output=Ret>;

    // Returns global static crate kernels structure. This structure
    // is stored in every crate, under an implementation defined name.
    //pub fn crate_kernels() -> &'static StaticCrateKernels;
  }
}

#[derive(Debug, Copy, Clone)]
pub struct KernelInfo<'upvar, T> {
  pub id: KernelId,
  pub info: T,
  pub upvars: &'upvar [&'upvar NumaSend],
}

pub fn json_kernel_info_for<'upvar, F, Args, Ret>(f: &'upvar F) -> KernelInfo<'upvar, &'static str>
  where F: Fn<Args, Output=Ret>
{
  let (id, str) = unsafe {
    intrinsics::json_kernel_info_for(f)
  };

  const UPVARS: &'static [&'static NumaSend] = &[];

  KernelInfo {
    id,
    info: str,
    upvars: UPVARS,
  }
}

