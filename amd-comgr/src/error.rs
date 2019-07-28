
use std::error;
use std::fmt;

use sys;

#[derive(Debug)]
pub enum Error {
  Generic,
  InvalidArgument,
  OutOfResources,
}

impl Error {
  pub(crate) fn check(status: sys::amd_comgr_status_t) -> Result<(), Self> {
    match status {
      sys::AMD_COMGR_STATUS_SUCCESS => Ok(()),
      sys::AMD_COMGR_STATUS_ERROR => Err(Error::Generic),
      sys::AMD_COMGR_STATUS_ERROR_INVALID_ARGUMENT => {
        Err(Error::InvalidArgument)
      },
      sys::AMD_COMGR_STATUS_ERROR_OUT_OF_RESOURCES => {
        Err(Error::OutOfResources)
      },
      _ => Err(Error::Generic),
    }
  }
}
impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self)
  }
}
impl error::Error for Error { }
