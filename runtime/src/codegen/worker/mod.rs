
use std::cell::{RefCell};
use std::collections::{BTreeMap, };
use std::fs::File;
use std::io::{self, Read, };
use std::sync::mpsc::{channel, Sender, Receiver, RecvTimeoutError, };
use std::sync::{Arc, Weak, Mutex};
use std::ops::{Deref};
use std::time::Duration;
use std::path::{Path, };

use rustc;
use rustc::hir::def_id::{CrateNum, DefId};
use rustc::middle::exported_symbols::{SymbolExportLevel, };
use rustc::mir::{Mir, CustomIntrinsicMirGen, };
use rustc::session::config::{OutputType, CrateType, };
use rustc::ty::query::Providers;
use rustc::ty::{self, TyCtxt, subst::Substs, Instance, };
use rustc::util::nodemap::DefIdMap;
use rustc_driver::{self, driver::compute_crate_disambiguator, };
use rustc_data_structures::fx::{FxHashMap};
use rustc_data_structures::sync::{Lrc, };
use rustc_metadata;
use rustc_incremental;
use syntax::feature_gate;
use syntax_pos::symbol::{Symbol, InternedString, };

use legionella_intrinsics::{DefIdFromKernelId, GetDefIdFromKernelId,
                            AxisId, DispatchPtr, insert_all_intrinsics, };

use tempdir::TempDir;

use hsa_core::kernel::KernelId;

use {Accelerator, AcceleratorTargetDesc};
use metadata::{Metadata, DummyMetadataLoader, CrateMetadata, CrateMetadataLoader,
               CrateNameHash};
use utils::UnsafeSyncSender;

use passes::{Pass, PassType};

use self::error::IntoErrorWithKernelId;

mod tls;
mod collector;
pub mod error;

#[derive(Debug, Clone)]
pub struct CodegenResults {
  pub kernel_symbol: String,
  pub outputs: BTreeMap<OutputType, Vec<u8>>,
}

/// Since we can't send MIR directly, or even serialized (we'd have to
/// serialize the whole crate too), the host codegenner will be responsible
/// for creating wrapping code to extract host kernel args.
/// Initially, no MIR will be created, eg by extracting a part of a function,
/// so this won't result in new host functions being codegenned (any function
/// we can reach would also be reachable in the original compilation).
pub enum HostCreateFuncMessage {
  /// A unmodified function in some crate
  ImplDefId(TyCtxtLessKernelId),

}

pub enum Message {
  /// So we can know when to exit.
  AddAccel(Weak<Accelerator>),
  HostCreateFunc {
    msg: HostCreateFuncMessage,
    accel_desc: Arc<AcceleratorTargetDesc>,
    ret: Sender<Result<usize, error::Error>>,
  },
  Codegen {
    id: KernelId,

    host_accel: Option<Sender<Message>>,

    ret: Sender<Result<CodegenResults, error::Error>>,
  },
}

#[derive(Clone)]
pub struct CodegenComms(Sender<Message>);
impl CodegenComms {
  pub fn new(accel_desc: Arc<AcceleratorTargetDesc>,
             accel: &Arc<Accelerator>,
             metadata: Arc<Mutex<Vec<Metadata>>>)
    -> io::Result<CodegenComms>
  {
    WorkerTranslatorData::new(accel_desc, accel, metadata)
  }
  pub fn codegen(&self, id: KernelId,
                 host: Option<CodegenComms>)
    -> Result<CodegenResults, error::Error>
  {
    let (tx, rx) = channel();
    let msg = Message::Codegen {
      id,
      host_accel: host
        .map(|host| {
          let CodegenComms(host) = host;
          host
        }),
      ret: tx,
    };

    self.0.send(msg)
      .expect("the codegen thread crashed/exited");

    let ret = rx.recv()
      .expect("the codegen thread crashed/exited")?;

    Ok(ret)
  }
  pub fn add_accel(&self, accel: &Arc<Accelerator>) {
    let msg = Message::AddAccel(Arc::downgrade(accel));
    self.0.send(msg)
      .expect("the codegen thread crashed/exited");
  }

