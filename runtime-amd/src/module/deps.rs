
use std::rc::{Rc, };
use std::sync::{Arc, };

use crate::module::CallError;
use crate::signal::{DeviceConsumable, DeviceSignal, GlobalSignal, };

/// This is unsafe because you must ensure the proper dep signals are registered!
/// You should probably just use the `GeobacterDeps` derive macro to implement this.
pub unsafe trait Deps {
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>;
}

unsafe impl Deps for DeviceSignal {
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    f(self)
  }
}
unsafe impl Deps for GlobalSignal {
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    f(self)
  }
}
unsafe impl<T> Deps for Option<T>
  where T: Deps,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    if let &Some(ref v) = self {
      v.iter_deps(f)?;
    }
    Ok(())
  }
}

unsafe impl<'b, T> Deps for &'b T
  where T: Deps + ?Sized,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    (&**self).iter_deps(f)
  }
}

macro_rules! impl_prim {
  ($($prim:ty,)*) => {$(

unsafe impl Deps for $prim {
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}

  )*};
}
impl_prim!(i8, u8, i16, u16, i32, u32, i64, u64, i128, u128, f32, f64,
           (), );
unsafe impl<T> Deps for *const T
  where T: ?Sized,
{
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}
unsafe impl<T> Deps for *mut T
  where T: ?Sized,
{
  fn iter_deps<'a>(&'a self, _: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    Ok(())
  }
}

unsafe impl<T> Deps for [T]
  where T: Deps,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    for v in self.iter() {
      v.iter_deps(f)?;
    }
    Ok(())
  }
}
unsafe impl<T> Deps for Rc<T>
  where T: Deps + ?Sized,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    (&**self).iter_deps(f)
  }
}
unsafe impl<T> Deps for Arc<T>
  where T: Deps + ?Sized,
{
  fn iter_deps<'a>(&'a self, f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), CallError>)
    -> Result<(), CallError>
  {
    (&**self).iter_deps(f)
  }
}
