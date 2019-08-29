
/// Types, functions, and globals for kernels

use geobacter_core::kernel::{KernelId, kernel_id_for, };
use spirv_help::*;

use gcore::{Capabilities, Capability, };
use vk_help::{StaticPipelineLayoutDesc,
              compute_pipeline_layout_desc,
              compute_pipeline_required_capabilities,
              compute_pipeline_required_extensions, };

extern "Rust" {

// Note the initializers for input storage class globals are
// deleted. So their value here doesn't matter.
#[geobacter(spirv_builtin = "DeviceIndex",
            storage_class = "Input")]
#[linkage = "extern_weak"]
static DEVICE_INDEX: *const u32;

#[geobacter(exe_model(any(Kernel, GLCompute)),
            spirv_builtin = "LocalInvocationId",
            storage_class = "Input")]
#[linkage = "extern_weak"]
static LOCAL_INVOCATION_ID: *const Vec3<u32>;

#[geobacter(exe_model(any(Kernel, GLCompute)),
            spirv_builtin = "LocalInvocationIndex",
            storage_class = "Input")]
#[linkage = "extern_weak"]
static LOCAL_INVOCATION_INDEX: *const u32;

#[geobacter(exe_model(any(Kernel, GLCompute)),
            spirv_builtlin = "NumSubgroups",
            storage_class = "Input")]
#[linkage = "extern_weak"]
static NUM_SUBGROUPS: *const u32;

#[geobacter(exe_model(any(Kernel, GLCompute)),
            spirv_builtlin = "NumWorkgroups",
            storage_class = "Input")]
#[linkage = "extern_weak"]
static NUM_WORKGROUPS: *const Vec3<u32>;

#[geobacter(spirv_builtlin = "SubgroupSize",
            storage_class = "Input")]
#[linkage = "extern_weak"]
static SUBGROUP_SIZE: *const u32;

#[geobacter(exe_model(any(Kernel, GLCompute)),
            spirv_builtlin = "WorkgroupId",
            storage_class = "Input")]
#[linkage = "extern_weak"]
static WORKGROUP_ID: *const Vec3<u32>;

#[geobacter(exe_model(any(Kernel, GLCompute)),
            spirv_builtlin = "WorkgroupSize",
            storage_class = "Input")]
#[linkage = "extern_weak"]
static WORKGROUP_SIZE: *const Vec3<u32>;

}

#[geobacter(exe_model(any(Kernel, GLCompute)),
            spirv_builtin = "GlobalInvocationId",
            storage_class = "Input")]
static GLOBAL_INVOCATION_ID: Vec3<u32> = Vec3::new_u32_c([0, 0, 0, ]);

macro_rules! g {
  ($name:ident) => {
    g(unsafe {
      $name.as_ref()
        .expect(concat!("don't call `", stringify!($name), "` on the host!"))
    }) as _
  };
}
macro_rules! gv3 {
  ($name:ident) => {{
    let v = g(unsafe {
      $name.as_ref()
        .expect(concat!("don't call `", stringify!($name), "` on the host!"))
    });
    Vec3::new([v.x() as _, v.y() as _, v.z() as _, ].into())
  }};
}
pub fn device_index() -> usize { g!(DEVICE_INDEX) }
pub fn global_invocation_id() -> Vec3<usize> {
  let id = GLOBAL_INVOCATION_ID;
  Vec3::new_usize_c([id.0 as _, id.1 as _, id.2 as _, ])
}
pub fn local_invocation_id() -> Vec3<usize> { gv3!(LOCAL_INVOCATION_ID) }
pub fn local_invocation_index() -> usize { g!(LOCAL_INVOCATION_INDEX) }
pub fn num_subgroups() -> usize { g!(NUM_SUBGROUPS) }
pub fn num_workgroups() -> Vec3<usize> { gv3!(NUM_WORKGROUPS) }
pub fn subgroup_size() -> usize { g!(SUBGROUP_SIZE) }
pub fn workgroup_id() -> Vec3<usize> { gv3!(WORKGROUP_ID) }
pub fn workgroup_size() -> Vec3<usize> { gv3!(WORKGROUP_SIZE) }

extern "rust-intrinsic" {
  fn __geobacter_check_glcompute_shader<F>(f: &F)
    where F: Fn<(), Output = ()>;
  fn __geobacter_check_kernel_shader<F>(f: &F)
    where F: Fn<(), Output = ()>;
}

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct KernelDesc {
  pub id: KernelId,
  pub pipeline_desc: StaticPipelineLayoutDesc,
  pub capabilities: Capabilities<'static, [Capability]>,
  pub extensions: &'static [&'static str],
}

impl KernelDesc {
  pub fn raw_device_extensions(&self) -> vk::device::RawDeviceExtensions {
    use std::ffi::CString;

    let iter = self.extensions.iter()
      .map(|&ext| unsafe {
        let ext = ext.to_string();

        CString::from_vec_unchecked(ext.into())
      });

    vk::device::RawDeviceExtensions::new(iter)
  }
}

pub fn kernel_desc<F>(f: &F) -> KernelDesc
  where F: Fn<(), Output = ()>,
{
  unsafe {
    __geobacter_check_glcompute_shader(f);
  }

  let id = kernel_id_for(f);
  let pipeline_desc =
    compute_pipeline_layout_desc(f);
  let caps =
    compute_pipeline_required_capabilities(f);
  let exts = compute_pipeline_required_extensions(f);

  KernelDesc {
    id,
    pipeline_desc,
    capabilities: caps,
    extensions: exts,
  }
}
