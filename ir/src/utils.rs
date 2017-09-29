
use std::hash::{Hash, Hasher};
use num_traits::Float;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct HashableFloat<T>(pub T)
  where T: Float;

impl<T> Hash for HashableFloat<T>
  where T: Float,
{
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.0.integer_decode().hash(state)
  }
}