  pub unsafe fn sync_comms(self) -> CodegenUnsafeSyncComms {
    let CodegenComms(this) = self;
    UnsafeSyncSender(this)
  }
}
pub type CodegenUnsafeSyncComms = UnsafeSyncSender<Message>;
impl From<CodegenUnsafeSyncComms> for CodegenComms {
  fn from(UnsafeSyncSender(v): CodegenUnsafeSyncComms) -> Self {
    CodegenComms(v)
  }
}

#[derive(Clone, Copy)]
pub struct TranslatorCtx<'a, 'b, 'c, 'tcx>
  where 'a: 'c,
        'tcx: 'b,
{
  tcx: TyCtxt<'b, 'tcx, 'tcx>,
  worker: &'c WorkerTranslatorData<'a>,
  root: DefId,
}

impl<'a, 'b, 'c, 'tcx> TranslatorCtx<'a, 'b, 'c, 'tcx>
  where 'a: 'c,
{
  pub fn passes(&self) -> &[Box<Pass>] { self.worker.passes.as_ref() }
  pub fn as_def_id(&self, id: KernelId) -> Result<DefId, String> {
    use rustc::hir::def_id::{DefIndex};

    let crate_num = self.worker.lookup_crate_num(id)
      .ok_or_else(|| {
        format!("crate metadata missing for `{}`", id.crate_name)
      })?;

    let def_id = DefId {
      krate: crate_num,
      index: DefIndex::from_raw_u32(id.index as u32),
    };
    Ok(def_id)
  }
  pub fn replace_def_id(&self, id: DefId) -> DefId {
    let replaced = self.worker.replaced_def_ids.borrow().get(&id)
    .map(|&id| id );
    if let Some(replaced) = replaced {
      return replaced;
    }

    let mut new_def_id = id;
    for pass in self.worker.passes.iter() {
      let ty = pass.pass_type();
      let replaced = match ty {
        PassType::Replacer(f) => {
          f(*self, new_def_id)
        },
      };
      match replaced {
        Some(id) => {
          new_def_id = id;
          break;
        },
        None => { continue; }
      }
    }

    let new_def_id = new_def_id;

    self.worker.replaced_def_ids.borrow_mut().insert(id, new_def_id);
    new_def_id
  }
  pub fn is_root(&self, def_id: DefId) -> bool {
    self.root == def_id
  }
}

impl<'a, 'b, 'c, 'tcx> Deref for TranslatorCtx<'a, 'b, 'c, 'tcx>
  where 'a: 'c,
{
  type Target = TyCtxt<'b, 'tcx, 'tcx>;
  fn deref(&self) -> &TyCtxt<'b, 'tcx, 'tcx> { &self.tcx }
}
impl<'a, 'b, 'c, 'tcx> DefIdFromKernelId for TranslatorCtx<'a, 'b, 'c, 'tcx>
  where 'a: 'c,
{
  fn get_cstore(&self) -> &rustc_metadata::cstore::CStore { self.worker.cstore }
}
struct DefIdFromKernelIdGetter;
impl GetDefIdFromKernelId for DefIdFromKernelIdGetter {
  fn with_self<F, R>(f: F) -> R
    where F: FnOnce(&dyn DefIdFromKernelId) -> R,
  {
    self::tls::with(|tcx| f(&tcx as &dyn DefIdFromKernelId) )
  }
}

