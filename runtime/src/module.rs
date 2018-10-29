
use std::marker::PhantomData;

use indexvec::Idx;

use hsa_core::traits::NumaSend;
use hsa_core::kernel::kernel_context_data_id;

use context::Context;

newtype_index!(FunctionId);

pub struct Function<'a, F, Args, Ret>
  where F: Fn<Args, Output = Ret> + 'a,
{
  id: u64,
  f: &'a F,
  _marker: PhantomData<(Args, Ret)>,
}

impl<'a, F, Args, Ret> Function<'a, F, Args, Ret>
  where F: Fn<Args, Output = Ret> + 'a,
{
  pub fn new(ctxt: Context, f: &'a F) -> Self {
    unimplemented!();
  }
}
