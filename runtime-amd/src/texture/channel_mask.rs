use num_traits::Zero;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Y;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct N;

pub trait Bit: Default {
  const BIT: u32;
  fn test() -> bool { Self::BIT != 0 }
}

impl Bit for Y {
  const BIT: u32 = 1;
}

impl Bit for N {
  const BIT: u32 = 0;
}

pub type All = Mask<Y, Y, Y, Y>;
pub type Mrgba = All;
pub type Mrgb = Mask<Y, Y, Y, N>;
pub type Ma = Mask<N, N, N, Y>;
pub type Mr = Mask<Y, N, N, N>;
pub type Mrg = Mask<Y, Y, N, N>;
pub type Mra = Mask<Y, N, N, Y>;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Mask<R, G, B, A>(pub R, pub G, pub B, pub A)
  where R: Bit, G: Bit, B: Bit, A: Bit;

pub trait MaskDetail {
  const MASK: u32;
}

impl<R, G, B, A> MaskDetail for Mask<R, G, B, A>
  where R: Bit, G: Bit, B: Bit, A: Bit,
{
  const MASK: u32 = (R::BIT << 0) | (G::BIT << 1) | (B::BIT << 2) | (A::BIT << 3);
}

pub(super) trait UnshiftHwMasking<RawPixel>: MaskDetail {
  fn unshift(p: RawPixel) -> RawPixel;
  fn shift(p: RawPixel) -> RawPixel;
}
impl<R, G, B, A> UnshiftHwMasking<[f32; 4]> for Mask<R, G, B, A>
  where R: Bit, G: Bit, B: Bit, A: Bit,
{
  default fn unshift(src: [f32; 4]) -> [f32; 4] {
    let mut dst: [f32; 4] = [Zero::zero(); 4];

    let mut src_i = 0u8;
    let mut dst_i = 0u8;
    while dst_i < 4 {
      let bit = 1u32 << (dst_i as u32);
      if Self::MASK & bit != 0 {
        dst[dst_i as usize] = src[src_i as usize];
        src_i += 1;
      }

      dst_i += 1;
    }

    dst
  }

  default fn shift(src: [f32; 4]) -> [f32; 4] {
    let mut dst: [f32; 4] = [Zero::zero(); 4];

    let mut dst_i = 0u8;
    let mut src_i = 0u8;
    while src_i < 4 {
      let bit = 1u32 << (src_i as u32);
      if Self::MASK & bit != 0 {
        dst[dst_i as usize] = src[src_i as usize];
        dst_i += 1;
      }

      src_i += 1;
    }

    dst
  }
}
impl UnshiftHwMasking<[f32; 4]> for All {
  #[inline(always)]
  fn unshift(p: [f32; 4]) -> [f32; 4] { p }

  #[inline(always)]
  fn shift(p: [f32; 4]) -> [f32; 4] { p }
}

pub trait SingleBitMask: MaskDetail { }
impl SingleBitMask for Mask<Y, N, N, N> { }
impl SingleBitMask for Mask<N, Y, N, N> { }
impl SingleBitMask for Mask<N, N, Y, N> { }
impl SingleBitMask for Mask<N, N, N, Y> { }

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn unshift() {
    type TT = Mask<N, Y, N, Y>;
    assert_eq!(TT::unshift([1.0, 1.0, 0.0, 0.0]),
               [0.0, 1.0, 0.0, 1.0]);
  }
}
