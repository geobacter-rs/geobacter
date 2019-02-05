#![allow(unused_variables)]

/// rewrites calls to either `liballoc_system` or `liballoc_jemalloc`.
/// TODO replace these aborts with writes to a AQL queue to the host
/// to request memory.

use core::alloc::Layout;
use std::intrinsics::abort;

use hsa_core::kernel::kernel_id_for;

use rustc::ty::item_path::{with_forced_absolute_paths};

use super::{Pass, PassType};


#[derive(Clone, Debug)]
pub struct AllocPass;

impl Pass for AllocPass {
  fn pass_type(&self) -> PassType {
    PassType::Replacer(|tcx, dd, def_id| {
      let path = with_forced_absolute_paths(|| tcx.item_path_str(def_id) );
      let info = match &path[..] {
        "alloc::heap::::__rust_alloc" |
        "alloc::alloc::::__rust_alloc" => {
          kernel_id_for(&__rust_alloc)
        },
        "alloc::heap::::__rust_alloc_zeroed" |
        "alloc::alloc::::__rust_alloc_zeroed" => {
          kernel_id_for(&__rust_alloc_zeroed)
        },
        "alloc::heap::::__rust_realloc" |
        "alloc::alloc::::__rust_realloc" => {
          kernel_id_for(&__rust_realloc)
        },
        "alloc::heap::::__rust_oom" |
        "alloc::alloc::::__rust_oom" => {
          kernel_id_for(&__rust_oom)
        },
        "alloc::heap::::__rust_usable_size" |
        "alloc::alloc::::__rust_usable_size" => {
          kernel_id_for(&__rust_usable_size)
        },
        "alloc::heap::::__rust_dealloc" |
        "alloc::alloc::::__rust_dealloc" => {
          kernel_id_for(&__rust_dealloc)
        },
        "alloc::heap::::__rust_alloc_excess" |
        "alloc::alloc::::__rust_alloc_excess" => {
          kernel_id_for(&__rust_alloc_excess)
        },
        "alloc::heap::::__rust_realloc_excess" |
        "alloc::alloc::::__rust_realloc_excess" => {
          kernel_id_for(&__rust_realloc_excess)
        },
        "alloc::heap::::__rust_grow_in_place" |
        "alloc::alloc::::__rust_grow_in_place" => {
          kernel_id_for(&__rust_grow_in_place)
        },
        "alloc::heap::::__rust_shrink_in_place" |
        "alloc::alloc::::__rust_shrink_in_place" => {
          kernel_id_for(&__rust_shrink_in_place)
        },
        "alloc::alloc::handle_alloc_error::::oom_impl" => {
          kernel_id_for(&handle_alloc_error)
        },
        _ => { return None; },
      };

      Some(dd.as_def_id(info).unwrap())
    })
  }
}
