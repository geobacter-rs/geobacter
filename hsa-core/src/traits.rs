
use std::error::Error;
use std::fmt::Debug;

use unit::*;

pub type EmptyResult = Result<(), Box<Error>>;

pub trait RuntimePlatformPrimitiveVisitor {
  fn visit_u8(&mut self, v: u8) -> EmptyResult;
  fn visit_u16(&mut self, v: u16) -> EmptyResult;
  fn visit_u32(&mut self, v: u32) -> EmptyResult;
  fn visit_u64(&mut self, v: u64) -> EmptyResult;

  fn visit_i8(&mut self, v: i8) -> EmptyResult;
  fn visit_i16(&mut self, v: i16) -> EmptyResult;
  fn visit_i32(&mut self, v: i32) -> EmptyResult;
  fn visit_i64(&mut self, v: i64) -> EmptyResult;

  fn visit_f16(&mut self, v: Real<N16<()>>) -> EmptyResult;
  fn visit_f24(&mut self, v: Real<N24<()>>) -> EmptyResult;
  fn visit_f32(&mut self, v: Real<N32<()>>) -> EmptyResult;
  fn visit_f64(&mut self, v: Real<N64<()>>) -> EmptyResult;
}

pub trait RuntimePlatformDataVisitor: RuntimePlatformPrimitiveVisitor {
  // TODO: signed strides? ndarray doesn't use isize, so for now I'm not either.
  fn visit_buffer_u8(&mut self, buffer: &[u8], start_offset: usize,
                     strides: &[usize], row_major: bool,
                     tag: Option<&mut u64>) -> EmptyResult;
}

#[hsa_lang_item = "numa-send"]
pub trait NumaSend: Debug {
  fn report_internal_buffers<'a>(&'a self, into: &mut RuntimePlatformDataVisitor);
}

/*macro_rules! impl_empty_numa_send {
  ($($ty:ty),*) => {
impl NumaSend for $ty {
  fn report_internal_buffers<'a>(&'a self, into: &mut RuntimePlatformDataVisitor) { }
}
  }
}

impl_empty_numa_send!(u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);

impl_empty_numa_send!(UInt<N8>, UInt<N16>, UInt<N32>, UInt<N64>);
impl_empty_numa_send!(Int<N8>, Int<N16>, Int<N32>, Int<N64>);
impl_empty_numa_send!(Real<N8>, Real<N16>, Real<N24>, Real<N32>,
                      Real<N64>);
*/
