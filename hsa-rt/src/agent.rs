
use std::fmt;
use std::os::raw::{c_void};
use std::ops::Range;
use std::result::Result;
use std::str::from_utf8;
use std::mem::transmute;

#[cfg(feature = "serde")]
use serde::{Serialize, Deserialize, };

use ApiContext;
use crate::error::Error;
use ffi;
use std::num::NonZeroU64;

macro_rules! agent_info {
  ($self:expr, $id:expr, $out:expr) => {
    {
      let mut out = $out;
      let __agent = ffi::hsa_agent_s { handle: $self.0.get(), };
      check_err!(ffi::hsa_agent_get_info(__agent, $id, out.as_mut_ptr() as *mut _) => out)
    }
  }
}
macro_rules! cache_info {
  ($self:expr, $id:expr, $out:expr) => {
    {
      let mut out = $out;
      check_err!(ffi::hsa_cache_get_info($self.0, $id,
                                         out.as_mut_ptr() as *mut _) => out)
    }
  }
}
macro_rules! isa_info {
  ($self:expr, $id:expr, $out:expr) => {
    {
      let mut out = $out;
      check_err!(ffi::hsa_isa_get_info_alt($self.0, $id,
                                           out.as_mut_ptr() as *mut _) => out)
    }
  }
}

