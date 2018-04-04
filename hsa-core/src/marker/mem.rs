
/*use std::mem;

#[hsa_lang_item = "global_mem"]
pub struct Global<T>(pub T)
  where T: ?Sized;

impl<T> Global<T> {

}

#[hsa_lang_item = "const_mem"]
pub struct Constant<T>(pub &'static T);

#[hsa_lang_item = "addr_space"]
pub trait AddressSpace {
  const Id: u32;
}
impl<T> AddressSpace for Global<T> {
  const Id: u32 = 1;
}

pub trait Mem<'r, T> {
  fn byte_size(&self) -> usize;
}
pub trait MutMem<'r, T>: Mem<'r, T> {}
impl<'r, T> Mem<'r, T> for &'r Global<T>
  where T: ?Sized,
{
  fn byte_size(&self) -> usize {
    mem::size_of::<T>()
  }
}



#[hsa_lang_item = "local_mem_ref"]
pub struct LocalRef<'r, T, U>(U, T)
  where U: SourceLoc<'r, T>;
impl<'r, T> SourceLoc<'r, T> for &'

#[hsa_lang_item = "private_mem"]
pub struct PrivateRef<T>(T);


*/