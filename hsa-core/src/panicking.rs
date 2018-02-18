/// This file contains functions which are used to
/// replace specific panicking functions in libstd.

use std::fmt;

#[inline(never)]
#[cold]
#[hsa_lang_item = "panic"]
pub extern fn rust_begin_panic(msg: fmt::Arguments,
                               file: &'static str,
                               line: u32,
                               col: u32) -> ! {
  // For now, we just trap, terminating the kernel.
  // TODO Write arguments to host accessible memory.
  use std::intrinsics::abort;
  unsafe {
    abort()
  }
}

#[inline(never)] #[cold] // avoid code bloat at the call sites as much as possible
#[hsa_lang_item = "libstd_begin_panic"]
pub fn begin_panic<M>(msg: M, 
                      file_line_col: &(&'static str, u32, u32)) -> !
  where M: Any + Send,
{
  // For now, we just trap, terminating the kernel.
  // TODO Write arguments to host accessible memory.
  use std::intrinsics::abort;
  unsafe {
    abort()
  }
}