pub struct WorkerTranslatorData<'a> {
  pub accel_desc: Arc<AcceleratorTargetDesc>,
  pub accels: Vec<Weak<Accelerator>>,
  pub sess: &'a rustc::session::Session,
  pub cstore: &'a rustc_metadata::cstore::CStore,
  pub passes: Vec<Box<Pass>>,
  replaced_def_ids: RefCell<FxHashMap<DefId, DefId>>,
  intrinsics: FxHashMap<InternedString, Lrc<dyn CustomIntrinsicMirGen>>,
}
impl<'a> WorkerTranslatorData<'a> {
  pub fn new(accel_desc: Arc<AcceleratorTargetDesc>,
             accel: &Arc<Accelerator>,
             metadata: Arc<Mutex<Vec<Metadata>>>)
    -> io::Result<CodegenComms>
  {
    use std::thread::Builder;

    use passes::alloc::AllocPass;
    use passes::lang_item::LangItemPass;
    use passes::panic::PanicPass;
    use passes::compiler_builtins::CompilerBuiltinsReplacerPass;

    use rustc::middle::cstore::MetadataLoader;
    use rustc::middle::dependency_format::Dependencies;

    let (tx, rx) = channel();
    let accel = Arc::downgrade(&accel);

    let name = format!("codegen thread for {}",
                       accel_desc.target.llvm_target);

    let f = move || {
      info!("codegen thread for {} startup",
            accel_desc.target.llvm_target);

      ::syntax::with_globals(move || {
        with_rustc_session(move |mut sess| {
          sess.entry_fn.set(None);
          sess.plugin_registrar_fn.set(None);
          sess.derive_registrar_fn.set(None);
          let ctype = CrateType::Executable;
          sess.crate_types.set(vec![ctype.clone()]);
          sess.recursion_limit.set(512);
          sess.allocator_kind.set(None);

          let mut deps: Dependencies = Default::default();
          deps.insert(ctype, vec![]);
          sess.dependency_formats.set(deps);

          sess.init_features(feature_gate::Features::new());

          let dis = compute_crate_disambiguator(&sess);
          sess.crate_disambiguator.set(dis);
          accel_desc.rustc_target_options(&mut sess.target.target);

          let md_loader = Box::new(DummyMetadataLoader);
          let md_loader = md_loader as Box<dyn MetadataLoader + Sync>;
          let cstore = rustc_metadata::cstore::CStore::new(md_loader);

          let mut data = WorkerTranslatorData {
            accel_desc,
            accels: vec![accel],
            sess: &sess,
            cstore: &cstore,
            passes: vec![
              Box::new(LangItemPass),
              Box::new(AllocPass),
              Box::new(PanicPass),
              Box::new(CompilerBuiltinsReplacerPass),
            ],
            replaced_def_ids: Default::default(),
            intrinsics: Default::default(),
          };
          {
            let metadata = metadata.lock().unwrap();
            data.initialize_cstore(metadata.as_ref());
          }
          insert_all_intrinsics(&DefIdFromKernelIdGetter,
          |k, v| {
            let k = Symbol::intern(k.as_ref()).as_interned_str();
            assert!(data.intrinsics.insert(k, v).is_none());
          });

          data.thread(&rx);
        })
      })
    };

    let _ = Builder::new()
      .name(name)
      .spawn(f)?;

    Ok(CodegenComms(tx))
  }

  fn thread(&mut self, rx: &Receiver<Message>) {
    let to = Duration::from_secs(10);

    let mut recv_msg = None;

    'outer: loop {
      {
        let arena = rustc::ty::AllArenas::new();
        let krate = create_empty_hir_crate();
        let dep_graph = rustc::dep_graph::DepGraph::new_disabled();
        let mut forest = rustc::hir::map::Forest::new(krate, &dep_graph);
        let mut defs = rustc::hir::map::definitions::Definitions::new();
        let disambiguator = self.sess.crate_disambiguator
          .borrow()
          .clone();
        defs.create_root_def("jit-methods", disambiguator);
        let defs = defs;

        'inner: loop {
          let msg = recv_msg
            .take()
            .unwrap_or_else(|| {
              rx.recv_timeout(to)
            });
          let msg = match msg {
            Ok(msg) => msg,
            Err(RecvTimeoutError::Timeout) => {
              break 'inner;
            },
            Err(RecvTimeoutError::Disconnected) => {
              return;
            },
          };

          match msg {
            Message::AddAccel(accel) => {
              self.accels.push(accel);
            },
            Message::HostCreateFunc {
              msg, accel_desc, ret,
            } => {
              unimplemented!();
            },
            Message::Codegen {
              id,
              host_accel,
              ret,
            } => {
              let result = self.codegen_kernel(id, &arena,
                                               unsafe {
                                                 ::std::mem::transmute(&mut forest)
                                               },
                                               unsafe {
                                                 ::std::mem::transmute(&defs)
                                               },
                                               &dep_graph);
              let _ = ret.send(result);
            },
          }

          continue 'inner;
        }
      }

