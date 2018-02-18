
use {ConstVal, Function};
use rustc_wrappers::DefIdDef;

use super::{Ty, };

newtype_idx!(#[derive(Deserialize, Serialize)] pub struct Trait => "trait");

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
#[derive(Serialize, Deserialize)]
pub enum AssociatedItemContainer {
  Trait(Trait),
  // TODO
  Impl,
}

#[derive(Debug, Clone, Hash)]
#[derive(Serialize, Deserialize)]
pub enum AssociatedItem {
  Const {
    id: ConstVal,
    name: String,
    container: AssociatedItemContainer,
  },
  Method {
    id: Function,
    name: String,
    container: AssociatedItemContainer,
  },
  Type {
    id: Ty,
    name: String,
  },
}

#[derive(Debug, Clone, Hash)]
#[derive(Serialize, Deserialize)]
pub struct TraitData {
  def_id: DefIdDef,
  super_traits: Vec<Trait>,
  associated_items: Vec<AssociatedItem>,
}
