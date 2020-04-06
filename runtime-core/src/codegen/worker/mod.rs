//! The main entry point to codegen. This worker thread is responsible for
//! initializing a Rust compiler driver and tricking it into just running
//! codegen on whatever kernel we need to compile.
//!
//! This thread also stores a cache of previously completed codegens. It
//! used to be stored in the Context, however now that `KernelDesc` is
//! parameterized by the PlatformCodegen trait, it makes more sense to
//! store it here.
//! TODO the cache is in memory only. It would be a good idea to be able
//! write it to disk to free some memory.
//!

use std::any::Any;
use std::collections::{BTreeMap, };
use std::io::{self, };
use std::marker::{PhantomData, };
use std::mem::{self, drop, };
use std::sync::mpsc::{channel, Sender, Receiver, RecvTimeoutError, };
use std::sync::{Arc, Weak, Once, };
use std::time::Duration;

use rustc_middle;
use rustc_middle::arena::Arena;
use rustc_middle::middle::cstore::EncodedMetadata;
use rustc_middle::middle::exported_symbols::{SymbolExportLevel, };
use rustc_middle::mir::{CustomIntrinsicMirGen, };
use rustc_middle::ty::query::Providers;
use rustc_middle::ty::{self, TyCtxt, subst::SubstsRef, };
use rustc_session::Session;
use rustc_session::config::OutputFilenames;
use rustc_data_structures::fx::{FxHashMap};
use rustc_data_structures::sync::{Lrc, WorkerLocal, };
use rustc_feature as feature_gate;
use rustc_hir::def_id::{CrateNum, DefId, DefIdMap };
use rustc_metadata;
use rustc_metadata::{creader::CrateLoader, creader::CStore, };
use rustc_incremental;
use rustc_resolve::Resolver;
use rustc_span::DUMMY_SP;
use rustc_span::symbol::{Symbol, };

use crossbeam_utils::sync::WaitGroup;

use gintrinsics::{DriverData as GIDriverData, GetDriverData,
                  GeobacterCustomIntrinsicMirGen,
                  GeobacterMirGen, };

use parking_lot::RwLock;

use tempfile::{Builder as TDBuilder, };

use crate::{AcceleratorTargetDesc, context::Context, };
use crate::utils::{HashMap, StableHash, };

use self::error::IntoErrorWithKernelInstance;
pub use self::driver_data::{DriverData, PlatformDriverData, };

mod collector;
pub mod error;
mod driver_data;
mod util;

use super::{PlatformCodegen, PKernelDesc, };
use super::products::*;
use crate::codegen::{PlatformIntrinsicInsert, };
use crate::codegen::worker::error::PError;
use crate::metadata::{CrateMetadataLoader, CrateMetadata, CrateNameHash, DummyMetadataLoader};
use crate::utils::env::{use_llc, print_opt_remarks};

const CRATE_NAME: &'static str = "geobacter-cross-codegen";

// TODO we need to create a talk to a "host codegen" so that we can ensure
// that adt's have the same layout in the shader/kernel as on the host.
// TODO XXX only one codegen query is allowed at a time.
// TODO codegen worker workers (ie codegen multiple functions concurrently)
// Note: recreate the session/tyctxt on *every* codegen. It is not safe to reuse.

static SETUP_RUSTC_INTERFACE_CALLBACKS: Once = Once::new();

pub struct CodegenDriver<P>(WorkerTranslatorData<P>)
  where P: PlatformCodegen;
impl<P> CodegenDriver<P>
  where P: PlatformCodegen,
{
  pub fn new(context: &Context,
             accel_desc: Arc<AcceleratorTargetDesc>,
             platform: P)
    -> io::Result<Self>
  {
    SETUP_RUSTC_INTERFACE_CALLBACKS.call_once(|| {
      rustc_interface::callbacks::setup_callbacks();
    });

    let inner = WorkerTranslatorData {
      context: context.clone(),
      platform,
      target_desc: accel_desc,
      accels: Default::default(),
      cache: Default::default(),
    };
    Ok(CodegenDriver(inner))
  }

  pub fn codegen(&self, desc: PKernelDesc<P>)
    -> Result<Arc<PCodegenResults<P>>, error::PError<P>>
  {
    self.0.codegen_kernel(desc)
  }
  pub fn add_accel(&self, accel: &Arc<P::Device>) {
    let mut this = self.0.accels.write();
    this.push(Arc::downgrade(accel));
  }
}

