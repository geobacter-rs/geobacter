use std::any::Any;
use std::collections::{BTreeMap, };
use std::fs::File;
use std::io::{self, Read, };
use std::sync::mpsc::{channel, Sender, Receiver, RecvTimeoutError, };
use std::sync::{Arc, Weak, };
use std::time::Duration;
use std::path::{Path, };

use rustc;
use rustc::hir::def_id::{CrateNum, DefId};
use rustc::middle::exported_symbols::{SymbolExportLevel, };
use rustc::mir::{CustomIntrinsicMirGen, };
use rustc::session::config::{CrateType, };
use rustc::ty::query::Providers;
use rustc::ty::{self, TyCtxt, subst::Substs, Instance, };
use rustc::ty::{Const, Closure, ParamEnv, Ref, FnDef, };
use rustc::util::nodemap::DefIdMap;
use rustc_driver::{self, driver::compute_crate_disambiguator, };
use rustc_data_structures::fx::{FxHashMap};
use rustc_data_structures::sync::{self, Lrc, RwLock, };
use rustc_metadata;
use rustc_metadata::cstore::CStore;
use rustc_incremental;
use rustc_target::abi::{Size, };
use syntax::feature_gate;
use syntax_pos::symbol::{Symbol, InternedString, };

use legionella_intrinsics::{DefIdFromKernelId, GetDefIdFromKernelId,
                            insert_all_intrinsics, LegionellaMirGen,
                            IsHost, };

use tempdir::TempDir;

use hsa_core::kernel::KernelId;

use {Accelerator, AcceleratorTargetDesc,
     context::Context, context::WeakContext, };
use utils::UnsafeSyncSender;

use passes::{Pass, };

use self::error::IntoErrorWithKernelId;
pub use self::driver_data::DriverData;

mod collector;
pub mod error;
mod driver_data;

use super::products::*;

