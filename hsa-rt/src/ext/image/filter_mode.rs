decl! {
  #[repr(u32)]
  pub enum FilterMode {
    /// Filter to the image element nearest (in Manhattan distance) to the specified coordinate.
    Nearest = 0,
    /// Filter to the image element calculated by combining the elements in a 2x2 square block
    /// or 2x2x2 cube block around the specified coordinate. The elements are combined using
    /// linear interpolation.
    Linear = 1,
  }
  pub trait FilterModeDetail { }
}
