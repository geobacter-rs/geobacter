decl! {
  #[repr(u32)]
  pub enum AddrMode {
    /// Out-of-range coordinates are not handled.
    Undefined = 0,
    /// Clamp out-of-range coordinates to the image edge.
    ClampToEdge = 1,
    /// Clamp out-of-range coordinates to the image border color.
    ClampToBorder = 2,
    /// Wrap out-of-range coordinates back into the valid coordinate range so the image appears
    /// as repeated tiles.
    Repeat = 3,
    /// Mirror out-of-range coordinates back into the valid coordinate range so the image
    /// appears as repeated tiles with every other tile a reflection.
    MirroredRepeat = 4,
  }
  pub trait AddrModeDetail { }
}