#[doc(hidden)]
pub enum HostTuplePadding {
  Arg(usize, Size),
  Pad(Size),
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
  HostGetKernArgPadding {
    kernel: DefId,
    ret: Sender<Result<Vec<HostTuplePadding>, error::Error>>,
  },
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
  pub fn new(context: &Context,
             accel_desc: Arc<AcceleratorTargetDesc>,
             accel: &Arc<Accelerator>)
    -> io::Result<CodegenComms>
  {
    WorkerTranslatorData::new(context, accel_desc,
                              accel)
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

struct DefIdFromKernelIdGetter;
impl GetDefIdFromKernelId for DefIdFromKernelIdGetter {
  fn with_self<'a, 'tcx, F, R>(tcx: TyCtxt<'a, 'tcx, 'tcx>, f: F) -> R
    where F: FnOnce(&dyn DefIdFromKernelId) -> R,
          'tcx: 'a
  {
    DriverData::with(tcx, move |_tcx, dd| {
      f(dd as &dyn DefIdFromKernelId)
    })
  }
}

pub struct WorkerTranslatorData {
  pub context: WeakContext,
  pub accel_desc: Arc<AcceleratorTargetDesc>,
  pub accels: Vec<Weak<Accelerator>>,
  pub sess: rustc::session::Session,
  pub passes: Vec<Box<Pass>>,
  intrinsics: FxHashMap<InternedString, Lrc<dyn CustomIntrinsicMirGen>>,
}
impl WorkerTranslatorData {
  pub fn new(ctx: &Context,
             accel_desc: Arc<AcceleratorTargetDesc>,
             accel: &Arc<Accelerator>)
    -> io::Result<CodegenComms>
  {
    use std::thread::Builder;

    use passes::alloc::AllocPass;
    use passes::lang_item::LangItemPass;
    use passes::panic::PanicPass;
    use passes::compiler_builtins::CompilerBuiltinsReplacerPass;

    use rustc::middle::dependency_format::Dependencies;

    let (tx, rx) = channel();
    let accel = Arc::downgrade(&accel);
    let context = ctx.clone();

    let name = format!("codegen thread for {}",
                       accel_desc.target.llvm_target);

    let f = move || {
      info!("codegen thread for {} startup",
            accel_desc.target.llvm_target);

      let weak_context = context.downgrade_ref();

      let ctxt = context.clone();
      ctxt.syntax_globals().with(move || {
        let ctxt = context.clone();
        with_rustc_session(&ctxt, move |mut sess| {
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

          let mut data = WorkerTranslatorData {
            context: weak_context,
            accel_desc,
            accels: vec![accel],
            sess,
            passes: vec![
              Box::new(LangItemPass),
              Box::new(AllocPass),
              Box::new(PanicPass),
              Box::new(CompilerBuiltinsReplacerPass),
            ],
            intrinsics: Default::default(),
          };
          insert_all_intrinsics(&DefIdFromKernelIdGetter,
          |k, v| {
            let k = Symbol::intern(k.as_ref()).as_interned_str();
            assert!(data.intrinsics.insert(k, v).is_none());
          });
          let (k, v) = LegionellaMirGen::new(IsHost(false),
                                             &DefIdFromKernelIdGetter);
          let k = Symbol::intern(k.as_ref()).as_interned_str();
          assert!(data.intrinsics.insert(k, v).is_none());

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

    /// Our code here run amok of Rust's borrow checker. Which is why
    /// this code has become pretty ugly. Sorry 'bout that.

    enum InternalMessage {
      Timeout,
      AddAccel(Weak<Accelerator>),
    }

    let to = Duration::from_secs(10);

    let mut recv_msg = None;

    'outer: loop {
      let internal_msg = 'inner: loop {
        let context = match self.context() {
          Ok(ctxt) => ctxt,
          Err(_) => {
            // the context can't be resurrected.
            return;
          },
        };
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

        loop {
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
            Err(RecvTimeoutError::Disconnected) => {
              return;
            },
          };

          match msg {
            Message::AddAccel(accel) => {
              break 'inner InternalMessage::AddAccel(accel);
            },
            Message::HostGetKernArgPadding {
              kernel, ret,
            } => {
              let _ = self.compute_host_tuple_padding(kernel, context.clone(),
                                              context.cstore(),
                                              &arena,
                                              unsafe {
                                                ::std::mem::transmute(&mut forest)
                                              },
                                              unsafe {
                                                ::std::mem::transmute(&defs)
                                              },
                                              &dep_graph);
            },
            Message::HostCreateFunc {
              ..
            } => {
              unimplemented!();
            },
            Message::Codegen {
              id,
              host_accel: _,
              ret,
            } => {
              let result = self.codegen_kernel(id,
                                               context.clone(),
                                               context.cstore(),
                                               &arena,
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
      };

      match internal_msg {
        InternalMessage::Timeout => { },
        InternalMessage::AddAccel(accel) => {
          self.accels.push(accel);
          continue 'outer;
        },
      }

      let live = self.context.upgrade().is_some();
      let live = live && self.accels.iter()
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

  fn context(&self) -> Result<Context, error::Error> {
    self.context.upgrade()
      .ok_or(error::Error::ContextDead)
  }

  fn compute_host_tuple_padding<'a, 'b>(&'a self, def_id: DefId, context: Context,
                                        cstore: &'b CStore,
                                        arena: &'b rustc::ty::AllArenas<'b>,
                                        forest: &'b mut rustc::hir::map::Forest,
                                        defs: &'b rustc::hir::map::definitions::Definitions,
                                        dep_graph: &rustc::dep_graph::DepGraph)
    -> Result<Vec<HostTuplePadding>, error::Error>
    where 'a: 'b,
  {
    self.with_tcx(def_id, context, cstore, arena, forest, defs, dep_graph, |tcx| {
      let parent_substs = tcx.empty_substs_for_def_id(def_id);
      let reveal_all = ParamEnv::reveal_all();
      let instance = match tcx.type_of(def_id).sty {
        Ref(_, &ty::TyS {
          sty: FnDef(def_id, subs),
          ..
        }, ..) |
        FnDef(def_id, subs) => {
          let subs = tcx
            .subst_and_normalize_erasing_regions(parent_substs,
                                                 reveal_all,
                                                 &subs);

          rustc::ty::Instance::resolve(tcx, reveal_all, def_id, subs)
            .expect("must be resolvable")
        },

        Ref(_, &ty::TyS {
          sty: Closure(def_id, subs),
          ..
        }, ..) |
        Closure(def_id, subs) => {
          let subs = tcx
            .subst_and_normalize_erasing_regions(parent_substs,
                                                 reveal_all,
                                                 &subs);

          let env = subs.closure_kind(def_id, tcx);
          Instance::resolve_closure(tcx, def_id, subs, env)
        },

        _ => {
          // TODO send an error back
          unreachable!("can't expand the type of this item: {}",
                       tcx.item_path_str(def_id));
        },
      };

      let ty = instance.ty(tcx);
      let sig = ty.fn_sig(tcx);
      let sig = tcx.normalize_erasing_late_bound_regions(reveal_all, &sig);

      let mut tuple_elems = vec![tcx.mk_mut_ref(tcx.types.re_erased, sig.output())];
      tuple_elems.extend(sig.inputs());
      let tuple_ty = tcx.mk_tup(tuple_elems.iter());
      unimplemented!();
    })
  }

  fn with_tcx<'a, 'b, F, R>(&'a self, def_id: DefId, context: Context,
                            cstore: &'b CStore,
                            arena: &'b rustc::ty::AllArenas<'b>,
                            forest: &'b mut rustc::hir::map::Forest,
                            defs: &'b rustc::hir::map::definitions::Definitions,
                            dep_graph: &rustc::dep_graph::DepGraph,
                            f: F)
    -> Result<R, error::Error>
    where F: for<'z> Fn(TyCtxt<'z, 'b, 'b>) -> Result<R, error::Error>,
          'a: 'b,
  {
    use rustc::hir::def_id::{DefId, DefIndex};
    use rustc_driver::get_codegen_backend;

    assert!(!def_id.is_local());

    let codegen = get_codegen_backend(&self.sess);

    // extern only providers:
    let mut local_providers = rustc::ty::query::Providers::default();
    rustc_driver::driver::default_provide(&mut local_providers);
    codegen.provide(&mut local_providers);
    providers_local(&mut local_providers);

    let mut extern_providers = local_providers.clone();
    rustc_driver::driver::default_provide_extern(&mut extern_providers);
    codegen.provide_extern(&mut extern_providers);
    provide_extern_overrides(&mut extern_providers);

    let disk_cache = rustc_incremental::load_query_result_cache(&self.sess);

    let (tx, rx) = channel();

    let tmpdir = TempDir::new("hsa-runtime-codegen")?;

    let out = rustc::session::config::OutputFilenames {
      out_directory: tmpdir.path().into(),
      out_filestem: "codegen.elf".into(),
      single_output_file: None,
      extra: "".into(),
      outputs: output_types(),
    };

    let map_crate = rustc::hir::map::map_crate(&self.sess, cstore,
                                               forest, defs);
    let resolutions = rustc::ty::Resolutions {
      freevars: Default::default(),
      trait_map: Default::default(),
      maybe_unused_trait_imports: Default::default(),
      maybe_unused_extern_crates: Default::default(),
      export_map: Default::default(),
      extern_prelude: Default::default(),
    };

    let driver_data = DriverData {
      context: self.context().unwrap(),
      accels: self.accels.as_ref(),

      root: def_id,

      passes: self.passes.as_ref(),

      replaced_def_ids: RwLock::new(Default::default()),
      type_of: RwLock::new(Default::default()),
      intrinsics: &self.intrinsics,
    };
    let driver_data: DriverData<'static> = unsafe {
      ::std::mem::transmute(driver_data)
    };
    let driver_data = Box::new(driver_data) as Box<dyn Any + Send + Sync>;

    let out = TyCtxt::create_and_enter(&self.sess, cstore,
                                       local_providers,
                                       extern_providers,
                                       &arena,
                                       resolutions,
                                       map_crate,
                                       disk_cache,
                                       "", tx, &out,
                                       Some(driver_data), move |tcx: TyCtxt| -> Result<R, error::Error> {
        // Do some initialization of the DepGraph that can only be done with the
        // tcx available.
        rustc_incremental::dep_graph_tcx_init(tcx);

        Ok(f(tcx)?)
      })?;

    Ok(out)
  }

  fn codegen_kernel<'a, 'b>(&'a self, id: KernelId, context: Context,
                            cstore: &'b CStore,
                            arena: &'b rustc::ty::AllArenas<'b>,
                            forest: &'b mut rustc::hir::map::Forest,
                            defs: &'b rustc::hir::map::definitions::Definitions,
                            dep_graph: &rustc::dep_graph::DepGraph)
    -> Result<CodegenResults, error::Error>
    where 'a: 'b,
  {
    use rustc::hir::def_id::{DefId, DefIndex};
    use rustc_driver::get_codegen_backend;

    let crate_num = self
      .lookup_crate_num(id, context.cstore())
      .ok_or_else(|| {
        error::Error::NoCrateMetadata(id)
      })?;

    info!("translating defid {:?}:{}", crate_num, id.index);

    let def_id = DefId {
      krate: crate_num,
      index: DefIndex::from_raw_u32(id.index as u32),
    };
    assert!(!def_id.is_local());

    let codegen = get_codegen_backend(&self.sess);

    // extern only providers:
    let mut local_providers = rustc::ty::query::Providers::default();
    rustc_driver::driver::default_provide(&mut local_providers);
    codegen.provide(&mut local_providers);
    providers_local(&mut local_providers);

    let mut extern_providers = local_providers.clone();
    rustc_driver::driver::default_provide_extern(&mut extern_providers);
    codegen.provide_extern(&mut extern_providers);
    provide_extern_overrides(&mut extern_providers);

    let disk_cache = rustc_incremental::load_query_result_cache(&self.sess);

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

    let map_crate = rustc::hir::map::map_crate(&self.sess, cstore,
                                               forest, defs);
    let resolutions = rustc::ty::Resolutions {
      freevars: Default::default(),
      trait_map: Default::default(),
      maybe_unused_trait_imports: Default::default(),
      maybe_unused_extern_crates: Default::default(),
      export_map: Default::default(),
      extern_prelude: Default::default(),
    };

    let driver_data = DriverData {
      context: self.context().unwrap(),
      accels: self.accels.as_ref(),

      root: def_id,

      passes: self.passes.as_ref(),

      replaced_def_ids: RwLock::new(Default::default()),
      type_of: RwLock::new(Default::default()),
      intrinsics: &self.intrinsics,
    };
    let driver_data: DriverData<'static> = unsafe {
      ::std::mem::transmute(driver_data)
    };
    let driver_data = Box::new(driver_data) as Box<dyn Any + Send + Sync>;

    let kernel_symbol = TyCtxt::create_and_enter(&self.sess, cstore,
                                                 local_providers,
                                                 extern_providers,
                                                 &arena,
                                                 resolutions,
                                                 map_crate,
                                                 disk_cache,
                                                 "", tx, &out,
                                                 Some(driver_data), |tcx: TyCtxt| -> Result<String, error::Error> {
        info!("output dir for {:?}: {}", def_id, out.out_directory.display());

        // Do some initialization of the DepGraph that can only be done with the
        // tcx available.
        rustc_incremental::dep_graph_tcx_init(tcx);

        let trans_data = codegen.codegen_crate(tcx, rx);

        codegen.join_codegen_and_link(trans_data, &self.sess, dep_graph, &out)
          .map_err(|err| {
            error!("codegen failed: `{:?}`!", err);
            error::Error::Codegen(id)
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

  fn lookup_crate_num(&self, kernel_id: KernelId,
                      cstore: &CStore) -> Option<CrateNum> {
    let mut out = None;
    let needed_fingerprint =
      (kernel_id.crate_hash_hi,
       kernel_id.crate_hash_lo);
    cstore.iter_crate_data(|num, data| {
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
  let asm    = (OutputType::Assembly, None);
  let ir_out = (OutputType::LlvmAssembly, None);
  let mut out = Vec::new();
  out.push(output);
  out.push(asm);
  out.push(ir_out);
  OutputTypes::new(&out[..])
}

pub fn with_rustc_session<F, R>(ctx: &Context, f: F) -> R
  where F: FnOnce(rustc::session::Session) -> R + sync::Send,
        R: sync::Send,
{
  use rustc_driver::driver::spawn_thread_pool;

  let opts = create_rustc_options(ctx);
  spawn_thread_pool(opts, move |opts| {
    let registry = rustc_driver::diagnostics_registry();
    let sess = rustc::session::build_session(opts, None, registry);
    f(sess)
  })
}
pub fn create_rustc_options(_ctx: &Context) -> rustc::session::config::Options {
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
  opts.cg.no_prepopulate_passes = false;
  if opts.cg.no_prepopulate_passes {
    opts.cg.passes.push("name-anon-globals".into());
  }
  opts.debugging_opts.print_llvm_passes = false;
  opts.debugging_opts.polly = true;
  opts.cg.llvm_args.push("-polly-run-inliner".into());
  opts.cg.llvm_args.push("-polly-register-tiling".into());
  opts.cg.llvm_args.push("-polly-check-vectorizable".into());
  opts.cg.llvm_args.push("-enable-polly-aligned".into());
  // TODO: -polly-target=gpu produces host side code which
  // then triggers the gpu side code.
  //opts.cg.llvm_args.push("-polly-target=gpu".into());
  opts.cg.llvm_args.push("-polly-vectorizer=polly".into());
  opts.cg.llvm_args.push("-polly-position=early".into());
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

fn replace<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                     def_id: DefId) -> DefId
{
  DriverData::with(tcx, |tcx, data| {
    data.replace_def_id(tcx, def_id)
  })
}

pub fn provide_mir_overrides(providers: &mut Providers) {
  *providers = Providers {
    is_mir_available: |tcx, def_id| {
      let mut providers = Providers::default();
      rustc_metadata::cstore::provide_extern(&mut providers);

      let def_id = replace(tcx, def_id);

      (providers.is_mir_available)(tcx, def_id)
    },
    optimized_mir: |tcx, def_id| {
      let mut providers = Providers::default();
      rustc_metadata::cstore::provide_extern(&mut providers);

      let def_id = replace(tcx, def_id);

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
      Lrc::new(vec![])
    },
    link_args: |_tcx, cnum| {
      assert_eq!(cnum, LOCAL_CRATE);
      Lrc::new(vec![])
    },
    type_of: |tcx, def_id| {
      DriverData::with(tcx, |_tcx, dd| {
        dd.expect_type_of(def_id)
      })
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

    let def_id = replace(tcx, def_id);

    let sig = (providers.fn_sig)(tcx, def_id);

    DriverData::with(tcx, |_tcx, dd| {
      if dd.is_root(def_id) {
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
    DriverData::with(tcx, |tcx, dd| {
      let name = tcx.item_name(def_id);
      info!("custom intrinsic: {}", name);
      dd.intrinsics
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