#[allow(dead_code)]
pub(crate) enum Message<P>
  where P: PlatformCodegen,
{
  /// So we can know when to exit.
  AddAccel(Weak<P::Device>),
  Codegen {
    desc: PKernelDesc<P>,
    ret: Sender<Result<Arc<PCodegenResults<P>>, error::PError<P>>>,
  },
}

struct DriverDataGetter<P>(PhantomData<P>)
  where P: PlatformCodegen;
impl<P> GetDriverData for DriverDataGetter<P>
  where P: PlatformCodegen,
{
  fn with_self<F, R>(tcx: TyCtxt, f: F) -> R
    where F: FnOnce(&dyn GIDriverData) -> R,
  {
    PlatformDriverData::<P>::with(tcx, move |_tcx, pd| {
      f(pd.dd() as &dyn GIDriverData)
    })
  }
}
impl<P> Default for DriverDataGetter<P>
  where P: PlatformCodegen,
{
  fn default() -> Self {
    DriverDataGetter(PhantomData)
  }
}
type IntrinsicsMap = FxHashMap<Symbol, Lrc<dyn CustomIntrinsicMirGen>>;
pub struct PlatformIntrinsicInserter<'a, P>(&'a mut IntrinsicsMap, PhantomData<P>);
impl<'a, P> PlatformIntrinsicInsert for PlatformIntrinsicInserter<'a, P>
  where P: PlatformCodegen,
{
  fn insert_name<T>(&mut self, name: &str, intrinsic: T)
    where T: GeobacterCustomIntrinsicMirGen
  {
    let k = Symbol::intern(name);
    let marker: DriverDataGetter<P> = DriverDataGetter::default();
    let v = GeobacterMirGen::wrap(intrinsic, &marker);
    self.0.insert(k, v);
  }
}

enum MaybeInProgress<P>
  where P: PlatformCodegen,
{
  InProgress(WaitGroup),
  Done(Arc<PCodegenResults<P>>),
}
impl<P> Clone for MaybeInProgress<P>
  where P: PlatformCodegen,
{
  fn clone(&self) -> Self {
    match self {
      MaybeInProgress::InProgress(p) => MaybeInProgress::InProgress(p.clone()),
      MaybeInProgress::Done(r) => MaybeInProgress::Done(r.clone()),
    }
  }
}

