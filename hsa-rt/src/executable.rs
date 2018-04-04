
use std::error::Error;
use std::ffi::CString;
use std::mem::transmute;
use std::os::raw::c_void;

use ffi;

use agent::{Profiles, DefaultFloatRoundingModes, Agent};
use code_object::{CodeObjectReader, LoadedCodeObject};


macro_rules! exe_info {
  ($self:expr, $id:expr, $out:expr) => {
    {
      let mut out = $out;
      check_err!(ffi::hsa_executable_get_info($self.0, $id, out.as_mut_ptr() as *mut _) => out)
    }
  }
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Executable(ffi::hsa_executable_t);

impl Executable {
  pub fn create<T>(profile: Profiles,
                   rounding_mode: DefaultFloatRoundingModes,
                   options: T) -> Result<Executable, Box<Error>>
    where T: AsRef<str>,
  {
    let options = CString::new(options.as_ref())?;
    let mut this = Executable(ffi::hsa_executable_s {
      handle: 0,
    });
    let o = check_err!(ffi::hsa_executable_create_alt(profile.into(),
                                                      rounding_mode.into(),
                                                      options.as_ptr(),
                                                      transmute(&mut this.0)) => this)?;
    Ok(o)
  }

  pub fn load_agent_code_object<T, U>(&self, agent: Agent, reader: &T,
                                      options: U) -> Result<LoadedCodeObject, Box<Error>>
    where T: CodeObjectReader,
          U: AsRef<str>,
  {
    let options = CString::new(options.as_ref())?;
    let mut this = LoadedCodeObject(ffi::hsa_loaded_code_object_s {
      handle: 0,
    });
    let o = check_err!(ffi::hsa_executable_load_agent_code_object(self.0,
                                                                  agent.0,
                                                                  reader.sys().0,
                                                                  options.as_ptr(),
                                                                  transmute(&mut this.0)) => this)?;

    Ok(o)
  }

  pub fn define_global_variable(&self,
                                name: &str,
                                ptr: *mut c_void) -> Result<(), Box<Error>> {
    let name = CString::new(name)?;
    check_err!(ffi::hsa_executable_global_variable_define(self.0,
                                                          name.as_ptr(),
                                                          ptr))?;
    Ok(())
  }
  pub fn define_agent_global_variable(&self,
                                      agent: &Agent,
                                      name: &str,
                                      ptr: *mut c_void) -> Result<(), Box<Error>> {
    let name = CString::new(name)?;
    check_err!(ffi::hsa_executable_agent_global_variable_define(self.0,
                                                                agent.0,
                                                                name.as_ptr(),
                                                                ptr))?;
    Ok(())
  }

  pub fn freeze<T>(self, options: T) -> Result<FrozenExecutable, Box<Error>>
    where T: AsRef<str>,
  {
    let options = CString::new(options.as_ref())?;
    check_err!(ffi::hsa_executable_freeze(self.0, options.as_ptr() as _))?;
    Ok(FrozenExecutable(self))
  }
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FrozenExecutable(Executable);
impl FrozenExecutable {

}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum State {
  Unfrozen,
  Frozen,
}

pub trait CommonExecutable {
  #[doc(hidden)]
  fn sys(&self) -> &Executable;

  fn profile(&self) -> Result<Profiles, Box<Error>> {
    let profile = exe_info!(self.sys(), ffi::hsa_executable_info_t_HSA_EXECUTABLE_INFO_PROFILE,
                            [0u32; 1])?;
    match profile[0] {
      ffi::hsa_profile_t_HSA_PROFILE_BASE => Ok(Profiles(true, false)),
      ffi::hsa_profile_t_HSA_PROFILE_FULL => Ok(Profiles(false, true)),
      _ => {
        return Ok(Err(format!("unknown profile enum value: {}", profile[0]))?);
      },
    }
  }
  fn state(&self) -> Result<State, Box<Error>> {
    let state = exe_info!(self.sys(), ffi::hsa_executable_info_t_HSA_EXECUTABLE_INFO_STATE,
                          [0u32; 1])?;
    match state[0] {
      ffi::hsa_executable_state_t_HSA_EXECUTABLE_STATE_UNFROZEN => Ok(State::Unfrozen),
      ffi::hsa_executable_state_t_HSA_EXECUTABLE_STATE_FROZEN => Ok(State::Frozen),
      _ => {
        return Ok(Err(format!("unknown state enum value: {}", state[0]))?);
      },
    }
  }
  fn default_float_rounding_mode(&self) -> Result<DefaultFloatRoundingModes, Box<Error>> {
    let mode = exe_info!(self.sys(), ffi::hsa_executable_info_t_HSA_EXECUTABLE_INFO_DEFAULT_FLOAT_ROUNDING_MODE,
                         [0u32; 1])?;
    match mode[0] {
      ffi::hsa_default_float_rounding_mode_t_HSA_DEFAULT_FLOAT_ROUNDING_MODE_DEFAULT =>
        Ok(DefaultFloatRoundingModes(true, false, false)),
      ffi::hsa_default_float_rounding_mode_t_HSA_DEFAULT_FLOAT_ROUNDING_MODE_ZERO =>
        Ok(DefaultFloatRoundingModes(false, true, false)),
      ffi::hsa_default_float_rounding_mode_t_HSA_DEFAULT_FLOAT_ROUNDING_MODE_NEAR =>
        Ok(DefaultFloatRoundingModes(false, false, true)),
      _ => {
        return Ok(Err(format!("unknown float rounding mode enum value: {}", mode[0]))?);
      },
    }
  }
}

impl CommonExecutable for Executable {
  #[doc(hidden)]
  fn sys(&self) -> &Executable { self }
}
impl CommonExecutable for FrozenExecutable {
  #[doc(hidden)]
  fn sys(&self) -> &Executable { &self.0 }
}

impl Drop for Executable {
  fn drop(&mut self) {
    unsafe {
      ffi::hsa_executable_destroy(self.0);
    }
  }
}