macro_rules! wavefront_info {
  ($self:expr, $id:expr, $out:expr) => {
    {
      let mut out = $out;
      check_err!(ffi::hsa_wavefront_get_info($self.0, $id,
                                             out.as_mut_ptr() as *mut _) => out)
    }
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Feature {
  Agent,
  Kernel,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum QueueType {
  Single,
  Multiple,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DeviceType {
  Cpu,
  Gpu,
  Dsp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MachineModels(bool, bool);
impl MachineModels {
  pub fn supports_small(&self) -> bool {
    self.0
  }
  pub fn supports_large(&self) -> bool {
    self.1
  }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Profiles(pub(crate) bool, pub(crate) bool);
impl Profiles {
  pub fn base() -> Self {
    Profiles(true, false)
  }
  pub fn full() -> Self {
    Profiles(true, true)
  }
  pub fn supports_base(&self) -> bool { self.0 }
  pub fn supports_full(&self) -> bool { self.1 }
}
#[doc(hidden)]
impl Into<ffi::hsa_profile_t> for Profiles {
  fn into(self) -> ffi::hsa_profile_t {
    if self.supports_full() {
      ffi::hsa_profile_t_HSA_PROFILE_FULL
    } else {
      ffi::hsa_profile_t_HSA_PROFILE_BASE
    }
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DefaultFloatRoundingModes(pub(crate) bool,
                                     pub(crate) bool,
                                     pub(crate) bool);
impl DefaultFloatRoundingModes {
  /// As (poorly) named by the spec.
  pub fn supports_default(&self) -> bool { self.0 }
  pub fn supports_zero(&self) -> bool { self.1 }
  pub fn supports_near(&self) -> bool { self.2 }

  pub fn near() -> Self {
    DefaultFloatRoundingModes(false, false, true)
  }
}
impl Default for DefaultFloatRoundingModes {
  fn default() -> Self {
    DefaultFloatRoundingModes(true, false, false)
  }
}

#[doc(hidden)]
// note: only one can be set for sane behaviour.
impl Into<ffi::hsa_default_float_rounding_mode_t> for DefaultFloatRoundingModes {
  fn into(self) -> ffi::hsa_default_float_rounding_mode_t {
    if self.supports_default() {
      ffi::hsa_default_float_rounding_mode_t_HSA_DEFAULT_FLOAT_ROUNDING_MODE_DEFAULT
    } else if self.supports_near() {
      ffi::hsa_default_float_rounding_mode_t_HSA_DEFAULT_FLOAT_ROUNDING_MODE_NEAR
    } else {
      ffi::hsa_default_float_rounding_mode_t_HSA_DEFAULT_FLOAT_ROUNDING_MODE_ZERO
    }
  }
}


#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Cache(ffi::hsa_cache_t, ApiContext);

impl Cache {
  pub fn name(&self) -> Result<String, Error> {
    let len = cache_info!(self, ffi::hsa_cache_info_t_HSA_CACHE_INFO_NAME_LENGTH,
                          [0u32; 1])?[0] as usize;
    let str = vec![0; len + 1];
    let mut str = cache_info!(self, ffi::hsa_cache_info_t_HSA_CACHE_INFO_NAME, str)?;
    while let Some(&0) = str.last() {
      str.pop();
    }
    let str = String::from_utf8(str)?;
    Ok(str)
  }
  pub fn level(&self) -> Result<u8, Error> {
    Ok(cache_info!(self, ffi::hsa_cache_info_t_HSA_CACHE_INFO_LEVEL,
                   [0u8; 1])?[0])
  }
  pub fn size(&self) -> Result<u32, Error> {
    Ok(cache_info!(self, ffi::hsa_cache_info_t_HSA_CACHE_INFO_SIZE,
                   [0u32; 1])?[0])
  }
  pub fn info(&self) -> Result<CacheInfo, Error> {
    Ok(CacheInfo {
      name: self.name()?,
      level: self.level()?,
      size: self.size()?,
    })
  }
}
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CacheInfo {
  pub name: String,
  pub level: u8,
  pub size: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Wavefront(ffi::hsa_wavefront_t, ApiContext);
impl Wavefront {
  pub fn size(&self) -> Result<u32, Error> {
    let size = wavefront_info!(self, ffi::hsa_wavefront_info_t_HSA_WAVEFRONT_INFO_SIZE,
                               [0u32; 1])?;
    Ok(size[0])
  }
  pub fn info(&self) -> Result<WavefrontInfo, Error> {
    Ok(WavefrontInfo {
      size: self.size()?,
    })
  }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct WavefrontInfo {
  pub size: u32,
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Isa(ffi::hsa_isa_t, ApiContext);
impl Isa {
  pub fn name(&self) -> Result<String, Error> {
    let len = isa_info!(self, ffi::hsa_isa_info_t_HSA_ISA_INFO_NAME_LENGTH,
                              [0u32; 1])?[0] as usize;
    let str = vec![0; len + 1];
    let mut str = isa_info!(self, ffi::hsa_isa_info_t_HSA_ISA_INFO_NAME, str)?;
    while let Some(&0) = str.last() {
      str.pop();
    }
    let str = String::from_utf8(str)?;
    Ok(str)
  }

  pub fn machine_model(&self) -> Result<MachineModels, Error> {
    let models = isa_info!(self, ffi::hsa_isa_info_t_HSA_ISA_INFO_MACHINE_MODELS,
                           [false; 2])?;

    Ok(MachineModels(models[0],
                     models[1]))
  }
  pub fn profiles(&self) -> Result<Profiles, Error> {
    let profiles = isa_info!(self, ffi::hsa_isa_info_t_HSA_ISA_INFO_PROFILES,
                             [false; 2])?;

    Ok(Profiles(profiles[0], profiles[1]))
  }
  pub fn default_float_rounding_modes(&self) -> Result<DefaultFloatRoundingModes, Error> {
    let modes = isa_info!(self, ffi::hsa_isa_info_t_HSA_ISA_INFO_DEFAULT_FLOAT_ROUNDING_MODES,
                          [false; 3])?;

    Ok(DefaultFloatRoundingModes(modes[0],
                                 modes[1],
                                 modes[2]))
  }
  pub fn base_profile_default_float_rounding_modes(&self) -> Result<DefaultFloatRoundingModes, Error> {
    let modes = isa_info!(self, ffi::hsa_isa_info_t_HSA_ISA_INFO_BASE_PROFILE_DEFAULT_FLOAT_ROUNDING_MODES,
                          [false; 3])?;

    Ok(DefaultFloatRoundingModes(modes[0],
                                 modes[1],
                                 modes[2]))
  }
  pub fn fast_f16(&self) -> Result<bool, Error> {
    let fast = isa_info!(self, ffi::hsa_isa_info_t_HSA_ISA_INFO_FAST_F16_OPERATION,
                          [false; 1])?;

    Ok(fast[0])
  }
  pub fn workgroup_max_dim(&self) -> Result<[u16; 3], Error> {
    let dim = isa_info!(self, ffi::hsa_isa_info_t_HSA_ISA_INFO_WORKGROUP_MAX_DIM,
                        [0u16; 3])?;
    Ok(dim)
  }
  pub fn workgroup_max_size(&self) -> Result<u32, Error> {
    let size = isa_info!(self, ffi::hsa_isa_info_t_HSA_ISA_INFO_WORKGROUP_MAX_SIZE,
                         [0u32; 1])?;
    Ok(size[0])
  }
  pub fn grid_max_dim(&self) -> Result<ffi::hsa_dim3_t, Error> {
    let grid = isa_info!(self, ffi::hsa_isa_info_t_HSA_ISA_INFO_GRID_MAX_DIM,
                         [ffi::hsa_dim3_t {
                           x: 0,
                           y: 0,
                           z: 0,
                          }; 1])?;
    Ok(grid[0])
  }
  pub fn grid_max_size(&self) -> Result<u64, Error> {
    let size = isa_info!(self, ffi::hsa_isa_info_t_HSA_ISA_INFO_GRID_MAX_SIZE,
                         [0u64; 1])?;
    Ok(size[0])
  }
  pub fn fbarrier_max_size(&self) -> Result<u32, Error> {
    let size = isa_info!(self, ffi::hsa_isa_info_t_HSA_ISA_INFO_FBARRIER_MAX_SIZE,
                         [0u32; 1])?;
    Ok(size[0])
  }

  pub fn wavefronts(&self) -> Result<Vec<Wavefront>, Error> {
    extern "C" fn get(out: ffi::hsa_wavefront_t,
                      items: *mut c_void) -> ffi::hsa_status_t {
      let items: &mut Vec<Wavefront> = unsafe {
        transmute(items)
      };
      items.push(Wavefront(out, ApiContext::default()));
      ffi::hsa_status_t_HSA_STATUS_SUCCESS
    }

    let mut out: Vec<Wavefront> = vec![];
    Ok(check_err!(ffi::hsa_isa_iterate_wavefronts(self.0, Some(get),
                                                  transmute(&mut out)) => out)?)
  }
  pub fn info(&self) -> Result<IsaInfo, Error> {
    Ok(IsaInfo {
      name: self.name()?,
      machine_model: self.machine_model()?,
      profiles: self.profiles()?,
      default_float_rounding_modes: self.default_float_rounding_modes()?,
      base_profile_default_float_rounding_modes: self.base_profile_default_float_rounding_modes()?,
      fast_f16: self.fast_f16()?,
      workgroup_max_dim: self.workgroup_max_dim()?,
      workgroup_max_size: self.workgroup_max_size()?,
      grid_max_dim: {
        let dim = self.grid_max_dim()?;
        [dim.x, dim.y, dim.z]
      },
      grid_max_size: self.grid_max_size()?,
      fbarrier_max_size: self.fbarrier_max_size()?,
      wavefronts: {
        let mut o = vec![];
        for wavefront in self.wavefronts().unwrap_or_default().into_iter() {
          o.push(wavefront.info()?);
        }
        o
      },
    })
  }
}

impl fmt::Debug for Isa {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let name = self.name();
    let name = name.as_ref()
      .map(|v| v.as_ref() )
      .unwrap_or_else(|_| "<bad name!>" );
    write!(f, "Isa({})", name)
  }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IsaInfo {
  pub name: String,
  pub machine_model: MachineModels,
  pub profiles: Profiles,
  pub default_float_rounding_modes: DefaultFloatRoundingModes,
  pub base_profile_default_float_rounding_modes: DefaultFloatRoundingModes,
  pub fast_f16: bool,
  pub workgroup_max_dim: [u16; 3],
  pub workgroup_max_size: u32,
  pub grid_max_dim: [u32; 3],
  pub grid_max_size: u64,
  pub fbarrier_max_size: u32,
  pub wavefronts: Vec<WavefrontInfo>,
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(transparent)]
pub struct Agent(pub(crate) NonZeroU64, pub(crate) ApiContext);
impl Agent {
  #[inline(always)]
  pub(crate) fn handle(&self) -> ffi::hsa_agent_t {
    ffi::hsa_agent_s {
      handle: self.0.get(),
    }
  }
  pub fn name(&self) -> Result<String, Error> {
    let bytes = agent_info!(self, ffi::hsa_agent_info_t_HSA_AGENT_INFO_NAME, [0u8; 64])?;
    let mut str = &bytes[..];
    while let Some(&0u8) = str.last() {
      let l = str.len();
      str = &str[..l-1];
    }
    Ok(from_utf8(str)?.into())
  }
  pub fn vendor_name(&self) -> Result<String, Error> {
    let bytes = agent_info!(self, ffi::hsa_agent_info_t_HSA_AGENT_INFO_VENDOR_NAME, [0u8; 54])?;
    let mut str = &bytes[..];
    while let Some(&0u8) = str.last() {
      let l = str.len();
      str = &str[..l-1];
    }
    Ok(from_utf8(str)?.into())
  }
  pub fn feature(&self) -> Result<Feature, Error> {
    let feature = agent_info!(self, ffi::hsa_agent_info_t_HSA_AGENT_INFO_FEATURE, [0; 1])?;
    match feature[0] {
      ffi::hsa_agent_feature_t_HSA_AGENT_FEATURE_AGENT_DISPATCH => Ok(Feature::Agent),
      ffi::hsa_agent_feature_t_HSA_AGENT_FEATURE_KERNEL_DISPATCH => Ok(Feature::Kernel),
      _ => Err(Error::General),
    }
  }
  pub fn queue_size(&self) -> Result<Range<u32>, Error> {
    let min = agent_info!(self, ffi::hsa_agent_info_t_HSA_AGENT_INFO_QUEUE_MIN_SIZE, [0u32; 1])?;
    let max = agent_info!(self, ffi::hsa_agent_info_t_HSA_AGENT_INFO_QUEUE_MAX_SIZE, [0u32; 1])?;

    Ok(Range {
      start: min[0],
      end:   max[0],
    })
  }
  pub fn queue_type(&self) -> Result<QueueType, Error> {
    let ty = agent_info!(self, ffi::hsa_agent_info_t_HSA_AGENT_INFO_QUEUE_TYPE, [0u32; 1])?;
    match ty[0] {
      ffi::hsa_queue_type_t_HSA_QUEUE_TYPE_MULTI => Ok(QueueType::Multiple),
      ffi::hsa_queue_type_t_HSA_QUEUE_TYPE_SINGLE => Ok(QueueType::Single),
      _ => Err(Error::General),
    }
  }
  pub fn extensions(&self) -> Result<[u8; 128], Error> {
    Ok(agent_info!(self, ffi::hsa_agent_info_t_HSA_AGENT_INFO_EXTENSIONS, [0u8; 128])?)
  }
  pub fn version(&self) -> Result<(u16, u16), Error> {
    let major = agent_info!(self, ffi::hsa_agent_info_t_HSA_AGENT_INFO_VERSION_MAJOR,
                            [0u16; 1])?;
    let minor = agent_info!(self, ffi::hsa_agent_info_t_HSA_AGENT_INFO_VERSION_MINOR,
                            [0u16; 1])?;

    Ok((major[0], minor[0]))
  }

  pub fn device_type(&self) -> Result<DeviceType, Error> {
    let ty = agent_info!(self, ffi::hsa_agent_info_t_HSA_AGENT_INFO_DEVICE,
                         [0u32; 1])?;
    let o = match ty[0] {
      ffi::hsa_device_type_t_HSA_DEVICE_TYPE_CPU => DeviceType::Cpu,
      ffi::hsa_device_type_t_HSA_DEVICE_TYPE_GPU => DeviceType::Gpu,
      ffi::hsa_device_type_t_HSA_DEVICE_TYPE_DSP => DeviceType::Dsp,
      _ => { return Err(Error::General); },
    };

    Ok(o)
  }

  pub fn caches(&self) -> Result<Vec<Cache>, Error> {
    extern "C" fn get(out: ffi::hsa_cache_t,
                      items: *mut c_void) -> ffi::hsa_status_t {
      let items: &mut Vec<Cache> = unsafe {
        transmute(items)
      };
      let c = Cache(out, ApiContext::default());
      items.push(c);
      ffi::hsa_status_t_HSA_STATUS_SUCCESS
    }

    let mut out: Vec<Cache> = vec![];
    let agent = ffi::hsa_agent_s {
      handle: self.0.get(),
    };
    Ok(check_err!(ffi::hsa_agent_iterate_caches(agent, Some(get),
                                                transmute(&mut out)) => out)?)
  }

  pub fn isas(&self) -> Result<Vec<Isa>, Error> {
    extern "C" fn get(out: ffi::hsa_isa_t,
                      items: *mut c_void) -> ffi::hsa_status_t {
      let items: &mut Vec<Isa> = unsafe {
        transmute(items)
      };
      items.push(Isa(out, ApiContext::upref()));
      ffi::hsa_status_t_HSA_STATUS_SUCCESS
    }

    let mut out: Vec<Isa> = vec![];
    let agent = ffi::hsa_agent_s {
      handle: self.0.get(),
    };
    Ok(check_err!(ffi::hsa_agent_iterate_isas(agent, Some(get),
                                              transmute(&mut out)) => out)?)
  }

  pub fn info(&self) -> Result<AgentInfo, Error> {
    Ok(AgentInfo {
      name: self.name()?,
      vendor: self.vendor_name()?,
      feature: self.feature()?,
      queue_size: self.queue_size()?,
      queue_type: self.queue_type()?,
      extensions: self.extensions()?.iter()
        .map(|&v| v )
        .collect(),
      version: self.version()?,
      device_type: self.device_type()?,
      caches: {
        let mut o = vec![];
        for cache in self.caches().unwrap_or_default().into_iter() {
          o.push(cache.info()?);
        }
        o
      },
      isas: {
        let mut o = vec![];
        for isa in self.isas().unwrap_or_default().into_iter() {
          o.push(isa.info()?);
        }
        o
      },
    })
  }

  #[doc(hidden)]
  pub unsafe fn raw_handle(&self) -> ffi::hsa_agent_t { self.handle() }
}
impl ::ContextRef for Agent {
  fn context(&self) -> &ApiContext { &self.1 }
}
impl fmt::Debug for Agent {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let name = self.name();
    let name = name.as_ref()
      .map(|v| v.as_ref() )
      .unwrap_or_else(|_| "<bad name!>" );
    write!(f, "Agent({}, \"{}\")", self.handle().handle, name)
  }
}
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AgentInfo {
  pub name: String,
  pub vendor: String,
  pub feature: Feature,
  pub queue_size: Range<u32>,
  pub queue_type: QueueType,
  pub extensions: Vec<u8>,
  pub version: (u16, u16),
  pub device_type: DeviceType,
  pub caches: Vec<CacheInfo>,
  pub isas: Vec<IsaInfo>,
}

impl ApiContext {
  pub fn agents(&self) -> Result<Vec<Agent>, Error> {
    extern "C" fn get_agent(agent_out: ffi::hsa_agent_t,
                            agents: *mut c_void) -> ffi::hsa_status_t {
      let agents: &mut Vec<Option<Agent>> = unsafe {
        transmute(agents)
      };
      let agent = NonZeroU64::new(agent_out.handle)
        .map(|agent| Agent(agent, ApiContext::upref()) );
      agents.push(agent);
      ffi::hsa_status_t_HSA_STATUS_SUCCESS
    }

    let mut out: Vec<Option<Agent>> = vec![];
    check_err!(ffi::hsa_iterate_agents(Some(get_agent), transmute(&mut out)) => ())?;

    let out = out.into_iter()
      .filter_map(|v| v )
      .collect();
    Ok(out)
  }
}
