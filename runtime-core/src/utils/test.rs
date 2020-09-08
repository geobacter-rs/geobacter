
use crate::context::Context;

lazy_static::lazy_static! {
  static ref CTX: Context = {
    Context::new()
      .expect("create context")
  };
}

pub fn context() -> &'static Context {
  &CTX
}
