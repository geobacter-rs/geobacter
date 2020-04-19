decl! {
  #[repr(u32)]
  pub enum CoordMode {
    /// Coordinates are used to directly address an image element.
    Unnormalized = 0,
    /// Coordinates are scaled by the image dimension size before being used to address an
    /// image element.
    Normalized = 1,
  }
  pub trait CoordModeDetail { }
}
