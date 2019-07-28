
use serde::{Serialize, Deserialize, };

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Platform {
  Unix,
  Windows,

  Hsa(self::hsa::Device),
  /// Present, but ATM completely unsupported.
  Cuda,
  Vulkan(self::vk::ExeModel),
}

pub mod hsa {
  use serde::{Serialize, Deserialize, };
  use std::str::FromStr;

  /// These are taken from the AMDGPU LLVM target machine.
  /// TODO do we care about pre-GFX8 GPUs?
  #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
  pub enum AmdGpu {
    //===----------------------------------------------------------------------===//
    // GCN GFX8 (Volcanic Islands (VI)).
    //===----------------------------------------------------------------------===//
    Gfx801,
    Carrizo,
    Gfx802,
    Iceland,
    Tonga,
    Gfx803,
    Fiji,
    Polaris10,
    Polaris11,
    Gfx810,
    Stoney,

    //===----------------------------------------------------------------------===//
    // GCN GFX9.
    //===----------------------------------------------------------------------===//

    Gfx900,
    Gfx902,
    Gfx904,
    Gfx906,
    Gfx909,
  }
  impl FromStr for AmdGpu {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
      use self::AmdGpu::*;

      let v = match s {
        "gfx801" => Gfx801,
        "carrizo" => Carrizo,
        "gfx802" => Gfx802,
        "iceland" => Iceland,
        "tonga" => Tonga,
        "gfx803" => Gfx803,
        "fiji" => Fiji,
        "polaris10" => Polaris10,
        "polaris11" => Polaris11,
        "gfx810" => Gfx810,
        "stoney" => Stoney,

        "gfx900" => Gfx900,
        "gfx902" => Gfx902,
        "gfx904" => Gfx904,
        "gfx906" => Gfx906,
        "gfx909" => Gfx909,

        _ => { return Err(()); },
      };

      Ok(v)
    }
  }

  #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
  pub enum Device {
    AmdGcn(AmdGpu)
  }
}
pub mod vk {
  use std::str::FromStr;

  use serde::{Serialize, Deserialize, };

  #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
  #[repr(u32)]
  pub enum ExeModel {
    Vertex,
    TessellationControl,
    TessellationEval,
    Geometry,
    Fragment,
    GLCompute,
    /// Present for completeness, not actually used.
    Kernel,
  }
  impl FromStr for ExeModel {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
      let r = match s {
        "Vertex" => ExeModel::Vertex,
        "TessellationControl" => ExeModel::TessellationControl,
        "TessellationEval" => ExeModel::TessellationEval,
        "Geometry" => ExeModel::Geometry,
        "Fragment" => ExeModel::Fragment,
        "GLCompute" => ExeModel::GLCompute,
        "Kernel" => ExeModel::Kernel,
        _ => { return Err(()); },
      };

      Ok(r)
    }
  }
}

#[cfg(unix)]
#[inline(always)]
pub const fn host_platform() -> Platform {
  Platform::Unix
}
#[cfg(windows)]
#[inline(always)]
pub const fn host_platform() -> Platform {
  Platform::Windows
}