      let live = self.accels.iter()
        .any(|a| a.upgrade().is_some());
      if !live { return; }
      // else wait for the next message, at which point we will reinitialize.
      match rx.recv() {
        Err(_) => { return; },
        Ok(msg) => {
          recv_msg = Some(Ok(msg));
        },
      }
    }
  }

  fn initialize_cstore(&mut self, md: &[Metadata]) {
    let mut loader = CrateMetadataLoader::default();
    let CrateMetadata(meta) = loader.build(md, &self.sess,
                                           self.cstore)
      .unwrap();
    for meta in meta.into_iter() {
      let name = CrateNameHash {
        name: meta.name,
        hash: meta.root.hash.as_u64(),
      };
      let cnum = loader.lookup_cnum(&name)
        .unwrap();
      self.cstore.set_crate_data(cnum, meta);
    }
  }

  fn codegen_kernel<'b>(&self, id: KernelId,
                        arena: &'a rustc::ty::AllArenas<'b>,
                        forest: &'b mut rustc::hir::map::Forest,
                        defs: &'b rustc::hir::map::definitions::Definitions,
                        dep_graph: &rustc::dep_graph::DepGraph)
    -> Result<CodegenResults, error::Error>
    where 'a: 'b,
  {
    use rustc::hir::def_id::{DefId, DefIndex};
    use rustc_driver::get_codegen_backend;

    let crate_num = self
      .lookup_crate_num(id)
      .ok_or_else(|| {
        error::Error::NoCrateMetadata(id)
      })?;

    info!("translating defid {:?}:{}", crate_num, id.index);

    let def_id = DefId {
      krate: crate_num,
      index: DefIndex::from_raw_u32(id.index as u32),
    };
    assert!(!def_id.is_local());

    let codegen = get_codegen_backend(self.sess);

    // extern only providers:
    let mut local_providers = rustc::ty::query::Providers::default();
    rustc_driver::driver::default_provide(&mut local_providers);
    codegen.provide(&mut local_providers);
    providers_local(&mut local_providers);

    let mut extern_providers = local_providers.clone();
    rustc_driver::driver::default_provide_extern(&mut extern_providers);
    codegen.provide_extern(&mut extern_providers);
    provide_extern_overrides(&mut extern_providers);

    let disk_cache = rustc_incremental::load_query_result_cache(self.sess);

    let (tx, rx) = channel();

    let tmpdir = TempDir::new("hsa-runtime-codegen")
      .with_kernel_id(id)?;

    let out = rustc::session::config::OutputFilenames {
      out_directory: tmpdir.path().into(),
      out_filestem: "codegen.elf".into(),
      single_output_file: None,
      extra: "".into(),
      outputs: output_types(),
    };

    let map_crate = rustc::hir::map::map_crate(self.sess, self.cstore,
                                               forest, defs);
    let resolutions = rustc::ty::Resolutions {
      freevars: Default::default(),
      trait_map: Default::default(),
      maybe_unused_trait_imports: Default::default(),
      maybe_unused_extern_crates: Default::default(),
      export_map: Default::default(),
      extern_prelude: Default::default(),
    };

    let kernel_symbol = rustc::ty::TyCtxt::create_and_enter(self.sess,
                                                            self.cstore,
                                                            local_providers,
                                                            extern_providers,
                                                            &arena,
                                                            resolutions,
                                                            map_crate,
                                                            disk_cache,
                                                            "", tx,
                                                            &out, |tcx| -> Result<String, _> {
        info!("output dir for {:?}: {}", def_id, out.out_directory.display());

        self::tls::enter(self, tcx, def_id, |tcx| {
          let trans_data = codegen.codegen_crate(tcx.tcx, rx);

          codegen.join_codegen_and_link(trans_data, self.sess,
                                        dep_graph, &out)
            .map_err(|err| {
              error!("codegen failed: `{:?}`!", err);
              error::Error::Codegen(id)
            })?;

          Ok(())
        })?;

        let root = Instance::mono(tcx, def_id);
        let kernel_symbol = tcx.symbol_name(root);
        info!("kernel symbol for def_id {:?}: {}", root,
              kernel_symbol);
        Ok(format!("{}", kernel_symbol))
      })?;
    info!("codegen complete {:?}:{}", crate_num, id.index);

    //let output_dir = tmpdir.path();
    let output_dir = tmpdir.into_path();

    let mut outputs = BTreeMap::new();
    for &output_type in out.outputs.keys() {
      let filename = Path::new(&out.out_filestem)
        .with_extension(output_type.extension());
      let output = output_dir.join(filename);
      info!("reading output {}", output.display());
      let mut file = File::open(output).with_kernel_id(id)?;
      let mut data = Vec::new();
      {
        file.read_to_end(&mut data)
          .with_kernel_id(id)?;
      }

      outputs.insert(output_type, data);
    }

    Ok(CodegenResults {
      kernel_symbol,
      outputs
    })
  }

  fn lookup_crate_num(&self, kernel_id: KernelId) -> Option<CrateNum> {
    let mut out = None;
    let needed_fingerprint =
      (kernel_id.crate_hash_hi,
       kernel_id.crate_hash_lo);
    self.cstore.iter_crate_data(|num, data| {
      if out.is_some() { return; }

      if data.name != kernel_id.crate_name {
        return;
      }
      let finger = data.root.disambiguator.to_fingerprint().as_value();
      if needed_fingerprint == finger {
        out = Some(num);
      }
    });

    out
  }
}

