
use crate::context::Context;

lazy_static::lazy_static! {
  static ref CTX: Context = {
    env_logger::init();
    Context::new()
      .expect("create context")
  };
}

pub fn context() -> &'static Context {
  &CTX
}
