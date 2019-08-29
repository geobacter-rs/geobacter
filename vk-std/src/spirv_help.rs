
use super::is_spirv;

pub fn g<T>(v: &'static T) -> T
  where T: Default + Copy,
{
  if is_spirv() {
    *v
  } else {
    Default::default()
  }
}
