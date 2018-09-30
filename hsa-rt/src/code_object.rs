
use std::error::Error;
use std::mem::transmute;

use ffi;
use ApiContext;

pub trait CodeObjectReader {
  #[doc(hidden)]
  fn sys(&self) -> &CodeObjectReaderSys;
}

#[doc(hidden)]
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CodeObjectReaderSys(pub(crate) ffi::hsa_code_object_reader_t, ApiContext);

impl Drop for CodeObjectReaderSys {
  fn drop(&mut self) {
    unsafe {
      ffi::hsa_code_object_reader_destroy(self.0);
    }
  }
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct CodeObjectReaderRef<'a> {
  sys: CodeObjectReaderSys,
  buf: &'a [u8],
}

impl<'a> CodeObjectReaderRef<'a> {
  pub fn create(from: &'a [u8]) -> Result<Self, Box<Error>> {
    let mut out = CodeObjectReaderSys(ffi::hsa_code_object_reader_s {
      handle: 0,
    }, ApiContext::upref());
    let sys = check_err!(ffi::hsa_code_object_reader_create_from_memory(from.as_ptr() as _,
                                                                        from.len(),
                                                                        transmute(&mut out.0)) => out)?;
    Ok(CodeObjectReaderRef {
      sys,
      buf: from,
    })
  }
}
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct CodeObjectReaderOwned {
  sys: CodeObjectReaderSys,
  buf: Vec<u8>,
}
impl CodeObjectReaderOwned {
  pub fn create(from: Vec<u8>) -> Result<Self, Box<Error>> {
    let mut out = CodeObjectReaderSys(ffi::hsa_code_object_reader_s {
      handle: 0,
    }, ApiContext::upref());
    let sys = check_err!(ffi::hsa_code_object_reader_create_from_memory(from.as_ptr() as _,
                                                                        from.len(),
                                                                        transmute(&mut out.0)) => out)?;
    Ok(CodeObjectReaderOwned {
      sys,
      buf: from,
    })
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct LoadedCodeObject(pub(crate) ffi::hsa_loaded_code_object_t);
