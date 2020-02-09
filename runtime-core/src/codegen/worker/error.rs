
use std::error::Error as StdError;
use std::{fmt, io, };

use geobacter_core::kernel::KernelInstance;

#[derive(Debug)]
pub enum Error {
  Io(Option<KernelInstance>, io::Error),
  NoCrateMetadata(KernelInstance),
  Codegen(KernelInstance),
  InitRoot(Box<dyn StdError + Send + Sync + 'static>),
  InitConditions(Box<dyn StdError + Send + Sync + 'static>),
  PreCodegen(Box<dyn StdError + Send + Sync + 'static>),
  PostCodegen(Box<dyn StdError + Send + Sync + 'static>),
  ContextDead,
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self)
  }
}

impl StdError for Error { }

impl From<io::Error> for Error {
  fn from(v: io::Error) -> Error {
    Error::Io(None, v)
  }
}

pub trait IntoErrorWithKernelInstance {
  type Output;
  fn with_kernel_instance(self, id: KernelInstance) -> Self::Output;
}
impl<T> IntoErrorWithKernelInstance for Result<T, io::Error> {
  type Output = Result<T, Error>;
  fn with_kernel_instance(self, id: KernelInstance) -> Self::Output {
    self.map_err(move |e| Error::Io(Some(id), e) )
  }
}