fn output_types() -> rustc::session::config::OutputTypes {
  use rustc::session::config::*;

  let output = (OutputType::Object, None);
  let ir_out = (OutputType::LlvmAssembly, None);
  let asm    = (OutputType::Assembly, None);
  let mut out = Vec::new();
  out.push(output);
  out.push(ir_out);
  out.push(asm);
  OutputTypes::new(&out[..])
}

pub fn with_rustc_session<F, R>(f: F) -> R
  where F: FnOnce(rustc::session::Session) -> R,
{
  use rustc_driver::driver::spawn_thread_pool;

  let opts = create_rustc_options();
  spawn_thread_pool(opts, move |opts| {
    let registry = rustc_driver::diagnostics_registry();
    let sess = rustc::session::build_session(opts, None, registry);
    f(sess)
  })
}

pub fn create_rustc_options() -> rustc::session::config::Options {
  use rustc::session::config::*;
  use rustc_target::spec::*;

  let mut opts = Options::default();
  opts.crate_types.push(CrateType::Executable);
  opts.output_types = output_types();
  opts.optimize = OptLevel::Aggressive;
  opts.cg.lto = LtoCli::No;
  opts.cg.panic = Some(PanicStrategy::Abort);
  opts.cg.incremental = None;
  opts.cg.overflow_checks = Some(false);
  opts.cli_forced_codegen_units = Some(1);
  opts.incremental = None;
  opts.debugging_opts.verify_llvm_ir = false;
  opts.debugging_opts.no_landing_pads = true;
  opts.debugging_opts.incremental_queries = false;
  //opts.debugging_opts.print_llvm_passes = true;
  opts.debugging_opts.polly = true;
  opts.cg.llvm_args.push("-polly-run-inliner".into());
  opts.cg.llvm_args.push("-polly-register-tiling".into());
  opts.cg.llvm_args.push("-polly-check-vectorizable".into());
  //opts.cg.llvm_args.push("-enable-polly-aligned".into());
  //opts.cg.llvm_args.push("-polly-target=gpu".into());
  opts.cg.llvm_args.push("-polly-vectorizer=polly".into());
  opts.cg.llvm_args.push("-polly-position=early".into());
  opts.cg.llvm_args.push("-polly-process-unprofitable".into());
  opts.cg.llvm_args.push("-polly-enable-polyhedralinfo".into());
  opts
}

