use std::error::{Error as StdError, };
use std::ffi::CString;
use std::num::NonZeroU64;
use std::path::{PathBuf, };
use std::ptr;

use sys;

use crate::error::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum DataKind {
  Bitcode,
  Relocatable,
  Executable,
  Bytes,
  Log,
  Source,
}
impl DataKind {
  pub(crate) fn to_sys(&self) -> sys::amd_comgr_data_kind_t {
    match self {
      DataKind::Bitcode => sys::AMD_COMGR_DATA_KIND_BC,
      DataKind::Relocatable => sys::AMD_COMGR_DATA_KIND_RELOCATABLE,
      DataKind::Executable => sys::AMD_COMGR_DATA_KIND_EXECUTABLE,
      DataKind::Bytes => sys::AMD_COMGR_DATA_KIND_BYTES,
      DataKind::Log => sys::AMD_COMGR_DATA_KIND_LOG,
      DataKind::Source => sys::AMD_COMGR_DATA_KIND_SOURCE,
    }
  }
}

pub trait Data: From<DataHandle> {
  fn kind_dyn(&self) -> DataKind;
  fn kind() -> DataKind;

  #[doc(hidden)]
  fn handle(&self) -> &DataHandle;

  fn set_data(&mut self, data: &[u8]) -> Result<(), Error> {
    let data_len = data.len();
    let data_ptr = data.as_ptr();

    let h = self.handle().handle();

    let s = unsafe {
      sys::amd_comgr_set_data(h, data_len as _, data_ptr as *const _)
    };
    Error::check(s)
  }
  fn data(&self) -> Result<Vec<u8>, Error> {
    let len = self.len()?;

    let mut data = Vec::new();
    if len != 0 {
      data.resize(len, 0u8);

      self.copy_data_into(&mut data)?;
    }

    Ok(data)
  }

  fn len(&self) -> Result<usize, Error> {
    let h = self.handle().handle();
    let mut len = 0;
    let s = unsafe {
      sys::amd_comgr_get_data(h, &mut len,
                              ptr::null_mut())
    };
    Error::check(s)?;

    Ok(len as _)
  }
  fn copy_data_into(&self, into: &mut [u8]) -> Result<(), Error> {
    let h = self.handle().handle();
    let mut len = into.len() as _;
    let s = unsafe {
      sys::amd_comgr_get_data(h, &mut len,
                              into.as_mut_ptr() as *mut _)
    };
    Error::check(s)?;

    Ok(())
  }

  fn set_name(&mut self, mut name: String) -> Result<(), Error> {
    let h = self.handle().handle();

    name.push('\0');

    let name_ptr = name.as_ptr();
    let s = unsafe {
      sys::amd_comgr_set_data_name(h, name_ptr as *const _)
    };
    Error::check(s)
  }
  fn cname(&self) -> Result<CString, Box<dyn StdError + Send + Sync + 'static>> {
    let h = self.handle().handle();
    let mut len = 0;
    let s = unsafe {
      sys::amd_comgr_get_data_name(h, &mut len,
                                   ptr::null_mut())
    };
    Error::check(s)?;

    // ignore the null terminator:
    len -= 1;

    let mut data = Vec::new();
    if len != 0 {
      data.resize(len as _, 0u8);

      let s = unsafe {
        sys::amd_comgr_get_data_name(h, &mut len,
                                     data.as_mut_ptr() as *mut _)
      };
      Error::check(s)?;
    }

    Ok(CString::new(data)?)
  }
  fn name(&self) -> Result<String, Box<dyn StdError + Send + Sync + 'static>> {
    Ok(self.cname()?.into_string()?)
  }
  fn name_path(&self) -> Result<PathBuf, Box<dyn StdError + Send + Sync + 'static>> {
    Ok(self.name()?.into())
  }
}
#[doc(hidden)]
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DataHandle(pub(crate) NonZeroU64);
impl DataHandle {
  fn new(kind: DataKind) -> Result<Self, Error> {
    let kind = kind.to_sys();

    let mut out = sys::amd_comgr_data_s {
      handle: 0,
    };
    let s = unsafe {
      sys::amd_comgr_create_data(kind, &mut out as *mut _)
    };
    Error::check(s)?;

    debug_assert_ne!(out.handle, 0);

    unsafe {
      Ok(DataHandle(NonZeroU64::new_unchecked(out.handle)))
    }
  }
  pub(crate) fn handle(&self) -> sys::amd_comgr_data_s {
    sys::amd_comgr_data_s {
      handle: self.0.get(),
    }
  }
}
impl Drop for DataHandle {
  fn drop(&mut self) {
    // XXX return status unchecked
    unsafe {
      sys::amd_comgr_release_data(self.handle())
    };
  }
}
impl !Sync for DataHandle { }
impl !Send for DataHandle { }

macro_rules! specialized_datatype {
  ($tyname:ident, $kind:ident) => (

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct $tyname(DataHandle);
impl $tyname {
  pub fn new() -> Result<Self, Error> {
    let h = DataHandle::new(DataKind::$kind)?;
    Ok($tyname(h))
  }
}
impl Data for $tyname {
  fn kind_dyn(&self) -> DataKind {
    DataKind::$kind
  }
  fn kind() -> DataKind {
    DataKind::$kind
  }
  #[doc(hidden)]
  fn handle(&self) -> &DataHandle { &self.0 }
}
impl From<DataHandle> for $tyname {
  fn from(v: DataHandle) -> Self {
    $tyname(v)
  }
}

  );
}

specialized_datatype!(BitcodeData, Bitcode);
specialized_datatype!(RelocatableData, Relocatable);
specialized_datatype!(ExecutableData, Executable);
specialized_datatype!(ByteData, Bytes);
specialized_datatype!(LogData, Log);
specialized_datatype!(SourceData, Source);

macro_rules! isa_name_fn {
  ($ty:ty) => (

impl $ty {
  pub fn isa_name(&self) -> Result<String, Box<dyn StdError>> {
    let h = self.handle().handle();
    let mut len = 0;
    let s = unsafe {
      sys::amd_comgr_get_data_isa_name(h, &mut len,
                                       ptr::null_mut())
    };
    Error::check(s)?;

    // ignore the null terminator:
    len -= 1;

    let mut data = Vec::new();
    if len != 0 {
      data.resize(len as _, 0u8);

      let s = unsafe {
        sys::amd_comgr_get_data_isa_name(h, &mut len,
                                         data.as_mut_ptr() as *mut _)
      };
      Error::check(s)?;
    }

    Ok(String::from_utf8(data)?)
  }
}

  );
}
isa_name_fn!(RelocatableData);
isa_name_fn!(ExecutableData);

impl LogData {
  pub fn data_str(&self) -> Result<String, Box<dyn StdError>> {
    let data = self.data()?;
    Ok(String::from_utf8(data)?)
  }
}
impl SourceData {
  pub fn data_str(&self) -> Result<String, Box<dyn StdError>> {
    let data = self.data()?;
    Ok(String::from_utf8(data)?)
  }
}
