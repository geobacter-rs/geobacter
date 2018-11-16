
use std::alloc::{Layout, };
use std::ffi::c_void;
use std::mem::{size_of, transmute, align_of_val, size_of_val, };
use std::ops::{Deref, DerefMut, Index, IndexMut, };
use std::ptr::{NonNull, };
use std::slice::{from_raw_parts, from_raw_parts_mut, SliceIndex, };
use std::sync::{Arc, atomic::AtomicUsize, };

use ffi;
use agent::Agent;
use error::Error;
use utils::set_data_ptr;

use nd;

pub mod amd;

