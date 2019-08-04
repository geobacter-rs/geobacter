
extern "rust-intrinsic" {
  /// Kills the current workitem/thread.
  pub fn __legionella_kill() -> !;
}
