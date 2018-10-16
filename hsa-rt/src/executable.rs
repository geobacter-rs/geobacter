
use std::error::Error;
use std::ffi::CString;
use std::mem::transmute;
use std::os::raw::c_void;

use ffi;
use ApiContext;
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
pub struct Executable(ffi::hsa_executable_t, ApiContext);

impl Executable {
  pub fn new<T>(profile: Profiles,
                rounding_mode: DefaultFloatRoundingModes,
                options: T) -> Result<Executable, Box<Error>>
    where T: AsRef<str>,
  {
    let options = CString::new(options.as_ref())?;
    let mut this = Executable(ffi::hsa_executable_s {
      handle: 0,
    },
                              ApiContext::upref());
    let o = check_err!(ffi::hsa_executable_create_alt(profile.into(),
                                                      rounding_mode.into(),
                                                      options.as_ptr(),
                                                      transmute(&mut this.0)) => this)?;
    Ok(o)
  }

  pub fn load_agent_code_object<T, U>(&self, agent: &Agent, reader: &T,
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
  fn agent_symbols(&self, agent: &Agent)
    -> Result<Vec<Symbol>, Box<Error>>
  {
    extern "C" fn get_symbol(_exec: ffi::hsa_executable_t,
                             _agent: ffi::hsa_agent_t,
                             symbol: ffi::hsa_executable_symbol_t,
                             data: *mut c_void)
      -> ffi::hsa_status_t
    {
      let &mut (exe, ref mut data): &mut (&Executable, &mut Vec<Symbol>) = unsafe {
        transmute(data)
      };
      let symbol = Symbol(exe, symbol);
      data.push(symbol);
      ffi::hsa_status_t_HSA_STATUS_SUCCESS
    }

    let mut out = Vec::new();
    {
      let mut data = (self, &mut out);
      check_err!(ffi::hsa_executable_iterate_agent_symbols(self.sys().0,
                                                           agent.0,
                                                           Some(get_symbol),
                                                           transmute(&mut data)))?;
    }
    Ok(out)
  }

  fn symbol_by_linker_name<T>(&self, name: T,
                              agent: Option<&Agent>)
    -> Result<Symbol, Box<Error>>
    where T: AsRef<str>,
  {
    let name = CString::new(name.as_ref().to_string())?;
    let agent = agent
      .map(|a| &a.0 as *const _ )
      .unwrap_or(0 as *const _);
    let mut out: ffi::hsa_executable_symbol_t = unsafe {
      ::std::mem::uninitialized()
    };
    check_err!(ffi::hsa_executable_get_symbol_by_name(self.sys().0,
                                                      name.as_ptr(),
                                                      agent,
                                                      &mut out as *mut _))?;
    Ok(Symbol(self.sys(), out))
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

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct Symbol<'a>(&'a Executable, ffi::hsa_executable_symbol_t);

macro_rules! sym_info {
  ($self:expr, $id:expr, $out:expr) => {
    {
      let mut out = $out;
      check_err!(ffi::hsa_executable_symbol_get_info($self.1, $id, out.as_mut_ptr() as *mut _) => out)
    }
  }
}

impl<'a> Symbol<'a> {
  pub fn name(&self) -> Result<String, Box<Error>> {
    let len = sym_info!(self, ffi::hsa_executable_symbol_info_t_HSA_EXECUTABLE_SYMBOL_INFO_NAME_LENGTH,
                        [0u32; 1])?[0] as usize;
    let bytes = sym_info!(self, ffi::hsa_executable_symbol_info_t_HSA_EXECUTABLE_SYMBOL_INFO_NAME,
                          vec![0u8; len])?;
    Ok(String::from_utf8(bytes)?)
  }
  pub fn kernel_object(&self) -> Result<Option<u64>, Box<Error>> {
    let attr = ffi::hsa_executable_symbol_info_t_HSA_EXECUTABLE_SYMBOL_INFO_KERNEL_OBJECT;
    let mut out: u64 = 0;
    check_err!(ffi::hsa_executable_symbol_get_info(self.1, attr,
                                                   &mut out as *mut u64 as *mut _))?;
    Ok(if out != 0 {
      Some(out)
    } else {
      None
    })
  }
}
