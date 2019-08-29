
//! This is the full Geobacter Rustc driver.
//! It includes intrinsics which themselves depend on intrinsics
//! present in `geobacter-core`.

#![feature(rustc_private)]

extern crate geobacter_intrinsics_common as common;
extern crate geobacter_amdgpu_intrinsics as amdgpu;
extern crate geobacter_vk_intrinsics as vk;
extern crate rustc_intrinsics;

extern crate geobacter_core;
extern crate rustc;
extern crate rustc_codegen_utils;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_metadata;
extern crate syntax;
extern crate syntax_pos;

use self::rustc::mir::{CustomIntrinsicMirGen, };
use self::rustc::ty::{TyCtxt, };
use self::rustc_data_structures::sync::{Lrc, };

use crate::common::DefIdFromKernelId;

pub fn main() {
  rustc_intrinsics::main(|gen| {
    insert_all_intrinsics(&GeneratorDefIdKernelId,
                          |k, v| {
                            let inserted = gen.intrinsics.insert(k.clone(), v);
                            assert!(inserted.is_none(), "key: {}", k);
                          });
  });
}

pub struct GeneratorDefIdKernelId;
impl common::DefIdFromKernelId for GeneratorDefIdKernelId {
  fn get_cstore(&self) -> &rustc_metadata::cstore::CStore {
    rustc_intrinsics::generators().cstore()
  }
}
impl common::GetDefIdFromKernelId for GeneratorDefIdKernelId {
  fn with_self<F, R>(_tcx: TyCtxt<'_>, f: F) -> R
    where F: FnOnce(&dyn DefIdFromKernelId) -> R,
  {
    f(&GeneratorDefIdKernelId)
  }
}

/// Call `into` for every intrinsic in every platform intrinsic crate.
pub fn insert_all_intrinsics<F, U>(marker: &U, mut into: F)
  where F: FnMut(String, Lrc<dyn CustomIntrinsicMirGen>),
        U: common::GetDefIdFromKernelId + Send + Sync + 'static,
{
  amdgpu::insert_all_intrinsics(marker, &mut into);
  vk::shader::insert_all_intrinsics(marker, &mut into);
  vk::vk::insert_all_intrinsics(marker, &mut into);
}
