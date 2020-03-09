
use std::mem::{size_of, transmute, };

extern "rust-intrinsic" {
  fn __geobacter_amdgpu_update_dpp2<T>(old: T, src: T, dpp_ctrl: i32,
                                       row_mask: i32, bank_mask: i32,
                                       bound_ctrl: bool) -> T;
}

// From the LLVM AMDGPU target machine:
// enum DppCtrl : unsigned {
//   QUAD_PERM_FIRST   = 0,
//   QUAD_PERM_ID      = 0xE4, // identity permutation
//   QUAD_PERM_LAST    = 0xFF,
//   DPP_UNUSED1       = 0x100,
//   ROW_SHL0          = 0x100,
//   ROW_SHL_FIRST     = 0x101,
//   ROW_SHL_LAST      = 0x10F,
//   DPP_UNUSED2       = 0x110,
//   ROW_SHR0          = 0x110,
//   ROW_SHR_FIRST     = 0x111,
//   ROW_SHR_LAST      = 0x11F,
//   DPP_UNUSED3       = 0x120,
//   ROW_ROR0          = 0x120,
//   ROW_ROR_FIRST     = 0x121,
//   ROW_ROR_LAST      = 0x12F,
//   WAVE_SHL1         = 0x130,
//   DPP_UNUSED4_FIRST = 0x131,
//   DPP_UNUSED4_LAST  = 0x133,
//   WAVE_ROL1         = 0x134,
//   DPP_UNUSED5_FIRST = 0x135,
//   DPP_UNUSED5_LAST  = 0x137,
//   WAVE_SHR1         = 0x138,
//   DPP_UNUSED6_FIRST = 0x139,
//   DPP_UNUSED6_LAST  = 0x13B,
//   WAVE_ROR1         = 0x13C,
//   DPP_UNUSED7_FIRST = 0x13D,
//   DPP_UNUSED7_LAST  = 0x13F,
//   ROW_MIRROR        = 0x140,
//   ROW_HALF_MIRROR   = 0x141,
//   BCAST15           = 0x142,
//   BCAST31           = 0x143,
//   DPP_UNUSED8_FIRST = 0x144,
//   DPP_UNUSED8_LAST  = 0x14F,
//   ROW_SHARE_FIRST   = 0x150,
//   ROW_SHARE_LAST    = 0x15F,
//   ROW_XMASK_FIRST   = 0x160,
//   ROW_XMASK_LAST    = 0x16F,
//   DPP_LAST          = ROW_XMASK_LAST
// };
//
// enum DppFiMode {
//   DPP_FI_0  = 0,
//   DPP_FI_1  = 1,
//   DPP8_FI_0 = 0xE9,
//   DPP8_FI_1 = 0xEA,
// };

