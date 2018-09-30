
use std::marker::PhantomData;
use serde_json;

use hsa_core;
use hsa_core::traits::NumaSend;

use context::Context;

pub struct ModuleData<'a, F, Args, Ret>
  where F: Fn<Args, Output = Ret> + 'a,
{
  id: u64,
  f: &'a F,
  upvars: &'a [&'a NumaSend],
  _marker: PhantomData<(Args, Ret)>,
}

impl<'a, F, Args, Ret> ModuleData<'a, F, Args, Ret>
  where F: Fn<Args, Output = Ret> + 'a,
{
  pub fn new(ctxt: Context, f: &'a F) -> Self {
    unimplemented!();
    /*
    let info = hsa_core::kernel_info::json_kernel_info_for(&f);
    let module = serde_json::from_str(info.info)
      .expect("internal error: compiletime/runtime info mismatch");

    ModuleData {
      id: info.id,
      f,
      ir: module,
      upvars: info.upvars,
      codegens: vec![],
      _marker: PhantomData,
    }*/
  }
}

pub trait ErasedModule {

}
