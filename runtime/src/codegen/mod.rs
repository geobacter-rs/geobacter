
use std::error::Error;
use std::ffi::{CString};
use std::sync::{mpsc};
use std::any::Any;

use rustc;
use rustc::session::Session;
use rustc::middle::cstore::{MetadataLoader};
use rustc::hir::def_id::{DefId};
use rustc::ty::{TyCtxt};
use rustc::ty::query::Providers;
use rustc_codegen_utils::codegen_backend::{CodegenBackend};
use syntax_pos::symbol::{Symbol};

use self::target_options::{TargetOptions, CodeModel};

use indexvec::IndexVec;

pub mod target_options;
pub mod worker;

pub struct LlvmTransCrate {
  inner: Box<CodegenBackend>,
  entry_shim: DefId,
}
impl LlvmTransCrate {
  pub fn new(sess: &Session, entry_shim: DefId) -> LlvmTransCrate {
    use rustc_driver::get_codegen_backend;
    LlvmTransCrate {
      inner: get_codegen_backend(sess),
      entry_shim,
    }
  }
}

impl CodegenBackend for LlvmTransCrate {
  fn init(&self, sess: &Session) {
    self.inner.init(sess)
  }
  fn print(&self, req: rustc::session::config::PrintRequest,
           sess: &Session) {
    self.inner.print(req, sess)
  }
  fn target_features(&self, sess: &Session) -> Vec<Symbol> {
    self.inner.target_features(sess)
  }
  fn print_passes(&self) { self.inner.print_passes() }
  fn print_version(&self) { self.inner.print_version() }
  fn diagnostics(&self) -> &[(&'static str, &'static str)] {
    self.inner.diagnostics()
  }

  fn metadata_loader(&self) -> Box<MetadataLoader> {
    Box::new(::metadata::DummyMetadataLoader)
  }
  fn provide(&self, providers: &mut Providers) {
    self.inner.provide(providers)
  }
  fn provide_extern(&self, providers: &mut Providers) {
    self.inner.provide_extern(providers)
  }
  fn trans_crate<'a, 'tcx>(&self,
                           tcx: TyCtxt<'a, 'tcx, 'tcx>,
                           rx: mpsc::Receiver<Box<Any + Send>>)
    -> Box<Any>
  {
    use rustc::ty::Instance;
    use rustc_target::spec::Abi;

    let root = Instance::mono(tcx, self.entry_shim);
    tcx.override_root(root, Abi::AmdGpuKernel);
    self.inner.trans_crate(tcx, rx)
  }

  /// This is called on the returned `Box<Any>` from `trans_crate`
  ///
  /// # Panics
  ///
  /// Panics when the passed `Box<Any>` was not returned by `trans_crate`.
  fn join_trans_and_link(
    &self,
    trans: Box<Any>,
    sess: &Session,
    dep_graph: &rustc::dep_graph::DepGraph,
    outputs: &rustc::session::config::OutputFilenames,
  ) -> Result<(), rustc::session::CompileIncomplete> {
    self.inner.join_trans_and_link(trans, sess, dep_graph, outputs)
  }
}

