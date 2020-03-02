
//! This is the full Geobacter Rustc driver.
//! It includes intrinsics which themselves depend on intrinsics
//! present in `geobacter-core`.

#![feature(rustc_private)]

extern crate geobacter_intrinsics_common as common;
extern crate geobacter_amdgpu_intrinsics as amdgpu;
extern crate geobacter_vk_intrinsics as vk;
extern crate geobacter_rustc_driver_base;

extern crate rustc;
extern crate rustc_data_structures;

use self::rustc::mir::{CustomIntrinsicMirGen, };
use self::rustc::ty::{TyCtxt, };
use self::rustc_data_structures::sync::{Lrc, };

use common::DriverData;

pub fn main() {
  geobacter_rustc_driver_base::main(|gen| {
    insert_all_intrinsics(&GeneratorDriverData,
                          |k, v| {
                            let inserted = gen.intrinsics.insert(k.clone(), v);
                            assert!(inserted.is_none(), "key: {}", k);
                          });
  });
}

pub struct GeneratorDriverData;
impl common::GetDriverData for GeneratorDriverData {
  fn with_self<F, R>(tcx: TyCtxt<'_>, f: F) -> R
    where F: FnOnce(&dyn DriverData) -> R,
  {
    geobacter_rustc_driver_base::Generators::with_self(tcx, f)
  }
}

/// Call `into` for every intrinsic in every platform intrinsic crate.
pub fn insert_all_intrinsics<F, U>(marker: &U, mut into: F)
  where F: FnMut(String, Lrc<dyn CustomIntrinsicMirGen>),
        U: common::GetDriverData + Send + Sync + 'static,
{
  common::intrinsics::insert_all_intrinsics(marker, &mut into);
  amdgpu::insert_all_intrinsics(marker, &mut into);
  vk::shader::insert_all_intrinsics(marker, &mut into);
  vk::vk::insert_all_intrinsics(marker, &mut into);
}
