
// For some reason, the HSA error enums trigger warnings here.
#![allow(non_upper_case_globals)]

use std::fmt;
use std::result::Result;

use ffi::hsa_status_t;


#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum Error {
  General,
  Exception,
  FrozenExecutable,
  IncompatibleArguments,
  InvalidAgent,
  InvalidAllocation,
  InvalidArgument,
  InvalidCache,
  InvalidCodeObject,
  InvalidCodeObjectReader,
  InvalidCodeSymbol,
  InvalidExecutable,
  InvalidExecutableSymbol,
  InvalidFile,
  InvalidIndex,
  InvalidIsa,
  InvalidIsaName,
  InvalidPacketFormat,
  InvalidQueue,
  InvalidQueueCreation,
  InvalidRegion,
  InvalidRuntimeState,
  InvalidSignal,
  InvalidSignalGroup,
  InvalidSymbolName,
  InvalidWavefront,
  NotInitialized,
  OutOfResources,
  Overflow,
  Underflow,
  ResourceFree,
  VariableAlreadyDefined,
  VariableUndefined,
  ImageFormatUnsupported,
  ImageSizeUnsupported,
  ImagePitchUnsupported,
  SamplerDescriptorUnsupported,
}

impl Error {
  pub fn from_status(s: hsa_status_t) -> Result<(), Error> {
    use self::Error::*;
    use ffi::*;

    let e = match s {
      hsa_status_t_HSA_STATUS_SUCCESS => { return Ok(()); }
      hsa_status_t_HSA_STATUS_ERROR => General,
      hsa_status_t_HSA_STATUS_ERROR_EXCEPTION => Exception,
      hsa_status_t_HSA_STATUS_ERROR_INCOMPATIBLE_ARGUMENTS => IncompatibleArguments,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_AGENT => InvalidAgent,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_ALLOCATION => InvalidAllocation,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_ARGUMENT => InvalidArgument,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_CACHE => InvalidCache,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_CODE_OBJECT => InvalidCodeObject,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_CODE_OBJECT_READER => InvalidCodeObjectReader,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_CODE_SYMBOL => InvalidCodeSymbol,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_EXECUTABLE => InvalidExecutable,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_EXECUTABLE_SYMBOL => InvalidExecutableSymbol,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_FILE => InvalidFile,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_INDEX => InvalidIndex,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_ISA => InvalidIsa,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_ISA_NAME => InvalidIsaName,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_PACKET_FORMAT => InvalidPacketFormat,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_QUEUE => InvalidQueue,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_QUEUE_CREATION => InvalidQueueCreation,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_REGION => InvalidRegion,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_RUNTIME_STATE => InvalidRuntimeState,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_SIGNAL => InvalidSignal,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_SIGNAL_GROUP => InvalidSignalGroup,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_SYMBOL_NAME => InvalidSymbolName,
      hsa_status_t_HSA_STATUS_ERROR_INVALID_WAVEFRONT => InvalidWavefront,
      hsa_status_t_HSA_STATUS_ERROR_NOT_INITIALIZED => NotInitialized,
      hsa_status_t_HSA_STATUS_ERROR_OUT_OF_RESOURCES => InvalidFile,
      hsa_status_t_HSA_STATUS_ERROR_REFCOUNT_OVERFLOW => OutOfResources,
      hsa_status_t_HSA_STATUS_ERROR_RESOURCE_FREE => ResourceFree,
      hsa_status_t_HSA_STATUS_ERROR_VARIABLE_ALREADY_DEFINED => VariableAlreadyDefined,
      hsa_status_t_HSA_STATUS_ERROR_VARIABLE_UNDEFINED => VariableUndefined,
      HSA_EXT_STATUS_ERROR_IMAGE_SIZE_UNSUPPORTED => ImageSizeUnsupported,
      HSA_EXT_STATUS_ERROR_IMAGE_PITCH_UNSUPPORTED => ImagePitchUnsupported,
      HSA_EXT_STATUS_ERROR_IMAGE_FORMAT_UNSUPPORTED => ImageFormatUnsupported,
      HSA_EXT_STATUS_ERROR_SAMPLER_DESCRIPTOR_UNSUPPORTED => SamplerDescriptorUnsupported,
      _ => General,
    };

    Err(e)
  }
}
impl From<::std::string::FromUtf8Error> for Error {
  fn from(_: ::std::string::FromUtf8Error) -> Self {
    Error::General
  }
}
impl From<::std::str::Utf8Error> for Error {
  fn from(_: ::std::str::Utf8Error) -> Self {
    Error::General
  }
}
impl From<std::num::TryFromIntError> for Error {
  fn from(_: std::num::TryFromIntError) -> Self {
    Error::Overflow
  }
}
impl ::std::error::Error for Error { }
impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self)
  }
}
#[cfg(feature = "alloc-wg")]
impl From<alloc_wg::alloc::LayoutErr> for Error {
  fn from(_: alloc_wg::alloc::LayoutErr) -> Error {
    Error::Overflow
  }
}
