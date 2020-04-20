
use std::ffi::CStr;
use std::iter::*;
use std::ops::{Range, Deref, };
use std::ptr;

use sys;

use crate::error::Error;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SupportedIsaIter(Range<usize>);
impl SupportedIsaIter {
  pub fn new() -> Result<Self, Error> {
    let mut count = 0;
    let s = unsafe {
      sys::amd_comgr_get_isa_count(&mut count)
    };
    Error::check(s)?;

    Ok(SupportedIsaIter(Range {
      start: 0,
      end: count as _,
    }))
  }
}

impl Iterator for SupportedIsaIter {
  type Item = Isa;
  fn next(&mut self) -> Option<Self::Item> {
    self.0.next().map(Isa::get)
  }

  fn size_hint(&self) -> (usize, Option<usize>) { self.0.size_hint() }
}
impl DoubleEndedIterator for SupportedIsaIter {
  fn next_back(&mut self) -> Option<Self::Item> {
    self.0.next_back().map(Isa::get)
  }
}
impl ExactSizeIterator for SupportedIsaIter { }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Isa(&'static CStr);
impl Isa {
  fn get(idx: usize) -> Isa {
    let mut ptr = ptr::null();
    let s = unsafe {
      sys::amd_comgr_get_isa_name(idx as _, &mut ptr as *mut _)
    };
    Error::check(s)
      .expect("amd_comgr_get_isa_name should not fail for us");

    unsafe {
      Isa(CStr::from_ptr(ptr))
    }
  }
}
impl Deref for Isa {
  type Target = CStr;
  fn deref(&self) -> &CStr { self.0 }
}
