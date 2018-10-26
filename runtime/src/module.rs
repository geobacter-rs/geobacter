
use std::marker::PhantomData;

use hsa_core::traits::NumaSend;

use context::Context;

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
    unimplemented!()
  }
}
