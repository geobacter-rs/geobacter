
//! Note this is just a stub for now.

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Metadata(NonZeroU64);
impl Metadata {
  pub(crate) fn handle(&self) -> sys::amd_comgr_metadata_node_t {
    sys::amd_comgr_metadata_node_s {
      handle: self.0.get(),
    }
  }
}
impl Drop for Metadata {
  fn drop(&mut self) {
    unsafe {
      sys::amd_comgr_destroy_metadata(self.handle())
    };
  }
}
