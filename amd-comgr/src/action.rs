
use std::ffi::{CString, };
use std::num::NonZeroU64;
use std::ops::{Deref, DerefMut, };
use std::path::{PathBuf, };
use std::ptr;

use sys;

use crate::error::Error;

use self::ActionKind::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Lang {
  Hc,
  /// The inner u8 is the major version of OpenCL.
  OpenCl(u8),
}
impl Lang {
  fn to_sys(this: Option<Self>) -> Result<sys::amd_comgr_language_s, Error> {
    let l = match this {
      Some(Lang::Hc) => sys::AMD_COMGR_LANGUAGE_HC,
      Some(Lang::OpenCl(1)) => sys::AMD_COMGR_LANGUAGE_OPENCL_1_2,
      Some(Lang::OpenCl(2)) => sys::AMD_COMGR_LANGUAGE_OPENCL_2_0,
      Some(Lang::OpenCl(_)) => { return Err(Error::InvalidArgument); },
      None => sys::AMD_COMGR_LANGUAGE_NONE,
    };

    Ok(l)
  }
}

/// Note some actions present in `libamd-comgr` are not reflected
/// here.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ActionKind {
  AddDeviceLibs,
  LinkBcToBc,
  LinkRelocToExe,
  LinkRelocToReloc,
  OptimizeBcToBc,
  CodegenBcToAssembly,
  CodegenBcToReloc,
}
impl ActionKind {
  pub(crate) fn to_sys(&self) -> sys::amd_comgr_action_kind_s {
    match self {
      AddDeviceLibs => sys::AMD_COMGR_ACTION_ADD_DEVICE_LIBRARIES,
      LinkBcToBc => sys::AMD_COMGR_ACTION_LINK_BC_TO_BC,
      LinkRelocToExe => sys::AMD_COMGR_ACTION_LINK_RELOCATABLE_TO_EXECUTABLE,
      LinkRelocToReloc => sys::AMD_COMGR_ACTION_LINK_RELOCATABLE_TO_RELOCATABLE,
      OptimizeBcToBc => sys::AMD_COMGR_ACTION_OPTIMIZE_BC_TO_BC,
      CodegenBcToAssembly => sys::AMD_COMGR_ACTION_CODEGEN_BC_TO_ASSEMBLY,
      CodegenBcToReloc => sys::AMD_COMGR_ACTION_CODEGEN_BC_TO_RELOCATABLE,
    }
  }
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ActionInfo(NonZeroU64);
impl ActionInfo {
  pub fn new() -> Result<Self, Error> {
    let mut out = sys::amd_comgr_action_info_s {
      handle: 0,
    };

    let s = unsafe {
      sys::amd_comgr_create_action_info(&mut out as *mut _)
    };
    Error::check(s)?;

    assert_ne!(out.handle, 0);

    unsafe {
      Ok(ActionInfo(NonZeroU64::new_unchecked(out.handle)))
    }
  }

  pub fn get_isa_name(&self) -> Result<Option<String>, Error> {
    let h = self.handle();
    let mut len = 0usize;
    let s = unsafe {
      sys::amd_comgr_action_info_get_isa_name(h, &mut len as *mut _,
                                              ptr::null_mut())
    };
    Error::check(s)?;

    if len <= 1 { return Ok(None); }

    // ignore the null terminator:
    len -= 1;

    let mut data = Vec::new();
    data.resize(len, 0u8);

    let s = unsafe {
      sys::amd_comgr_action_info_get_isa_name(h, &mut len as *mut _,
                                              data.as_mut_ptr() as *mut _)
    };
    Error::check(s)?;

    // we only allow setting the name using a `String`,
    // so we don't have to check for utf8-ness here.
    let s = unsafe { String::from_utf8_unchecked(data) };
    Ok(Some(s))
  }
  /// See https://llvm.org/docs/AMDGPUUsage.html#code-object-target-identification
  pub fn set_isa_name(&mut self, name: Option<&str>) -> Result<(), Error> {
    let h = self.handle();
    let name = name.map(|n| CString::new(n).unwrap() );
    let name_ptr = name.as_ref()
      .map(|n| n.as_ptr() )
      .unwrap_or(ptr::null());

    let s = unsafe {
      sys::amd_comgr_action_info_set_isa_name(h, name_ptr)
    };
    Error::check(s)?;

    Ok(())
  }

