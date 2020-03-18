
use std::fmt;
use std::ops::*;

use crate::Error;
use crate::module::*;

/// Do not implement drop on this type. Drop on `grid` is likewise also disallowed.
#[derive(Copy, Clone)]
pub(super) struct LaunchArgs<'a, A>
  where A: Kernel + 'a,
{
  pub(super) args: &'a A,
  /// The real grid size. The grid size as given to HSA will be rounded up to align with the
  /// workgroup size. This field records the user grid size as originally given.
  pub(super) grid: A::Grid,
}

pub struct VectorParams<G>
  where G: GridDims,
{
  wi: <G::Workgroup as WorkgroupDims>::Idx,
  // wg_size comes from A
  wg_id: G::Idx,
  wg_idx: G::Idx,
  grid_size: G,
  grid_id: G::Idx,
  gid: G::Elem,
}
pub type KVectorParams<A> = VectorParams<<A as Kernel>::Grid>;
impl<G> VectorParams<G>
  where G: GridDims,
{
  #[inline(always)]
  pub(super) fn new(grid: &G, wg_size: &G::Workgroup) -> Option<Self> {
    let wg = G::workgroup_id();
    let wi = G::workitem_id();

    // check that this is not an extra workitem (ie in the rounded up portion of the grid).
    if grid.grid_oob(wg_size,
                     &wg,
                     &wi)
    {
      return None;
    }

    Some(VectorParams {
      gid: grid.global_linear_id(wg_size, &wg, &wi),
      wg_idx: G::workgroup_idx(wg_size, &wg),
      grid_id: grid.grid_id(wg_size, &wg, &wi),
      grid_size: grid.clone(),
      wi,
      wg_id: wg,
    })
  }

  #[inline(always)]
  pub fn wi(&self) -> &<G::Workgroup as WorkgroupDims>::Idx {
    &self.wi
  }
  #[inline(always)]
  pub fn wg_id(&self) -> &G::Idx {
    &self.wg_id
  }
  #[inline(always)]
  pub fn wg_idx(&self) -> &G::Idx {
    &self.wg_idx
  }
  #[inline(always)]
  pub fn grid_size(&self) -> &G {
    &self.grid_size
  }
  #[inline(always)]
  pub fn grid_id(&self) -> &G::Idx {
    &self.grid_id
  }

  /// Globally Linear Id.
  #[inline(always)]
  pub fn gl_id(&self) -> &G::Elem {
    &self.gid
  }
}
impl<G> Clone for VectorParams<G>
  where G: GridDims,
        <G::Workgroup as WorkgroupDims>::Idx: Clone,
        G::Idx: Clone,
        G: Clone,
{
  fn clone(&self) -> Self {
    VectorParams {
      wi: self.wi.clone(),
      wg_id: self.wg_id.clone(),
      wg_idx: self.wg_idx.clone(),
      grid_id: self.grid_id.clone(),
      grid_size: self.grid_size.clone(),
      gid: self.gid.clone(),
    }
  }
}
impl<G> Copy for VectorParams<G>
  where G: GridDims,
        <G::Workgroup as WorkgroupDims>::Idx: Copy,
        G::Idx: Copy,
        G: Copy,
{ }
impl<G> fmt::Debug for VectorParams<G>
  where G: GridDims,
        <G::Workgroup as WorkgroupDims>::Idx: fmt::Debug,
        G::Workgroup: 'static,
        G::Idx: fmt::Debug,
        G: fmt::Debug,
        G::Elem: fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.debug_struct("VectorParams")
      .field("wi", &self.wi)
      .field("wg_id", &self.wg_id)
      .field("wg_idx", &self.wg_idx)
      .field("grid_id", &self.grid_id)
      .field("grid_size", &self.grid_size)
      .field("gid", &self.gid)
      .finish()
  }
}

pub trait Completion: Deps {
  type CompletionSignal: SignalHandle + ?Sized;

  /// Iterates over all dep signals, but will skip the completion signal.
  #[inline(always)]
  fn iter_arg_deps<'a>(&'a self,
                       f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), Error>)
    -> Result<(), Error>
  {
    self.iter_deps(&mut move |signal| {
      if signal.signal_ref() == self.completion().signal_ref() {
        // Don't add the completion signal to the deps; we'd never actually launch!
        Ok(())
      } else {
        f(signal)
      }
    })
  }

  fn completion(&self) -> &Self::CompletionSignal;
}

/// Implement this trait for your kernel's argument structure. In the future, a derive macro
/// will help you deal with these details.
pub trait Kernel: Completion + Send + Sync + Unpin {
  type Grid: GridDims;
  const WORKGROUP: <Self::Grid as GridDims>::Workgroup;

  type Queue: ?Sized;

  fn queue(&self) -> &Self::Queue;

  /// Run the kernel. This function is called in every work item and group.
  fn kernel(&self, vp: KVectorParams<Self>)
    where Self: Sized;
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::utils::test::*;

  struct CompletionTest {
    dep: Arc<GlobalSignal>,
    completion: Arc<GlobalSignal>,
  }
  /// TODO make the deps derive macro work in this crate
  unsafe impl Deps for CompletionTest {
    #[inline(always)]
    fn iter_deps<'a>(&'a self,
                     f: &mut dyn FnMut(&'a dyn DeviceConsumable) -> Result<(), Error>)
      -> Result<(), Error>
    {
      // Note: call `f` on both signals; otherwise there'll be silent test breakage.
      self.dep.iter_deps(f)?;
      self.completion.iter_deps(f)?;
      Ok(())
    }
  }
  impl Completion for CompletionTest {
    type CompletionSignal = Arc<GlobalSignal>;
    fn completion(&self) -> &Self::CompletionSignal { &self.completion }
  }
  #[test]
  fn no_completion_in_deps() {
    let _ = device();

    let c = Arc::new(GlobalSignal::new(1).unwrap());

    let deps = CompletionTest {
      dep: Arc::new(GlobalSignal::new(1).unwrap()),
      completion: c.clone(),
    };

    deps.iter_arg_deps(&mut move |s| {
      assert_ne!(s.signal_ref(), c.signal_ref());
      Ok(())
    })
      .unwrap();
  }
}