pub struct WorkerTranslatorData<P>
  where P: PlatformCodegen,
{
  pub(self) context: Context,
  pub(crate) platform: P,
  pub target_desc: Arc<AcceleratorTargetDesc>,
  pub accels: RwLock<Vec<Weak<P::Device>>>,
  cache: RwLock<HashMap<PKernelDesc<P>, MaybeInProgress<P>>>,
}
impl<P> WorkerTranslatorData<P>
  where P: PlatformCodegen,
{
  pub fn new(ctx: &Context,
             target_desc: Arc<AcceleratorTargetDesc>,
             platform: P)
    -> io::Result<CodegenDriver<P>>
  {
    use std::thread::Builder;

    let (_tx, rx) = channel();
    let context = ctx.clone();

    let name = format!("codegen thread for {}",
                       target_desc.target.llvm_target);

    let f = move || {
      info!("codegen thread for {} startup",
            target_desc.target.llvm_target);

      let mut data = WorkerTranslatorData {
        context,
        platform,
        target_desc,
        accels: Default::default(),
        cache: Default::default(),
      };

      data.thread(&rx);
    };

    let _ = Builder::new()
      .name(name)
      .spawn(f)?;

    unimplemented!("TODO: host codegen queries");
  }

  /// Only used for host codegen queries.
  fn thread(&mut self, rx: &Receiver<Message<P>>) {

    enum InternalMessage<D> {
      Timeout,
      AddAccel(Weak<D>),
    }

    let to = Duration::from_secs(10);

    let mut recv_msg = None;

    // Ensure we don't timeout and quit before we've received the
    // first message (which should be an add accel message)
    let mut first_msg = true;

    'outer: loop {
      let internal_msg = 'inner: loop {
        let msg = recv_msg
          .take()
          .unwrap_or_else(|| {
            rx.recv_timeout(to)
          });
        let msg = match msg {
          Ok(msg) => msg,
          Err(RecvTimeoutError::Timeout) => {
            break 'inner InternalMessage::Timeout;
          },
          Err(error) => {
            warn!("receive error: {:?}", error);
            return;
          },
        };

        match msg {
          Message::AddAccel(accel) => {
            first_msg = false;
            break 'inner InternalMessage::AddAccel(accel);
          },
          _ if first_msg => {
            panic!("first message must be Message::AddAccel; \
                    fix your Accelerator impl");
          },
          Message::Codegen {
            desc,
            ret,
          } => {
            let result = self.codegen_kernel(desc);
            let _ = ret.send(result);
          },
        }
      };

      match internal_msg {
        InternalMessage::Timeout => { },
        InternalMessage::AddAccel(accel) => {
          self.accels.write().push(accel);
          continue 'outer;
        },
      }

      let live = {
        let accels = self.accels.read();
        accels.iter()
          .any(|a| a.upgrade().is_some())
      };
      if !live && !first_msg { return; }

      // else wait for the next message, at which point we will reinitialize.
      match rx.recv() {
        Err(error) => {
          warn!("receive error: {:?}", error);
          return;
        },
        Ok(msg) => {
          recv_msg = Some(Ok(msg));
        },
      }
    }
  }
  fn initialize_sess<F, R>(&self, f: F) -> Result<R, error::PError<P>>
    where F: FnOnce(Session, CStore) -> Result<R, error::PError<P>> + Send,
          R: Send,
  {
    self::util::on_global_thread_pool(|| {
      let metadata = self.context.load_metadata()
        .map_err(error::Error::LoadMetadata)?;

      let mut opts = create_rustc_options();
      self.platform.modify_rustc_session_options(&self.target_desc,
                                                 &mut opts);


      let registry = rustc_driver::diagnostics_registry();
      let mut sess = rustc_session::build_session(opts, None, registry);

      sess.crate_types.set(sess.opts.crate_types.clone());
      sess.recursion_limit.set(512);
      sess.type_length_limit.set(1048576);
      sess.const_eval_limit.set(1_000_000);

      sess.init_features(feature_gate::Features::default());

      // TODO hash the accelerator target desc
      let dis = self::util::compute_crate_disambiguator(&sess);
      sess.crate_disambiguator.set(dis);
      self.target_desc.rustc_target_options(&mut sess.target.target);

      // initialize the cstore for this codegen:
      // We have to do this everytime because the CStore does some
      // behind the scenes (vs letting the query system do it) caching
      // which causes missing allocations the second time around.
      // XXX fix upstream to remove that implicit assumption, that is
      // 1 cstore per 1 tcx (1 to 1 and onto).
      let mut cstore = CStore::default();
      {
        let mut loader = CrateMetadataLoader::default();
        let CrateMetadata(meta) = loader.build(&metadata, &mut cstore)
          .map_err(|err| error::Error::LoadMetadata(err.into()) )?;
        for meta in meta.into_iter() {
          let name = CrateNameHash {
            name: meta.root.name(),
            hash: meta.root.hash().as_u64(),
          };
          let cnum = loader.lookup_cnum(&name)
            .unwrap();

          cstore.set_crate_data(cnum, meta);
        }
      }

      f(sess, cstore)
    })
  }
  fn codegen_kernel(&self, desc: PKernelDesc<P>)
    -> Result<Arc<PCodegenResults<P>>, error::PError<P>>
  {
    use std::collections::hash_map::Entry;
    loop {
      loop {
        let cache = self.cache.read();
        if let Some(results) = cache.get(&desc).cloned() {
          // we *MUST* drop the read lock before waiting on in progress codegen.
          drop(cache);

          let progress = match results {
            MaybeInProgress::InProgress(progress) => progress,
            MaybeInProgress::Done(results) => {
              return Ok(results);
            },
          };

          debug!("{:?}: waiting for existing codegen..", desc.instance);

          progress.wait();
          continue;
        } else {
          break;
        }
      }

      let mut cache = self.cache.write();
      match cache.entry(desc.clone()) {
        Entry::Occupied(_) => {
          // someone beat us.
          continue;
        },
        Entry::Vacant(v) => {
          v.insert(MaybeInProgress::InProgress(WaitGroup::new()));
          break;
        },
      }
    }

    let result = self.initialize_sess(|sess, cstore, | {
      self.codegen_kernel_inner(desc.clone(),
                                sess,
                                cstore)
          .map(Arc::new)
    });

    let mut cache = self.cache.write();
    match result {
      Ok(ref result) => {
        let entry = cache.get_mut(&desc).unwrap();
        *entry = MaybeInProgress::Done(result.clone());
      },
      Err(_) => {
        // we still need to unblock other threads
        cache.remove(&desc);
      },
    }

    result
  }
  fn codegen_kernel_inner(&self,
                          desc: PKernelDesc<P>,
                          sess: Session,
                          cstore: CStore)
    -> Result<PCodegenResults<P>, error::PError<P>>
  {
    use self::util::get_codegen_backend;
    use syntax::ast;

    let context = &self.context;

    let instance = desc.instance;
    let hash = instance.stable_hash();
    info!("translating {:?}, hash: 0x{:x}",
          instance, hash);

    let codegen = get_codegen_backend(&sess);

    // extern only providers:
    let mut local_providers = rustc_middle::ty::query::Providers::default();
    self::util::default_provide(&mut local_providers);
    codegen.provide(&mut local_providers);
    Self::providers_local(&mut local_providers);

    let mut extern_providers = local_providers.clone();
    self::util::default_provide_extern(&mut extern_providers);
    codegen.provide_extern(&mut extern_providers);
    Self::provide_extern_overrides(&mut extern_providers);

    let disk_cache = rustc_incremental::load_query_result_cache(&sess);

    let tmpdir = TDBuilder::new()
      .prefix("geobacter-runtime-codegen-")
      .tempdir()
      .with_kernel_instance(desc.instance)?;

    let out = OutputFilenames::new(
      tmpdir.path().into(),
      "codegen.elf".into(),
      None,
      Default::default(),
      sess.opts.output_types.clone(),
    );

    let krate = create_empty_hir_crate();
    let dep_graph = rustc_middle::dep_graph::DepGraph::new(Default::default(),
                                                           Default::default());
    let arenas = WorkerLocal::new(|_| Arena::default());

    let ast_krate = ast::Crate {
      module: ast::Mod {
        inner: DUMMY_SP,
        items: vec![],
        inline: false,
      },
      attrs: vec![],
      span: DUMMY_SP,
      proc_macros: Default::default(),
    };
    let resolver_arenas = Resolver::arenas();
    let crate_name = CRATE_NAME;
    let crate_loader = CrateLoader::new_from_cstore(&sess, &DummyMetadataLoader,
                                                    crate_name, cstore);
    let resolver = Resolver::new_with_cloader(&sess, &ast_krate, crate_name,
                                              &resolver_arenas, crate_loader);
    let mut resolutions = resolver.into_outputs();
    let definitions = mem::take(&mut resolutions.definitions);

    let mut intrinsics = IntrinsicsMap::default();
    {
      let marker: DriverDataGetter<P> = DriverDataGetter::default();
      gintrinsics::intrinsics::insert_all_intrinsics(&marker, |k, v| {
        let k = Symbol::intern(&k);
        assert!(intrinsics.insert(k, v).is_none());
      });
    }
    {
      let mut inserter = PlatformIntrinsicInserter(&mut intrinsics,
                                                   PhantomData::<P>);
      self.platform
        .insert_intrinsics(&self.target_desc, &mut inserter);
      self.platform
        .insert_kernel_intrinsics(&desc, &mut inserter);
    }

    let accels = self.accels.read().clone();

    let driver_data: PlatformDriverData<P> =
      PlatformDriverData::new(context.clone(),
                              &accels,
                              &self.target_desc,
                              intrinsics,
                              &self.platform);
    let driver_data: PlatformDriverData<'static, P> = unsafe {
      ::std::mem::transmute(driver_data)
    };
    let driver_data = Box::new(driver_data) as Box<dyn Any + Send + Sync>;

    let gcx = TyCtxt::create_global_ctxt(
      &sess,
      Lrc::new(rustc_lint::LintStore::new()),
      local_providers,
      extern_providers,
      &arenas,
      resolutions,
      &krate,
      &definitions,
      dep_graph.clone(),
      disk_cache,
      CRATE_NAME,
      &out,
      Some(driver_data),
    );

    let results: Result<PCodegenResults<P>, PError<P>> = ty::tls::enter_global(&gcx, |tcx| {
      // Do some initialization of the DepGraph that can only be done with the
      // tcx available.
      tcx.sess.time("dep graph tcx init", || rustc_incremental::dep_graph_tcx_init(tcx));

      tcx.sess.time("platform root and condition init",
           move || {
             PlatformDriverData::<P>::with(tcx, |tcx, pd| {
               pd.init_root(desc, tcx)?;

               pd.init_conditions(tcx)?;

               pd.pre_codegen(tcx)
             })
           })?;

      let metadata = EncodedMetadata::new();
      let need_metadata_module = false;

      let ongoing_codegen = tcx.sess.time("codegen", || {
        let _prof_timer = tcx.prof.generic_activity("codegen_crate");
        codegen.codegen_crate(tcx, metadata, need_metadata_module)
      });

      let codegen_results = tcx.sess.time("LLVM codegen",
           || {
             codegen.join_codegen(ongoing_codegen, &sess, &dep_graph)
               .map_err(|_| {
                 error::Error::Codegen
               })
           })?;
      tcx.sess.time("link",
                    || {
                      codegen.link(&sess, codegen_results, &out)
                        .map_err(|_| {
                          error::Error::Linking
                        })
                    })?;

      let results = PlatformDriverData::<P>::with(tcx, |tcx, pd| {
        pd.post_codegen(tcx, &tmpdir.path(), &out)
      })?;

      Ok(results)
    });

    let mut results = results?;

    let output_dir = tmpdir.into_path();

    self.platform
      .post_codegen(&self.target_desc,
                    &output_dir,
                    &mut results)
      .map_err(error::Error::PostCodegen)?;
    // check that the platform actually inserted an exe entry:
    debug_assert!(results.exe_ref().is_some(),
      "internal platform codegen error: platform didn't insert an Exe \
       output type into the results");

    info!("codegen intermediates dir: {}", output_dir.display());
    info!("codegen complete {:?}, hash: 0x{:x}",
          instance, hash);

    Ok(results)
  }
}
pub fn create_rustc_options() -> rustc_session::config::Options {
  use rustc_session::config::*;
  use rustc_target::spec::*;

  let mut opts = Options::default();
  // We need to have the tcx build the def_path_hash_to_def_id map:
  opts.debugging_opts.query_dep_graph = true;
  opts.optimize = OptLevel::No;
  opts.optimize = OptLevel::Aggressive;
  opts.debuginfo = DebugInfo::Full;

  let output = (OutputType::Bitcode, None);
  let ir_out = (OutputType::LlvmAssembly, None);
  let mut out = Vec::new();
  out.push(output);
  out.push(ir_out);

  if opts.optimize == OptLevel::Aggressive && !use_llc() {
    // prevent LLVM from taking us down if we're not optimizing:
    let asm = (OutputType::Assembly, None);
    let obj = (OutputType::Object, None);
    out.push(asm);
    out.push(obj);
  }
  opts.output_types = OutputTypes::new(&out);

  let print_remarks = print_opt_remarks();
  if print_remarks {
    opts.debuginfo = DebugInfo::Limited;
  }
  // Avoid a mystery warning about not being about to remove a temp
  // object after codegen.
  opts.cg.save_temps = true;
  opts.cg.lto = LtoCli::No;
  opts.cg.panic = Some(PanicStrategy::Abort);
  opts.cg.incremental = None;
  opts.cg.overflow_checks = Some(false);
  if print_remarks {
    opts.cg.remark = Passes::All;
  }
  opts.cli_forced_codegen_units = Some(1);
  opts.incremental = None;
  opts.debugging_opts.verify_llvm_ir = false;
  opts.debugging_opts.no_landing_pads = true;
  opts.cg.no_prepopulate_passes = false;
  if opts.cg.no_prepopulate_passes {
    opts.cg.passes.push("name-anon-globals".into());
  } else if opts.optimize != OptLevel::No {
    // Should we run this unconditionally?
    opts.cg.passes.push("wholeprogramdevirt".into());
    opts.cg.passes.push("speculative-execution".into());
    for _ in 0..2 {
      opts.cg.passes.push("infer-address-spaces".into());
      opts.cg.passes.push("instcombine".into());
      opts.cg.passes.push("sroa".into());
      opts.cg.passes.push("mem2reg".into());
    }
    opts.cg.passes.push("simplifycfg".into());
  }
  opts.debugging_opts.print_llvm_passes = false;
  opts.cg.llvm_args.push("-expensive-combines".into());
  opts.cg.llvm_args.push("-spec-exec-only-if-divergent-target".into());
  opts.cg.llvm_args.push("-amdgpu-early-inline-all".into());
  opts.cg.llvm_args.push("-amdgpu-prelink".into());
  opts.cg.llvm_args.push("-sroa-strict-inbounds".into());
  opts.debugging_opts.polly =
    opts.optimize == OptLevel::Aggressive;
  opts.cg.llvm_args.push("-polly-run-inliner".into());
  opts.cg.llvm_args.push("-polly-register-tiling".into());
  opts.cg.llvm_args.push("-polly-check-vectorizable".into());
  opts.cg.llvm_args.push("-enable-polly-aligned".into());
  opts.cg.llvm_args.push("-polly-vectorizer=stripmine".into());
  //opts.cg.llvm_args.push("-polly-position=early".into());
  opts.cg.llvm_args.push("-polly-enable-polyhedralinfo".into());

  // Disable these alignment assumptions inserted during optimization:
  // They aren't really helpful and in fact block a lot of possible mem2reg promotions as a
  // result of their presence.
  opts.cg.llvm_args
    .push("-preserve-alignment-assumptions-during-inlining=false".into());
  opts
}

