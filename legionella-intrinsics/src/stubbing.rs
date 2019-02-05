
/// Some functions, like `std::panicking::rust_panic_hook`,
/// we need to override with an aborting stub. `panic!`
/// will likely never be supported in shaders, and probably won't
/// receive support in kernels.
/// TODO info about which stub caused an abort.
/// TODO it would be nice to be able to map DefId -> KernelId instead of
/// String -> KernelId. Need a way to translate the absolute path into a
/// DefId.

use crate::rustc::hir::def_id::{DefId, };
use crate::rustc::ty::item_path::{with_forced_absolute_paths};
use crate::rustc::ty::{TyCtxt, Instance, InstanceDef, };
use crate::rustc_data_structures::fx::{FxHashMap};

use crate::hsa_core::kernel::{KernelId, kernel_id_for, };

use self::stubs::*;
use crate::{DefIdFromKernelId, };

pub struct Stubber {
  builtins: bool,
  stubs: FxHashMap<String, KernelId>,
}

impl Stubber {
  pub fn map_instance<'tcx>(&self,
                            tcx: TyCtxt<'_, '_, 'tcx>,
                            kid_did: &dyn DefIdFromKernelId,
                            inst: Instance<'tcx>)
    -> Instance<'tcx>
  {
    Instance {
      def: match inst.def {
        InstanceDef::Item(did) => {
          let did = self.map_did(tcx, kid_did, did);
          InstanceDef::Item(did)
        },
        def => def,
      },
      substs: inst.substs,
    }
  }
  pub fn map_did(&self,
                 tcx: TyCtxt<'_, '_, '_>,
                 kid_did: &dyn DefIdFromKernelId,
                 did: DefId)
    -> DefId
  {
    let new_did = self.map_did_(tcx, kid_did, did);

    if new_did != did {
      debug!("stubbing {:?} with {:?}", did, new_did);
    }

    new_did
  }
  fn map_did_(&self,
              tcx: TyCtxt<'_, '_, '_>,
              kid_did: &dyn DefIdFromKernelId,
              did: DefId)
    -> DefId
  {
    let path = with_forced_absolute_paths(|| tcx.item_path_str(did) );

    debug!("did {:?} path {}", did, path);

    // let an entry in stubs override our builtin stubs (see `self::stubs`):
    if let Some(&kid) = self.stubs.get(&path) {
      return kid_did.convert_kernel_id(kid)
        .unwrap_or(did);
    }

    if !self.builtins { return did; }

    macro_rules! check {
      ($path:expr, $kid_did:expr, $abs_path:expr, $stub:expr) => {
        if $path == $abs_path {
          let id = kernel_id_for(&$stub);
          let stub_did = $kid_did.convert_kernel_id(id)
            .unwrap_or(did);
          return stub_did;
        }
      };
    }
    macro_rules! check_alloc {
      (@detail $path:expr, $kid_did:expr, $flavor:ident, $stub:ident) => {
        {
          let check_path = concat!("alloc::", stringify!($flavor), "::::",
                                   stringify!($stub));
          check!($path, $kid_did, check_path, $stub);
        }
      };

      ($path:expr, $kid_did:expr, $($stub:ident),*) => {$(
        check_alloc!(@detail $path, $kid_did, heap, $stub);
        check_alloc!(@detail $path, $kid_did, alloc, $stub);
      )*};
    }

    check_alloc!(path, kid_did,
                 __rust_alloc,
                 __rust_oom,
                 __rust_dealloc,
                 __rust_usable_size,
                 __rust_realloc,
                 __rust_alloc_zeroed,
                 __rust_alloc_excess,
                 __rust_realloc_excess,
                 __rust_grow_in_place);
    check!(path, kid_did, "alloc::alloc::handle_alloc_error::::oom_impl",
           handle_alloc_error);

    check!(path, kid_did, "core::panicking::panic_fmt::::panic_impl",
           rust_begin_panic);
    check!(path, kid_did, "std::panicking::rust_panic_with_hook",
           rust_panic_with_hook);
    check!(path, kid_did, "core::panicking::panic_fmt",
           rust_panic_fmt);
    check!(path, kid_did, "core::panicking::panic", rust_panic);
    check!(path, kid_did, "panic_unwind::imp::rust_eh_personality",
           rust_eh_personality);
    return did;
  }
}

impl Default for Stubber {
  fn default() -> Self {
    Stubber {
      builtins: true,
      stubs: Default::default(),
    }
  }
}

mod stubs {
  #![allow(unused_variables)]

  use std::fmt;
  use std::intrinsics::abort;
  use core::alloc::Layout;
  use core::panic::{PanicInfo, BoxMeUp, };

  // ALLOC
  pub fn __rust_alloc(size: usize, align: usize) -> *mut u8 {
    unsafe { abort() };
  }
  pub fn __rust_oom(err: *const u8) -> ! {
    unsafe { abort() };
  }
  pub fn __rust_dealloc(ptr: *mut u8, size: usize, align: usize) {
    unsafe { abort() };
  }
  pub fn __rust_usable_size(layout: *const u8,
                            min: *mut usize,
                            max: *mut usize) {
    unsafe { abort() };
  }
  pub fn __rust_realloc(ptr: *mut u8,
                        old_size: usize,
                        align: usize,
                        new_size: usize) -> *mut u8 {
    unsafe { abort() };
  }
  pub fn __rust_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    unsafe { abort() };
  }
  pub fn __rust_alloc_excess(size: usize,
                             align: usize,
                             excess: *mut usize,
                             err: *mut u8) -> *mut u8 {
    unsafe { abort() };
  }
  pub fn __rust_realloc_excess(ptr: *mut u8,
                               old_size: usize,
                               old_align: usize,
                               new_size: usize,
                               new_align: usize,
                               excess: *mut usize,
                               err: *mut u8) -> *mut u8 {
    unsafe { abort() };
  }
  pub fn __rust_grow_in_place(ptr: *mut u8,
                              old_size: usize,
                              old_align: usize,
                              new_size: usize,
                              new_align: usize) -> u8 {
    unsafe { abort() };
  }
  pub fn __rust_shrink_in_place(ptr: *mut u8,
                                old_size: usize,
                                old_align: usize,
                                new_size: usize,
                                new_align: usize) -> u8 {
    unsafe { abort() };
  }
  pub fn handle_alloc_error(_layout: Layout) -> ! {
    unsafe { abort() };
  }

  // PANIC
  #[inline(always)]
  pub fn rust_panic_with_hook(_payload: &mut dyn BoxMeUp,
                              _message: Option<&fmt::Arguments>,
                              _file_line_col: &(&str, u32, u32))
    -> !
  {
    unsafe { abort() }
  }
  #[inline(always)]
  pub fn rust_begin_panic(_: &PanicInfo) -> ! {
    unsafe { abort() }
  }
  #[inline(always)]
  pub fn rust_panic(_expr_file_line_col: &(&'static str, &'static str,
                                           u32, u32))
    -> !
  {
    unsafe { abort() }
  }
  #[inline(always)]
  pub fn rust_panic_fmt(_fmt: fmt::Arguments,
                        _file_line_col: &(&'static str, u32, u32))
    -> !
  {
    unsafe { abort() }
  }
  // Never called.
  pub fn rust_eh_personality() {}
}
