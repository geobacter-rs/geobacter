
extern "rust-intrinsic" {
  /// Kills the current workitem/thread.
  pub fn __geobacter_kill() -> !;
  /// Call a function by it's type. No closures.
  pub fn __geobacter_call_by_type<F, A, R>(args: A) -> R
    where F: Fn<A, Output = R>;
}
