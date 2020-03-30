
use std::error::Error as StdError;
use std::{fmt, io, };

use geobacter_core::kernel::KernelInstance;

use crate::codegen::PlatformCodegen;

#[derive(Debug)]
pub enum Error<E> {
  Io(Option<KernelInstance>, io::Error),
  LoadMetadata(Box<dyn StdError + Send + Sync + 'static>),
  ConvertKernelInstance(KernelInstance),
  Codegen,
  Linking,
  InitRoot(E),
  InitConditions(E),
  PreCodegen(E),
  PostCodegen(E),
  ContextDead,
}
pub type PError<P> = Error<<<P as PlatformCodegen>::Device as crate::Device>::Error>;

impl<E> fmt::Display for Error<E>
  where E: fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self)
  }
}

impl<E> StdError for Error<E>
  where E: StdError + 'static,
{
  fn source(&self) -> Option<&(dyn StdError + 'static)> {
    match self {
      Error::Io(_, inner) => Some(inner),
      Error::InitRoot(inner) |
      Error::InitConditions(inner) |
      Error::PreCodegen(inner) |
      Error::PostCodegen(inner) => Some(inner),
      _ => None,
    }
  }
}

impl<E> From<io::Error> for Error<E> {
  fn from(v: io::Error) -> Error<E> {
    Error::Io(None, v)
  }
}

pub trait IntoErrorWithKernelInstance<E> {
  type Output;
  fn with_kernel_instance(self, id: KernelInstance) -> Self::Output;
}
impl<E, T> IntoErrorWithKernelInstance<E> for Result<T, io::Error> {
  type Output = Result<T, Error<E>>;
  fn with_kernel_instance(self, id: KernelInstance) -> Self::Output {
    self.map_err(move |e| Error::Io(Some(id), e) )
  }
}
