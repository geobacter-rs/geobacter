#![allow(unused_variables)]

/// rewrites calls to either `liballoc_system` or `liballoc_jemalloc`.
/// TODO replace these aborts with writes to a AQL queue to the host
/// to request memory.

use core::alloc::Layout;
use std::intrinsics::abort;

use hsa_core::kernel::kernel_id_for;

use rustc::ty::item_path::{with_forced_absolute_paths};

use super::{Pass, PassType};

fn __rust_alloc(size: usize, align: usize) -> *mut u8 {
  unsafe { abort() };
}
fn __rust_oom(err: *const u8) -> ! {
  unsafe { abort() };
}
fn __rust_dealloc(ptr: *mut u8, size: usize, align: usize) {
  unsafe { abort() };
}
fn __rust_usable_size(layout: *const u8,
                      min: *mut usize,
                      max: *mut usize) {
  unsafe { abort() };
}
fn __rust_realloc(ptr: *mut u8,
                  old_size: usize,
                  align: usize,
                  new_size: usize) -> *mut u8 {
  unsafe { abort() };
}
fn __rust_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
  unsafe { abort() };
}
fn __rust_alloc_excess(size: usize,
                       align: usize,
                       excess: *mut usize,
                       err: *mut u8) -> *mut u8 {
  unsafe { abort() };
}
fn __rust_realloc_excess(ptr: *mut u8,
                         old_size: usize,
                         old_align: usize,
                         new_size: usize,
                         new_align: usize,
                         excess: *mut usize,
                         err: *mut u8) -> *mut u8 {
  unsafe { abort() };
}
fn __rust_grow_in_place(ptr: *mut u8,
                        old_size: usize,
                        old_align: usize,
                        new_size: usize,
                        new_align: usize) -> u8 {
  unsafe { abort() };
}
fn __rust_shrink_in_place(ptr: *mut u8,
                          old_size: usize,
                          old_align: usize,
                          new_size: usize,
                          new_align: usize) -> u8 {
  unsafe { abort() };
}
fn handle_alloc_error(_layout: Layout) -> ! {
  unsafe { abort() };
}
#[derive(Clone, Debug)]
pub struct AllocPass;

impl Pass for AllocPass {
  fn pass_type(&self) -> PassType {
    PassType::Replacer(|tcx, def_id| {
      let path = with_forced_absolute_paths(|| tcx.item_path_str(def_id) );
      info!("called on {}", path);
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

      Some(tcx.as_def_id(info).unwrap())
    })
  }
}
