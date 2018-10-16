
use std::error::Error as StdError;
use std::{fmt, io, };

use hsa_core::kernel::KernelId;

#[derive(Debug)]
pub enum Error {
  Io(KernelId, io::Error),
  NoCrateMetadata(KernelId),
  Codegen(KernelId),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self)
  }
}

impl StdError for Error { }

pub trait IntoErrorWithKernelId {
  type Output;
  fn with_kernel_id(self, id: KernelId) -> Self::Output;
}
impl<T> IntoErrorWithKernelId for Result<T, io::Error> {
  type Output = Result<T, Error>;
  fn with_kernel_id(self, id: KernelId) -> Self::Output {
    self.map_err(move |e| Error::Io(id, e) )
  }
}
