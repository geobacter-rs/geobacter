
use std::borrow::Cow;

#[derive(Serialize, Deserialize, Debug, Hash, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct GlobalId(u64);

#[derive(Serialize, Deserialize, Debug, Hash, Clone)]
pub struct Global {
  pub name: Cow<'static, str>,
}
