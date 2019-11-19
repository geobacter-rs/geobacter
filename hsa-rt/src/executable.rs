
use std::ffi::CString;
use std::mem::transmute;
use std::num::NonZeroU64;
use std::os::raw::c_void;

use crate::ApiContext;
use crate::agent::{Profiles, DefaultFloatRoundingModes, Agent};
use crate::code_object::{CodeObjectReader, LoadedCodeObject};
use crate::error::Error;
use crate::ffi;
use utils::uninit;

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
                options: T) -> Result<Executable, Error>
    where T: Into<Vec<u8>>,
  {
    let options = CString::new(options)
      .map_err(|_| Error::InvalidArgument )?;
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
                                      options: U)
    -> Result<LoadedCodeObject, Error>
    where T: CodeObjectReader,
          U: Into<Vec<u8>>,
  {
    let options = CString::new(options)
      .map_err(|_| Error::InvalidArgument )?;
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
                                ptr: *mut c_void)
    -> Result<(), Error>
  {
    let name = CString::new(name)
      .map_err(|_| Error::InvalidArgument )?;
    check_err!(ffi::hsa_executable_global_variable_define(self.0,
                                                          name.as_ptr(),
                                                          ptr))?;
    Ok(())
  }
  pub fn define_agent_global_variable(&self,
                                      agent: &Agent,
                                      name: &str,
                                      ptr: *mut c_void)
    -> Result<(), Error>
  {
    let name = CString::new(name)
      .map_err(|_| Error::InvalidArgument )?;
    check_err!(ffi::hsa_executable_agent_global_variable_define(self.0,
                                                                agent.0,
                                                                name.as_ptr(),
                                                                ptr))?;
    Ok(())
  }

  pub fn freeze<T>(self, options: T) -> Result<FrozenExecutable, Error>
    where T: Into<Vec<u8>>,
  {
    let options = CString::new(options)
      .map_err(|_| Error::InvalidArgument )?;
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

  fn profile(&self) -> Result<Profiles, Error> {
    let profile = exe_info!(self.sys(), ffi::hsa_executable_info_t_HSA_EXECUTABLE_INFO_PROFILE,
                            [0u32; 1])?;
    match profile[0] {
      ffi::hsa_profile_t_HSA_PROFILE_BASE => Ok(Profiles(true, false)),
      ffi::hsa_profile_t_HSA_PROFILE_FULL => Ok(Profiles(false, true)),
      _ => Err(Error::General),
    }
  }
  fn state(&self) -> Result<State, Error> {
    let state = exe_info!(self.sys(), ffi::hsa_executable_info_t_HSA_EXECUTABLE_INFO_STATE,
                          [0u32; 1])?;
    match state[0] {
      ffi::hsa_executable_state_t_HSA_EXECUTABLE_STATE_UNFROZEN => Ok(State::Unfrozen),
      ffi::hsa_executable_state_t_HSA_EXECUTABLE_STATE_FROZEN => Ok(State::Frozen),
      _ => Err(Error::General),
    }
  }
  fn default_float_rounding_mode(&self) -> Result<DefaultFloatRoundingModes, Error> {
    let mode = exe_info!(self.sys(), ffi::hsa_executable_info_t_HSA_EXECUTABLE_INFO_DEFAULT_FLOAT_ROUNDING_MODE,
                         [0u32; 1])?;
    match mode[0] {
      ffi::hsa_default_float_rounding_mode_t_HSA_DEFAULT_FLOAT_ROUNDING_MODE_DEFAULT =>
        Ok(DefaultFloatRoundingModes(true, false, false)),
      ffi::hsa_default_float_rounding_mode_t_HSA_DEFAULT_FLOAT_ROUNDING_MODE_ZERO =>
        Ok(DefaultFloatRoundingModes(false, true, false)),
      ffi::hsa_default_float_rounding_mode_t_HSA_DEFAULT_FLOAT_ROUNDING_MODE_NEAR =>
        Ok(DefaultFloatRoundingModes(false, false, true)),
      _ => Err(Error::General),
    }
  }
  fn agent_symbols(&self, agent: &Agent)
    -> Result<Vec<Symbol>, Error>
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
    -> Result<Symbol, Error>
    where T: Into<Vec<u8>>,
  {
    let name = CString::new(name)
      .map_err(|_| Error::InvalidArgument )?;
    let agent = agent
      .map(|a| &a.0 as *const _ )
      .unwrap_or(0 as *const _);
    let mut out: ffi::hsa_executable_symbol_t = unsafe {
      uninit()
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
pub enum SymbolKind {
  IndirectFunction,
  Kernel,
  Variable,
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
  pub fn name(&self) -> Result<String, Error> {
    let len = sym_info!(self, ffi::hsa_executable_symbol_info_t_HSA_EXECUTABLE_SYMBOL_INFO_NAME_LENGTH,
                        [0u32; 1])?[0] as usize;
    let bytes = sym_info!(self, ffi::hsa_executable_symbol_info_t_HSA_EXECUTABLE_SYMBOL_INFO_NAME,
                          vec![0u8; len])?;
    Ok(String::from_utf8(bytes)?)
  }
  pub fn kind(&self) -> Result<SymbolKind, Error> {
    let attr = ffi::hsa_executable_symbol_info_t_HSA_EXECUTABLE_SYMBOL_INFO_TYPE;
    let mut out: ffi::hsa_symbol_kind_t = 0;
    let out_ptr = &mut out as *mut ffi::hsa_symbol_kind_t as *mut _;
    check_err!(ffi::hsa_executable_symbol_get_info(self.1, attr, out_ptr))?;
    let kind = match out {
      ffi::hsa_symbol_kind_t_HSA_SYMBOL_KIND_INDIRECT_FUNCTION => SymbolKind::IndirectFunction,
      ffi::hsa_symbol_kind_t_HSA_SYMBOL_KIND_KERNEL => SymbolKind::Kernel,
      ffi::hsa_symbol_kind_t_HSA_SYMBOL_KIND_VARIABLE => SymbolKind::Variable,
      _ => { return Err(Error::General); },
    };

    Ok(kind)
  }
  pub fn is_kernel(&self) -> bool {
    self.kind()
      .map(|kind| match kind {
        SymbolKind::Kernel => true,
        _ => false,
      } )
      .unwrap_or_default()
  }
  pub fn kernel_object(&self) -> Result<Option<NonZeroU64>, Error> {
    let attr = ffi::hsa_executable_symbol_info_t_HSA_EXECUTABLE_SYMBOL_INFO_KERNEL_OBJECT;
    let mut out: u64 = 0;
    check_err!(ffi::hsa_executable_symbol_get_info(self.1, attr,
                                                   &mut out as *mut u64 as *mut _))?;
    Ok(NonZeroU64::new(out))
  }
  /// Size of static group segment memory required by the kernel
  /// (per work-group), in bytes. The value of this attribute is
  /// undefined if the symbol is not a kernel. The type of this
  /// attribute is uint32_t.
  ///
  /// The reported amount does not include any dynamically allocated
  /// group segment memory that may be requested by the application
  /// when a kernel is dispatched.
  pub fn kernel_group_segment_size(&self) -> Result<u32, Error> {
    let attr = ffi::hsa_code_symbol_info_t_HSA_CODE_SYMBOL_INFO_KERNEL_GROUP_SEGMENT_SIZE;
    let mut out: u32 = 0;
    check_err!(ffi::hsa_executable_symbol_get_info(self.1, attr,
                                                   &mut out as *mut u32 as *mut _))?;
    Ok(out)
  }
  /// Size of kernarg segment memory that is required to hold the
  /// values of the kernel arguments, in bytes. Must be a multiple
  /// of 16. The value of this attribute is undefined if the symbol
  /// is not a kernel. The type of this attribute is uint32_t.
  pub fn kernel_kernarg_segment_size(&self) -> Result<u32, Error> {
    let attr = ffi::hsa_code_symbol_info_t_HSA_CODE_SYMBOL_INFO_KERNEL_KERNARG_SEGMENT_SIZE;
    let mut out: u32 = 0;
    check_err!(ffi::hsa_executable_symbol_get_info(self.1, attr,
                                                   &mut out as *mut u32 as *mut _))?;
    Ok(out)
  }
  /// Alignment (in bytes) of the buffer used to pass arguments to
  /// the kernel, which is the maximum of 16 and the maximum
  /// alignment of any of the kernel arguments. The value of this
  /// attribute is undefined if the symbol is not a kernel. The type
  /// of this attribute is uint32_t.
  pub fn kernel_kernarg_segment_align(&self) -> Result<u32, Error> {
    let attr = ffi::hsa_executable_symbol_info_t_HSA_EXECUTABLE_SYMBOL_INFO_KERNEL_KERNARG_SEGMENT_ALIGNMENT;
    let mut out: u32 = 0;
    check_err!(ffi::hsa_executable_symbol_get_info(self.1, attr,
                                                   &mut out as *mut u32 as *mut _))?;
    Ok(out)
  }
  /// Size of static private, spill, and arg segment memory required
  /// by this kernel (per work-item), in bytes. The value of this
  /// attribute is undefined if the symbol is not a kernel. The type
  /// of this attribute is uint32_t.
  ///
  /// If the value of ::HSA_CODE_SYMBOL_INFO_KERNEL_DYNAMIC_CALLSTACK
  /// is true, the kernel may use more private memory than the
  /// reported value, and the application must add the dynamic call
  /// stack usage to @a private_segment_size when populating a kernel
  /// dispatch packet.
  pub fn kernel_private_segment_size(&self) -> Result<u32, Error> {
    let attr = ffi::hsa_code_symbol_info_t_HSA_CODE_SYMBOL_INFO_KERNEL_PRIVATE_SEGMENT_SIZE;
    let mut out: u32 = 0;
    check_err!(ffi::hsa_executable_symbol_get_info(self.1, attr,
                                                   &mut out as *mut u32 as *mut _))?;
    Ok(out)
  }
}
