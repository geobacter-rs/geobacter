
use std::iter::*;
use std::marker::PhantomData;
use std::num::NonZeroU64;
use std::ops::{Range, };

use sys;

use crate::action::Action;
use crate::data::{Data, DataKind, DataHandle, BitcodeData,
                  RelocatableData, ExecutableData, ByteData,
                  LogData, SourceData, };
use crate::error::Error;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DataSet(NonZeroU64);
impl !Sync for DataSet { }
impl !Send for DataSet { }
impl DataSet {
  pub fn new() -> Result<Self, Error> {
    let mut out = sys::amd_comgr_data_set_s {
      handle: 0,
    };

    let s = unsafe {
      sys::amd_comgr_create_data_set(&mut out as *mut _)
    };
    Error::check(s)?;

    debug_assert_ne!(out.handle, 0);

    unsafe {
      Ok(DataSet(NonZeroU64::new_unchecked(out.handle)))
    }
  }

  fn handle(&self) -> sys::amd_comgr_data_set_s {
    sys::amd_comgr_data_set_s {
      handle: self.0.get(),
    }
  }

  pub fn len<T>(&self) -> Result<usize, Error>
    where T: Data,
  {
    let mut len = 0usize;
    let s = unsafe {
      sys::amd_comgr_action_data_count(self.handle(),
                                       T::kind().to_sys(),
                                       &mut len as *mut _)
    };
    Error::check(s)?;

    Ok(len)
  }
  pub fn get<T>(&self, idx: usize) -> Result<T, Error>
    where T: Data,
  {
    let mut out = sys::amd_comgr_data_s {
      handle: 0,
    };

    let s = unsafe {
      sys::amd_comgr_action_data_get_data(self.handle(),
                                          T::kind().to_sys(),
                                          idx,
                                          &mut out as *mut _)
    };
    Error::check(s)?;

    debug_assert_ne!(out.handle, 0);

    let data = unsafe {
      DataHandle(NonZeroU64::new_unchecked(out.handle))
    };
    Ok(data.into())
  }
  pub fn iter<T>(&self) -> Result<SetIter<T>, Error>
    where T: Data,
  {
    let len = self.len::<T>()?;
    let r = Range {
      start: 0,
      end: len,
    };
    Ok(SetIter(r, self, PhantomData))
  }

  /// Note you can't remove a single piece of data among many,
  /// only whole kinds can be removed.
  pub fn add_data<D>(&mut self, data: &D) -> Result<(), Error>
    where D: Data,
  {
    let h = self.handle();
    let data_h = data.handle().handle();
    let s = unsafe {
      sys::amd_comgr_data_set_add(h, data_h)
    };
    Error::check(s)
  }
  pub fn remove_all<D>(&mut self, kind: DataKind)
    -> Result<(), Error>
  {
    let h = self.handle();
    let s = unsafe {
      sys::amd_comgr_data_set_remove(h, kind.to_sys())
    };
    Error::check(s)
  }

  pub fn perform(&self, action: &Action) -> Result<DataSet, Error> {
    let mut out = DataSet::new()?;
    self.perform_into(action, &mut out)?;
    Ok(out)
  }
  pub fn perform_into(&self, action: &Action, out: &mut DataSet)
    -> Result<(), Error>
  {
    let input_h = self.handle();
    let result_h = out.handle();
    let kind = action.kind.to_sys();
    let info = action.info.handle();
    let s = unsafe {
      sys::amd_comgr_do_action(kind, info, input_h, result_h)
    };
    Error::check(s)
  }
}

macro_rules! specialized_datatype {
  ($ty_name:ty, $len_name:ident, $get_name:ident, $iter_name:ident) => {

impl DataSet {
  pub fn $len_name(&self) -> Result<usize, Error> {
    self.len::<$ty_name>()
  }
  pub fn $get_name(&self, idx: usize) -> Result<$ty_name, Error> {
    self.get::<$ty_name>(idx)
  }
  pub fn $iter_name(&self) -> Result<SetIter<$ty_name>, Error> {
    self.iter::<$ty_name>()
  }
}

  };
}

specialized_datatype!(BitcodeData, bitcode_len, get_bitcode,
                      bitcode_iter);
specialized_datatype!(RelocatableData, relocatables_len,
                      get_relocatable, relocatable_iter);
specialized_datatype!(ExecutableData, executables_len,
                      get_executable, executable_iter);
specialized_datatype!(ByteData, bytes_len, get_byte_data,
                      byte_data_iter);
specialized_datatype!(LogData, logs_len, get_log, log_iter);
specialized_datatype!(SourceData, sources_len, get_source,
                      source_iter);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SetIter<'a, T>(Range<usize>, &'a DataSet, PhantomData<T>)
  where T: Data;

impl<'a, T> Iterator for SetIter<'a, T>
  where T: Data,
{
  type Item = Result<T, Error>;
  fn next(&mut self) -> Option<Self::Item> {
    self.0.next()
      .map(|idx| {
        self.1.get::<T>(idx)
      })
  }

  fn size_hint(&self) -> (usize, Option<usize>) { self.0.size_hint() }
}

impl<'a, T> DoubleEndedIterator for SetIter<'a, T>
  where T: Data,
{
  fn next_back(&mut self) -> Option<Self::Item> {
    self.0.next_back()
      .map(|idx| {
        self.1.get::<T>(idx)
      })
  }
}
impl<'a, T> ExactSizeIterator for SetIter<'a, T>
  where T: Data,
{ }