  pub fn set_lang(&mut self, lang: Option<Lang>) -> Result<(), Error> {
    let h = self.handle();
    let lang = Lang::to_sys(lang)?;
    let s = unsafe {
      sys::amd_comgr_action_info_set_language(h, lang)
    };
    Error::check(s)?;
    Ok(())
  }

  pub fn get_logging(&self) -> Result<bool, Error> {
    let mut logging = false;
    let s = unsafe {
      sys::amd_comgr_action_info_get_logging(self.handle(),
                                             &mut logging as *mut _)
    };
    Error::check(s)?;
    Ok(logging)
  }
  pub fn set_logging(&mut self, logging: bool) -> Result<(), Error> {
    let s = unsafe {
      sys::amd_comgr_action_info_set_logging(self.handle(), logging)
    };
    Error::check(s)?;
    Ok(())
  }

  pub fn get_options(&self) -> Result<Option<String>, Error> {
    let h = self.handle();
    let mut len = 0usize;
    let s = unsafe {
      sys::amd_comgr_action_info_get_options(h, &mut len as *mut _,
                                             ptr::null_mut())
    };
    Error::check(s)?;

    if len <= 1 { return Ok(None); }

    // ignore the null terminator:
    len -= 1;

    let mut data = Vec::new();
    data.resize(len, 0u8);

    let s = unsafe {
      sys::amd_comgr_action_info_get_options(h, &mut len as *mut _,
                                             data.as_mut_ptr() as *mut _)
    };
    Error::check(s)?;

    // we only allow setting the name using a `String`,
    // so we don't have to check for utf8-ness here.
    let s = unsafe { String::from_utf8_unchecked(data) };
    Ok(Some(s))
  }
  pub fn set_options(&mut self, options: Option<&str>) -> Result<(), Error> {
    let h = self.handle();
    let options = options.map(|n| CString::new(n).unwrap() );
    let options_ptr = options.as_ref()
      .map(|n| n.as_ptr() )
      .unwrap_or(ptr::null());

    let s = unsafe {
      sys::amd_comgr_action_info_set_options(h, options_ptr)
    };
    Error::check(s)?;

    Ok(())
  }

  pub fn get_working_path(&self) -> Result<Option<PathBuf>, Error> {
    let h = self.handle();
    let mut len = 0usize;
    let s = unsafe {
      sys::amd_comgr_action_info_get_working_directory_path(h, &mut len as *mut _,
                                                            ptr::null_mut())
    };
    Error::check(s)?;

    if len <= 1 { return Ok(None); }

    // ignore the null terminator:
    len -= 1;

    let mut data = Vec::new();
    data.resize(len, 0u8);

    let s = unsafe {
      sys::amd_comgr_action_info_get_working_directory_path(h, &mut len as *mut _,
                                                            data.as_mut_ptr() as *mut _)
    };
    Error::check(s)?;

    // we only allow setting the name using a `String`,
    // so we don't have to check for utf8-ness here.
    let s = unsafe { String::from_utf8_unchecked(data) };
    Ok(Some(s.into()))
  }
  pub fn set_working_path(&mut self, path: Option<PathBuf>) -> Result<(), Error> {
    let path = if let Some(ref path) = path {
      let path = path.to_str()
        .ok_or(Error::InvalidArgument)?;
      Some(CString::new(path).unwrap())
    } else {
      None
    };

    let path_ptr = path.as_ref()
      .map(|n| n.as_ptr() )
      .unwrap_or(ptr::null());

    let h = self.handle();
    let s = unsafe {
      sys::amd_comgr_action_info_set_working_directory_path(h, path_ptr)
    };
    Error::check(s)?;

    Ok(())
  }

  pub(crate) fn handle(&self) -> sys::amd_comgr_action_info_t {
    sys::amd_comgr_action_info_s {
      handle: self.0.get(),
    }
  }
}
impl Drop for ActionInfo {
  fn drop(&mut self) {
    let h = self.handle();
    unsafe {
      sys::amd_comgr_destroy_action_info(h);
    }
  }
}
impl !Sync for ActionInfo { }
impl !Send for ActionInfo { }

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Action {
  pub kind: ActionKind,
  pub info: ActionInfo,
}
impl Deref for Action {
  type Target = ActionInfo;
  fn deref(&self) -> &ActionInfo {
    &self.info
  }
}
impl DerefMut for Action {
  fn deref_mut(&mut self) -> &mut ActionInfo {
    &mut self.info
  }
}
