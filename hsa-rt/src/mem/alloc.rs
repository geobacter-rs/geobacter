
use super::region::Region;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RegionAllocator(Region);

/*pub trait Alloc {
  type Pointer: Copy + Eq + Ord;

  fn alloc
}*/
