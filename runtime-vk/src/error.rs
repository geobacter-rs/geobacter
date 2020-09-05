
use std::error::Error as StdError;
use std::fmt;
use std::geobacter::kernel::KernelInstanceRef;
use std::io::Error as IoError;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
  Generic(Box<dyn StdError + Send + Sync + 'static>),
  LoadRustcMetadata(Box<dyn StdError + Send + Sync + 'static>),
  Io(IoError),
  Cmd(String),
  ConvertKernelInstance(KernelInstanceRef<'static>),
  ContextDead,
  Codegen,
  Linking,
  CodegenInitRoot(Box<Error>),
  CodegenInitConditions(Box<Error>),
  CodegenPreCodegen(Box<Error>),
  CodegenPostCodegen(Box<Error>),
  MissingSpirVObject,
  OutOfHostMemory,
  OutOfDeviceMemory,
  MissingRequiredFeature,
}
impl StdError for Error {
  fn source(&self) -> Option<&(dyn StdError + 'static)> {
    match self {
      Error::Generic(ref inner) => Some(&**inner),
      Error::Io(inner) => Some(inner),
      Error::CodegenInitConditions(inner) |
      Error::CodegenInitRoot(inner) |
      Error::CodegenPostCodegen(inner) |
      Error::CodegenPreCodegen(inner) => Some(inner),
      _ => None,
    }
  }
}
impl From<IoError> for Error {
  #[inline(always)]
  fn from(v: IoError) -> Self {
    Error::Io(v)
  }
}
impl From<vk::OomError> for Error {
  #[inline(always)]
  fn from(v: vk::OomError) -> Self {
    match v {
      vk::OomError::OutOfHostMemory => Error::OutOfHostMemory,
      vk::OomError::OutOfDeviceMemory => Error::OutOfDeviceMemory,
    }
  }
}
impl From<grt_core::codegen::error::Error<Error>> for Error {
  #[inline(always)]
  fn from(v: grt_core::codegen::error::Error<Error>) -> Self {
    use grt_core::codegen::error::Error::*;

    match v {
      Io(_, err) => Error::Io(err),
      LoadMetadata(err) => Error::LoadRustcMetadata(err),
      ConvertKernelInstance(ki) => Error::ConvertKernelInstance(ki),
      Codegen => Error::Codegen,
      Linking => Error::Linking,
      InitRoot(inner) => Error::CodegenInitRoot(Box::new(inner)),
      InitConditions(inner) => Error::CodegenInitConditions(Box::new(inner)),
      PreCodegen(inner) => Error::CodegenPreCodegen(Box::new(inner)),
      PostCodegen(inner) => Error::CodegenPostCodegen(Box::new(inner)),
      ContextDead => Error::ContextDead,
    }
  }
}
impl From<Box<dyn StdError + Send + Sync + 'static>> for Error {
  fn from(v: Box<dyn StdError + Send + Sync + 'static>) -> Self {
    v.downcast()
      .map(|v| *v )
      .or_else(|v| {
        v.downcast()
          .map(|v: Box<IoError>| Error::Io(*v) )
      })
      .unwrap_or_else(Error::Generic)
  }
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self)
  }
}