pub fn create_empty_hir_crate() -> rustc::hir::Crate {
  use rustc::hir::*;
  use syntax_pos::DUMMY_SP;

  let m = Mod {
    inner: DUMMY_SP,
    item_ids: HirVec::from(vec![]),
  };

  let attrs = HirVec::from(vec![]);
  let span = DUMMY_SP;
  let exported_macros = HirVec::from(vec![]);
  let items = BTreeMap::new();
  let trait_items = BTreeMap::new();
  let impl_items = BTreeMap::new();
  let bodies = BTreeMap::new();
  let trait_impls = BTreeMap::new();
  let trait_auto_impl = BTreeMap::new();

  let body_ids = Vec::new();

  Crate {
    module: m,
    attrs,
    span,
    exported_macros,
    items,
    trait_items,
    impl_items,
    bodies,
    trait_impls,
    trait_auto_impl,
    body_ids,
  }
}

fn replace(def_id: DefId) -> DefId {
  self::tls::with(|tcx| {
    tcx.replace_def_id(def_id)
  })
}

pub fn provide_mir_overrides(providers: &mut Providers) {
  *providers = Providers {
    is_mir_available: |tcx, def_id| {
      let mut providers = Providers::default();
      rustc_metadata::cstore::provide_extern(&mut providers);

      let def_id = replace(def_id);

      (providers.is_mir_available)(tcx, def_id)
    },
    optimized_mir: |tcx, def_id| {
      let mut providers = Providers::default();
      rustc_metadata::cstore::provide_extern(&mut providers);

      let def_id = replace(def_id);

      (providers.optimized_mir)(tcx, def_id)
    },
    .. *providers
  }
}
pub fn provide_extern_overrides(providers: &mut Providers) {
  provide_mir_overrides(providers);
  collector::provide(providers);
  providers_remote_and_local(providers);
}

pub fn stub_providers(providers: &mut Providers) {
  *providers = Providers {
    coherent_trait: |_tcx, _def_id| {
      // No-op.
    },
    .. *providers
  };
}

pub fn providers_local(providers: &mut Providers) {
  use rustc::hir::def_id::{LOCAL_CRATE};
  use rustc_data_structures::svh::Svh;

  use std::rc::Rc;

  *providers = Providers {
    crate_hash: |_tcx, cnum| {
      assert_eq!(cnum, LOCAL_CRATE);
      // XXX?
      Svh::new(1)
    },
    crate_name: |_tcx, id| {
      assert_eq!(id, LOCAL_CRATE);
      Symbol::intern("jit-methods")
    },
    crate_disambiguator: |tcx, cnum| {
      assert_eq!(cnum, LOCAL_CRATE);
      tcx.sess.crate_disambiguator.borrow().clone()
    },
    native_libraries: |_tcx, cnum| {
      assert_eq!(cnum, LOCAL_CRATE);
      Rc::new(vec![])
    },
    link_args: |_tcx, cnum| {
      assert_eq!(cnum, LOCAL_CRATE);
      Rc::new(vec![])
    },
    all_crate_nums: |tcx, cnum| {
      // tcx.cstore is private, so work around here.
      let mut providers = Providers::default();
      rustc::ty::provide(&mut providers);
      (providers.all_crate_nums)(tcx, cnum)
    },
    postorder_cnums: |tcx, cnum| {
      // tcx.cstore is private, so work around here.
      let mut providers = Providers::default();
      rustc::ty::provide(&mut providers);
      (providers.postorder_cnums)(tcx, cnum)
    },
    output_filenames: |tcx, cnum| {
      // tcx.cstore is private, so work around here.
      let mut providers = Providers::default();
      rustc::ty::provide(&mut providers);
      (providers.output_filenames)(tcx, cnum)
    },
    .. *providers
  };
  provide_mir_overrides(providers);
  collector::provide(providers);
  providers_remote_and_local(providers);
}

