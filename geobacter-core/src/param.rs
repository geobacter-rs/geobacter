
//! Support for programmatic "kernel" parameters for affecting codegen. These functions
//! are *constant*, though they may not be used in types (ie as a configurable array
//! length). Instead these are intended to allow LLVM to specialize via constant
//! propagation.
//!

extern "rust-intrinsic" {
  /// F is just a marker here. It won't be called.
  fn __geobacter_specialization_param<F, R>(_: &F) -> &'static [R]
    where F: Fn() -> R;
}

/// F is just a marker here. It won't be called. Currently, f can't be a closure, but that
/// restriction isn't strictly required here.
/// THIS ASSUMES IDENTICAL HOST/DEVICE ENDIANNESS. Endianness swapping will be handled
/// automatically Later(TM), but that will almost certainly be a breaking change.
pub fn get_spec_param<F, R>(f: &F) -> Option<&'static R>
  where F: Fn() -> R,
{
  unsafe {
    __geobacter_specialization_param::<F, R>(f)
      .get(0)
  }
}
