use std::fmt;
use std::result::Result;

use ffi::hsa_status_t;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum Error {
  General,
  Exception,
  FrozenExecutable,
  IncompatibleArguments,
  InvalidAgen,
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
  RefCountOverflow,
  ResourceFree,
  VariableAlreadyDefined,
  VariableUndefined,
}

impl Error {
  pub fn from_status(s: hsa_status_t) -> Result<(), Error> {
    use self::Error::*;
    use ffi::*;

    let e = match s {
      hsa_status_t_HSA_STATUS_SUCCESS => { return Ok(()); }
      hsa_status_t_HSA_STATUS_ERROR => General,
      hsa_status_t_HSA_STATUS_ERROR_EXCEPTION => Exception,
      _ => General,
    };

    Err(e)
  }
}
impl ::std::error::Error for Error {
  fn description(&self) -> &str {
    // TODO
    unimplemented!();
  }
}
impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self)
  }
}