/// Do not implement this on your own.
/// Except for `self` and `default`, all arguments must be constants.
pub unsafe trait Dpp: Copy + Sized + 'static {
  /// You probably shouldn't call this directly. dpp_ctrl has a special meaning
  /// to llvm.amdgcn.update.dpp intrinsic.
  #[doc(hidden)]
  fn update_dpp(self, default: Self, dpp_ctrl: i32,
                row_mask: i32, bank_mask: i32,
                bound_ctrl: bool) -> Self;

  /// `perm` selects the element in a single bank.
  /// So only values 0-3. This function masks each perm element by 0x3.
  #[inline(always)]
  fn quad_perm(self, default: Self, perm: [u8; 4],
               row_mask: i32, bank_mask: i32,
               bound_ctrl: bool) -> Self {
    let perm = (perm[0] & 0x3) << 6
    | (perm[1] & 0x3) << 4
    | (perm[2] & 0x3) << 2
    | (perm[3] & 0x3);

    self.update_dpp(default, perm as _,
                    row_mask, bank_mask,
                    bound_ctrl)
  }
  #[inline(always)]
  fn row_shl(self, default: Self, count: u8,
             row_mask: i32, bank_mask: i32,
             bound_ctrl: bool) -> Self {
    if count != 0 {
      let count = count.min(15) as i32;
      self.update_dpp(default, 0x100 + count,
                      row_mask, bank_mask,
                      bound_ctrl)
    } else {
      self
    }
  }
  #[inline(always)]
  fn row_shr(self, default: Self, count: u8,
             row_mask: i32, bank_mask: i32,
             bound_ctrl: bool) -> Self {
    if count != 0 {
      let count = count.min(15) as i32;
      self.update_dpp(default, 0x110 + count,
                      row_mask, bank_mask,
                      bound_ctrl)
    } else {
      self
    }
  }
  #[inline(always)]
  fn row_ror(self, default: Self, count: u8,
             row_mask: i32, bank_mask: i32,
             bound_ctrl: bool) -> Self {
    if count != 0 {
      let count = count.min(15) as i32;
      self.update_dpp(default, 0x120 + count,
                      row_mask, bank_mask,
                      bound_ctrl)
    } else {
      self
    }
  }
  #[inline(always)]
  fn wave_shl1(self, default: Self, row_mask: i32, bank_mask: i32,
               bound_ctrl: bool) -> Self {
    self.update_dpp(default, 0x130,
                    row_mask, bank_mask,
                    bound_ctrl)
  }
  #[inline(always)]
  fn wave_rol1(self, default: Self, row_mask: i32, bank_mask: i32,
               bound_ctrl: bool) -> Self {
    self.update_dpp(default, 0x134,
                    row_mask, bank_mask,
                    bound_ctrl)
  }
  #[inline(always)]
  fn wave_shr1(self, default: Self, row_mask: i32, bank_mask: i32,
               bound_ctrl: bool) -> Self {
    self.update_dpp(default, 0x138,
                    row_mask, bank_mask,
                    bound_ctrl)
  }
  #[inline(always)]
  fn wave_ror1(self, default: Self, row_mask: i32, bank_mask: i32,
               bound_ctrl: bool) -> Self {
    self.update_dpp(default, 0x13C,
                    row_mask, bank_mask,
                    bound_ctrl)
  }
}

macro_rules! impl_dpp {
  ($(($this:ty, $ity:ty, $($cast:ty,)? ),)*) => ($(

unsafe impl Dpp for $this {
  #[inline(always)]
  fn update_dpp(self, default: Self, dpp_ctrl: i32,
                row_mask: i32, bank_mask: i32,
                bound_ctrl: bool) -> Self {
    unsafe {
      let src: $ity = transmute(self);
      let old: $ity = transmute(default);
      let r = __geobacter_amdgpu_update_dpp2(old $(as $cast)?, src $(as $cast)?,
                                             dpp_ctrl, row_mask,
                                             bank_mask, bound_ctrl);

      transmute(r as $ity)
    }
  }
}

  )*)
}
macro_rules! impl_dpp_split {
  ($(($this:ty, ),)*) => ($(

unsafe impl Dpp for $this {
  #[inline(always)]
  fn update_dpp(self, default: Self, dpp_ctrl: i32,
                row_mask: i32, bank_mask: i32,
                bound_ctrl: bool) -> Self {
    unsafe {
      let mut old: [u32; size_of::<Self>() / size_of::<u32>()] =
        transmute(self);
      let src: [u32; size_of::<Self>() / size_of::<u32>()] =
        transmute(default);
      let mut iter = 0u8;
      // no for _ in 0..len { } here. std::mem::swap poisons many optimizations.
      while iter < (size_of::<Self>() / size_of::<u32>()) as u8 {
        let t = &mut (*old.as_mut_ptr().add(iter as usize));
        let src = *src.as_ptr().add(iter as usize);
        *t = t.update_dpp(src, dpp_ctrl, row_mask,
                          bank_mask, bound_ctrl);
        iter += 1;
      }
      transmute(old)
    }
  }
}

  )*)
}
impl_dpp! {
  (i8, u8, u32, ),
  (u8, u8, u32, ),
  (i16, u16, u32, ),
  (u16, u16, u32, ),
  (f32, u32, ),
  (i32, u32, ),
  (u32, u32, ),
}
impl_dpp_split! {
  (f64, ),
  (i64, ),
  (u64, ),
  (i128, ),
  (u128, ),
  (isize, ),
  (usize, ),
}
