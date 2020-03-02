
use geobacter_core::kernel::KernelInstance;

use crate::driver_data::GetDriverData;

use rustc::mir::CustomIntrinsicMirGen;
use rustc_data_structures::sync::Lrc;

pub use rustc_help::intrinsics::*;

pub mod work_item_kill;
pub use work_item_kill::*;

pub fn insert_all_intrinsics<F, U>(marker: &U, mut into: F)
  where F: FnMut(String, Lrc<dyn CustomIntrinsicMirGen>),
        U: GetDriverData + Send + Sync + 'static,
{
  let (k, v) = GeobacterMirGen::new(SpecializationParam, marker);
  into(k, v);
  let (k, v) = GeobacterMirGen::new(CallByType, marker);
  into(k, v);
}