pub fn create_empty_hir_crate<'hir>() -> rustc_hir::Crate<'hir> {
  use rustc_hir::*;

  let m = Mod {
    inner: DUMMY_SP,
    item_ids: Default::default(),
  };

  let attrs = Default::default();
  let span = DUMMY_SP;
  let exported_macros = Default::default();
  let items = BTreeMap::new();
  let trait_items = BTreeMap::new();
  let impl_items = BTreeMap::new();
  let bodies = BTreeMap::new();
  let trait_impls = BTreeMap::new();
  let modules = BTreeMap::new();

  let body_ids = Vec::new();

  Crate {
    item: CrateItem {
      module: m,
      attrs,
      span,
    },
    exported_macros,
    items,
    trait_items,
    impl_items,
    bodies,
    trait_impls,
    body_ids,
    modules,
    non_exported_macro_attrs: Default::default(),
    proc_macros: Default::default(),
  }
}

impl<P> WorkerTranslatorData<P>
  where P: PlatformCodegen,
{
  pub fn provide_mir_overrides(providers: &mut Providers) {
    *providers = Providers {
      is_mir_available: |tcx, def_id| {
        let mut providers = Providers::default();
        rustc_metadata::provide_extern(&mut providers);

        let def_id = PlatformDriverData::<P>::with(tcx, |tcx, pd| {
          let dd = pd.dd();
          dd.stubber().unwrap()
            .stub_def_id(tcx, dd, def_id)
        });

        (providers.is_mir_available)(tcx, def_id)
      },
      optimized_mir: |tcx, def_id| {
        let mut providers = Providers::default();
        rustc_metadata::provide_extern(&mut providers);

        let def_id = PlatformDriverData::<P>::with(tcx, |tcx, pd| {
          let dd = pd.dd();
          dd.stubber().unwrap()
            .stub_def_id(tcx, dd, def_id)
        });

        (providers.optimized_mir)(tcx, def_id)
      },
      symbol_name: |tcx, instance| {
        let mut providers = Providers::default();
        rustc_symbol_mangling::provide(&mut providers);

        let instance = PlatformDriverData::<P>::with(tcx, |tcx, pd| {
          let dd = pd.dd();
          dd.stubber().unwrap()
            .map_instance(tcx, dd, instance)
        });

        (providers.symbol_name)(tcx, instance)
      },
      ..*providers
    }
  }
  pub fn provide_extern_overrides(providers: &mut Providers) {
    Self::provide_mir_overrides(providers);
    Self::providers_remote_and_local(providers);
  }

  pub fn providers_local(providers: &mut Providers) {
    use rustc_data_structures::svh::Svh;
    use rustc_hir::def_id::{LOCAL_CRATE};

    *providers = Providers {
      crate_hash: |_tcx, cnum| {
        assert_eq!(cnum, LOCAL_CRATE);
        // XXX?
        Svh::new(1)
      },
      crate_name: |_tcx, id| {
        assert_eq!(id, LOCAL_CRATE);
        Symbol::intern(CRATE_NAME)
      },
      crate_disambiguator: |tcx, cnum| {
        assert_eq!(cnum, LOCAL_CRATE);
        tcx.sess.crate_disambiguator.borrow().clone()
      },
      native_libraries: |_tcx, cnum| {
        assert_eq!(cnum, LOCAL_CRATE);
        Lrc::new(vec![])
      },
      link_args: |_tcx, cnum| {
        assert_eq!(cnum, LOCAL_CRATE);
        Lrc::new(vec![])
      },
      type_of: |tcx, def_id| {
        PlatformDriverData::<P>::with(tcx, |_tcx, pd| {
          pd.dd().expect_type_of(def_id)
        })
      },
      dependency_formats: |tcx, cnum| {
        use rustc_middle::middle::dependency_format::Linkage;
        use rustc_session::config::CrateType;

        assert_eq!(cnum, LOCAL_CRATE);
        let fake = tcx.all_crate_nums(LOCAL_CRATE)
          .iter()
          .map(|_| Linkage::NotLinked )
          .collect();
        vec![(CrateType::Cdylib, fake)].into()
      },
      ..*providers
    };
    Self::provide_mir_overrides(providers);
    Self::providers_remote_and_local(providers);
  }

  fn providers_remote_and_local(providers: &mut Providers) {
    use rustc_session::config::EntryFnType;

    *providers = Providers {
      fn_sig: |tcx, def_id| {
        use rustc_middle::ty::{Binder, FnSig, };

        let mut providers = Providers::default();
        rustc_metadata::provide_extern(&mut providers);

        // no stubbing here. We want the original fn sig

        let sig = (providers.fn_sig)(tcx, def_id);

        PlatformDriverData::<P>::with(tcx, |_tcx, pd| {
          if pd.dd().is_root(def_id) {
            // modify the abi:
            let sig = FnSig {
              abi: pd.dd().target_desc.kernel_abi.clone(),
              ..*sig.skip_binder()
            };
            Binder::bind(sig)
          } else {
            sig
          }
        })
      },
      reachable_non_generics,
      custom_intrinsic_mirgen: |tcx, def_id| {
        PlatformDriverData::<P>::with(tcx, |tcx, pd| {
          let name = tcx.item_name(def_id);
          debug!("custom intrinsic: {}", name);
          pd.dd()
            .intrinsics
            .read()
            .get(&name)
            .cloned()
        })
      },
      upstream_monomorphizations,
      upstream_monomorphizations_for,
      codegen_fn_attrs: |tcx, def_id| {
        let mut providers = Providers::default();
        rustc_typeck::provide(&mut providers);

        let mut attrs = (providers.codegen_fn_attrs)(tcx, def_id);

        PlatformDriverData::<P>::with(tcx, move |tcx, pd| {
          pd.platform.codegen_fn_attrs(tcx, &pd.driver_data,
                                       def_id, &mut attrs);


          if let Some(ref spirv) = attrs.spirv {
            info!("spirv attrs for {:?}: {:#?}", def_id, spirv);
          }

          attrs
        })
      },
      item_attrs: |tcx, def_id| {
        let mut providers = Providers::default();
        rustc_metadata::provide_extern(&mut providers);

        // Note: no replace here. For one, we'll introduce a cycle. And
        // we don't want to use the attributes of a different item anyway.

        let attrs = (providers.item_attrs)(tcx, def_id);

        PlatformDriverData::<P>::with(tcx, move |tcx, pd| {
          pd.platform.item_attrs(tcx, &pd.driver_data, def_id, attrs)
        })
      },
      entry_fn,
      collect_and_partition_mono_items: |tcx, cnum| {
        PlatformDriverData::<P>::with(tcx, move |tcx, pd| {
          collector::collect_and_partition_mono_items(tcx,
                                                      pd.dd(),
                                                      cnum)
        })
      },
      // we need to override this because otherwise rustc will get confused
      // which will eventually lead to an out of bounds slice index.
      // our synthetic crate will never have any `extern crate ...;`, so
      // overriding this should be a bit of an optimization anyway in
      // addition to being a bugfix.
      missing_extern_crate_item: |_tcx, _cnum| { true },
      ..*providers
    };

    fn entry_fn<'tcx>(_tcx: TyCtxt<'tcx>, _cnum: CrateNum) -> Option<(DefId, EntryFnType)> {
      None
    }
    fn reachable_non_generics<'tcx>(tcx: TyCtxt<'tcx>, _cnum: CrateNum)
      -> &'tcx DefIdMap<SymbolExportLevel>
    {
      // we need to recodegen everything
      tcx.arena.alloc(Default::default())
    }
    fn upstream_monomorphizations(tcx: TyCtxt, _cnum: CrateNum)
      -> &DefIdMap<FxHashMap<SubstsRef, CrateNum>>
    {
      // we never have any upstream monomorphizations.
      tcx.arena.alloc(Default::default())
    }
    fn upstream_monomorphizations_for(_tcx: TyCtxt, _def_id: DefId)
      -> Option<&FxHashMap<SubstsRef, CrateNum>>
    {
      None
    }
  }
}

pub struct TyCtxtLessKernelId {
  pub crate_name: String,
  pub crate_hash_hi: u64,
  pub crate_hash_lo: u64,
  pub index: u64,
}
impl TyCtxtLessKernelId {
  pub fn from_def_id(tcx: TyCtxt<'_>,
                     def_id: DefId) -> Self {
    let crate_name = tcx.crate_name(def_id.krate);
    let crate_name = format!("{}", crate_name);

    let disambiguator = tcx.crate_disambiguator(def_id.krate);
    let (crate_hash_hi, crate_hash_lo) = disambiguator.to_fingerprint().as_value();

    let index = def_id.index.as_usize() as u64;

    TyCtxtLessKernelId {
      crate_name,
      crate_hash_hi,
      crate_hash_lo,
      index,
    }
  }
}
