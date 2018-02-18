
use std::error::Error;
//use std::heap::{Alloc};

use super::region::Region;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RegionAllocator(Region);
