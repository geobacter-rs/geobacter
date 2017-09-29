#![feature(unboxed_closures, fn_traits)]

#[cfg(feature = "serial")]
#[macro_use] extern crate serde_derive;
#[cfg(feature = "serial")]
extern crate serde;

use std::marker::PhantomData;

use std::fmt::{self, Debug, Formatter};
use std::vec::IntoIter;
use std::slice::{Iter, IterMut};
use std::iter::{Enumerate, Map, Extend, FromIterator, IntoIterator};
use std::ops::{Range, Index, IndexMut};

#[macro_export]
macro_rules! newtype_idx {
  ($(#[$extra:meta])* pub struct $ty_name:ident => $debug_name:expr) => {
    $(#[$extra])*
    #[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
    pub struct $ty_name(pub usize);
    impl $crate::Idx for $ty_name {
      fn new(v: usize) -> Self {
        $ty_name(v)
      }
      fn index(self) -> usize {
        self.0
      }
    }
  }
}

pub trait Idx: Copy + Eq + Debug + 'static {
  fn new(v: usize) -> Self;
  fn index(self) -> usize;
}

pub type EnumeratedIter<I, IT> = Map<Enumerate<IT>, IntoIdx<I>>;

#[cfg_attr(feature = "serial", derive(Deserialize, Serialize))]
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct IndexVec<I, T>
  where I: Idx,
{
  vec: Vec<T>,
  #[cfg_attr(feature = "serial", serde(skip))]
  _marker: PhantomData<Fn(&I)>,
}

impl<I, T> IndexVec<I, T>
  where I: Idx,
{
  pub fn new() -> Self { Default::default() }
  pub fn with_capacity(cap: usize) -> Self {
    IndexVec {
      vec: Vec::with_capacity(cap),
      _marker: PhantomData,
    }
  }
  pub fn from_elem<S>(elem: T, universe: &IndexVec<I, S>) -> Self
    where T: Clone,
  {
    IndexVec {
      vec: vec![elem; universe.len()],
      _marker: PhantomData,
    }
  }
  pub fn from_elem_n(elem: T, n: usize) -> Self
    where T: Clone,
  {
    IndexVec {
      vec: vec![elem; n],
      _marker: PhantomData,
    }
  }
  pub fn len(&self) -> usize { self.vec.len() }
  pub fn is_empty(&self) -> bool { self.vec.is_empty() }
  pub fn into_iter(self) -> IntoIter<T> { self.vec.into_iter() }
  pub fn into_iter_enumerated(self) -> EnumeratedIter<I, IntoIter<T>> {
    self.vec.into_iter().enumerate().map(Default::default())
  }
  pub fn iter(&self) -> Iter<T> { self.vec.iter() }
  pub fn iter_mut(&mut self) -> IterMut<T> { self.vec.iter_mut() }
  pub fn iter_enumerated(&self) -> EnumeratedIter<I, Iter<T>> { self.vec.iter().enumerate().map(Default::default()) }
  pub fn iter_enumerated_mut(&mut self) -> EnumeratedIter<I, IterMut<T>> { self.vec.iter_mut().enumerate().map(Default::default()) }
  pub fn indices(&self) -> Map<Range<usize>, IntoIdx<I>> { (0..self.len()).map(Default::default()) }
  pub fn last_idx(&self) -> Option<I> {
    if self.is_empty() {
      None
    } else {
      Some(I::new(self.len() - 1))
    }
  }
  pub fn shrink_to_fit(&mut self) { self.vec.shrink_to_fit() }
  pub fn swap(&mut self, l: I, r: I) { self.vec.swap(l.index(), r.index()) }
  pub fn truncate(&mut self, s: usize) { self.vec.truncate(s) }
  pub fn get(&self, i: I) -> Option<&T> { self.vec.get(i.index()) }
  pub fn get_mut(&mut self, i: I) -> Option<&mut T> { self.vec.get_mut(i.index()) }

  pub fn resize(&mut self, s: usize, v: T)
    where T: Clone,
  {
    self.vec.resize(s, v);
  }
  pub fn binary_search(&self, v: &T) -> Result<I, I>
    where T: Ord,
  {
    self.vec.binary_search(v)
      .map(I::new)
      .map_err(I::new)
  }
  pub fn push(&mut self, d: T) -> I {
    let idx = I::new(self.len());
    self.vec.push(d);
    idx
  }
}

impl<I, T> Default for IndexVec<I, T>
  where I: Idx,
{
  fn default() -> Self {
    IndexVec {
      vec: vec![],
      _marker: PhantomData,
    }
  }
}

impl<I, T> Index<I> for IndexVec<I, T>
  where I: Idx,
{
  type Output = T;
  fn index(&self, i: I) -> &T { &self.vec[i.index()] }
}
impl<I, T> IndexMut<I> for IndexVec<I, T>
  where I: Idx,
{
  fn index_mut(&mut self, i: I) -> &mut T { &mut self.vec[i.index()] }
}
impl<I, T> Extend<T> for IndexVec<I, T>
  where I: Idx,
{
  fn extend<IT>(&mut self, iter: IT)
    where IT: IntoIterator<Item = T>,
  {
    self.vec.extend(iter)
  }
}
impl<I, T> FromIterator<T> for IndexVec<I, T>
  where I: Idx,
{
  fn from_iter<IT>(iter: IT) -> Self
    where IT: IntoIterator<Item = T>,
  {
    IndexVec {
      vec: FromIterator::from_iter(iter),
      _marker: PhantomData,
    }
  }
}
impl<I, T> IntoIterator for IndexVec<I, T>
  where I: Idx,
{
  type Item = T;
  type IntoIter = IntoIter<T>;

  fn into_iter(self) -> IntoIter<T> { self.into_iter() }
}
impl<'a, I, T> IntoIterator for &'a IndexVec<I, T>
  where I: Idx,
{
  type Item = &'a T;
  type IntoIter = Iter<'a, T>;

  fn into_iter(self) -> Self::IntoIter {
    self.iter()
  }
}
impl<'a, I, T> IntoIterator for &'a mut IndexVec<I, T>
  where I: Idx,
{
  type Item = &'a mut T;
  type IntoIter = IterMut<'a, T>;

  fn into_iter(self) -> Self::IntoIter {
    self.iter_mut()
  }
}

impl<I, T> Debug for IndexVec<I, T>
  where I: Idx,
        T: Debug,
{
  fn fmt(&self, f: &mut Formatter) -> fmt::Result {
    Debug::fmt(&self.vec, f)
  }
}

pub struct IntoIdx<I>(PhantomData<Fn(&I)>)
  where I: Idx;
impl<I> Default for IntoIdx<I>
  where I: Idx,
{
  fn default() -> Self {
    IntoIdx(PhantomData)
  }
}
impl<I, T> FnOnce<((usize, T),)> for IntoIdx<I>
  where I: Idx,
{
  type Output = (I, T);

  extern "rust-call" fn call_once(self, ((n, t),): ((usize, T),)) -> Self::Output {
    (I::new(n), t)
  }
}
impl<I, T> FnMut<((usize, T),)> for IntoIdx<I>
  where I: Idx,
{
  extern "rust-call" fn call_mut(&mut self, ((n, t),): ((usize, T),)) -> Self::Output {
    (I::new(n), t)
  }
}
impl<I> FnOnce<(usize,)> for IntoIdx<I>
  where I: Idx,
{
  type Output = I;

  extern "rust-call" fn call_once(self, (n,): (usize,)) -> Self::Output {
    I::new(n)
  }
}
impl<I> FnMut<(usize,)> for IntoIdx<I>
  where I: Idx,
{
  extern "rust-call" fn call_mut(&mut self, (n,): (usize,)) -> Self::Output {
    I::new(n)
  }
}