fn providers_remote_and_local(providers: &mut Providers) {
  *providers = Providers {
    fn_sig,
    reachable_non_generics,
    custom_intrinsic_mirgen,
    upstream_monomorphizations,
    upstream_monomorphizations_for,
    .. *providers
  };

  fn fn_sig<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>, def_id: DefId) -> ty::PolyFnSig<'tcx> {
    use rustc::ty::{Binder, FnSig, };
    use rustc_target::spec::abi::Abi;

    let mut providers = Providers::default();
    rustc_metadata::cstore::provide_extern(&mut providers);
    let sig = (providers.fn_sig)(tcx, def_id);

    self::tls::with_tcx(tcx, |tcx| {
      if tcx.is_root(def_id) {
        // modify the abi:
        let sig = FnSig {
          abi: Abi::AmdGpuKernel,
          .. *sig.skip_binder()
        };
        Binder::bind(sig)
      } else {
        sig
      }
    })
  }
  fn reachable_non_generics<'a, 'tcx>(_tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                      _cnum: CrateNum)
    -> Lrc<DefIdMap<SymbolExportLevel>>
  {
    // we need to recodegen everything
    Lrc::new(DefIdMap())
  }
  fn upstream_monomorphizations<'a, 'tcx>(_tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                          _cnum: CrateNum)
    -> Lrc<DefIdMap<Lrc<FxHashMap<&'tcx Substs<'tcx>, CrateNum>>>>
  {
    // we never have any upstream monomorphizations.
    Lrc::new(Default::default())
  }
  fn upstream_monomorphizations_for<'a, 'tcx>(_tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                              _def_id: DefId)
    -> Option<Lrc<FxHashMap<&'tcx Substs<'tcx>, CrateNum>>>
  {
    None
  }
  fn custom_intrinsic_mirgen<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       def_id: DefId)
    -> Option<Lrc<dyn CustomIntrinsicMirGen>>
  {
    self::tls::with_tcx(tcx, |tcx| {
      let name = tcx.item_name(def_id);
      info!("custom intrinsic: {}", name);
      tcx.worker.intrinsics
        .get(&name)
        .cloned()
    })
  }
}

pub struct TyCtxtLessKernelId {
  pub crate_name: String,
  pub crate_hash_hi: u64,
  pub crate_hash_lo: u64,
  pub index: u64,
}
impl TyCtxtLessKernelId {
  pub fn from_def_id(tcx: TyCtxt<'_, '_, '_>,
                     def_id: DefId) -> Self {
    let crate_name = tcx.crate_name(def_id.krate);
    let crate_name = format!("{}", crate_name);

    let disambiguator = tcx.crate_disambiguator(def_id.krate);
    let (crate_hash_hi, crate_hash_lo) = disambiguator.to_fingerprint().as_value();

    let index = def_id.index.as_raw_u32() as u64;

    TyCtxtLessKernelId {
      crate_name,
      crate_hash_hi,
      crate_hash_lo,
      index,
    }
  }
}
