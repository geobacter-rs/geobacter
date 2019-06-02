use std::any::Any;
use std::collections::{BTreeMap, };
use std::error::{Error, };
use std::fs::File;
use std::io::{self, Read, };
use std::sync::mpsc::{channel, Sender, Receiver, RecvTimeoutError, };
use std::sync::{Arc, Weak, };
use std::time::Duration;
use std::path::{Path, };

use rustc;
use rustc::hir::{SpirVTypeSpec, SpirVAttrNode, };
use rustc::hir::def_id::{CrateNum, DefId};
use rustc::middle::exported_symbols::{SymbolExportLevel, };
use rustc::mir::{CustomIntrinsicMirGen, };
use rustc::session::config::{CrateType, };
use rustc::ty::query::Providers;
use rustc::ty::{self, TyCtxt, subst::Substs, Instance, };
use rustc::ty::{ParamEnv, FnDef, AdtDef, };
use rustc::util::nodemap::DefIdMap;
use rustc_driver::{self, driver::compute_crate_disambiguator, };
use rustc_data_structures::fx::{FxHashMap};
use rustc_data_structures::indexed_vec::Idx;
use rustc_data_structures::sync::{self, Lrc, };
use rustc_metadata;
use rustc_metadata::cstore::CStore;
use rustc_incremental;
use rustc_target::abi::{LayoutDetails, Variants, FieldPlacement,
                        VariantIdx, };
use syntax::ast;
use syntax::feature_gate;
use syntax_pos::symbol::{Symbol, InternedString, };

use crossbeam::sync::WaitGroup;

use lintrinsics::{DefIdFromKernelId, GetDefIdFromKernelId,
                  insert_all_intrinsics, LegionellaMirGen,
                  ExeModel, };
use lintrinsics::attrs::*;

use tempdir::TempDir;

use hsa_core::kernel::KernelId;
use lcore::*;
use lstd::vk_help::{StaticPipelineLayoutDesc, StaticShaderInterfaceDef, };

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

pub type CodegenShaderInterface = (StaticShaderInterfaceDef, StaticShaderInterfaceDef);

// TODO we need to create a talk to a "host codegen" so that we can ensure
// that adt's have the same layout in the shader/kernel as on the host.
// TODO XXX only one codegen query is allowed at a time.
// TODO codegen worker workers (ie codegen multiple functions concurrently)

#[derive(Clone, Copy, Eq, PartialEq, Debug, Hash)]
pub struct CodegenDesc {
  pub id: KernelId,
  pub exe_model: ExecutionModel,
  pub pipeline: StaticPipelineLayoutDesc,
  pub interface: Option<CodegenShaderInterface>,
  pub capabilities: Capabilities<'static, [Capability]>,
  pub extensions: &'static [&'static str],
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

/// DefId is used here because they *should* be identical over every
/// codegen, due to the shared CStore.
/// Additionally, we require all these queries to block, so that we can send
/// references of things. Normally, we would have no assertion that accel tcx
/// outlives the refs sent. It is still unsafe here!
pub(crate) enum HostQueryMessage {
  TyLayout {
    ty: &'static ty::Ty<'static>,
    wait: WaitGroup,
    ret: Sender<Result<LayoutDetails, Box<Error + Sync + Send>>>,
  },
}

pub(crate) enum Message {
  /// So we can know when to exit.
  AddAccel(Weak<Accelerator>),
  StartHostQuery {
    rx: Receiver<HostQueryMessage>,
  },
  Codegen {
    desc: CodegenDesc,

    //host_codegen: Sender<HostQueryMessage>,

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
                              Some(accel))
  }
  pub fn new_host(context: &Context,
                  desc: Arc<AcceleratorTargetDesc>)
    -> io::Result<CodegenComms>
  {

    WorkerTranslatorData::new(context, desc, None)
  }

  fn start_host_query_session(&self) -> Sender<HostQueryMessage> {
    let (accel_tx, host_rx) = channel();

    let msg = Message::StartHostQuery {
      rx: host_rx,
    };

    self.0.send(msg)
      .expect("the codegen thread crashed/exited");

    accel_tx
  }

  pub fn codegen(&self,
                 desc: CodegenDesc,
                 host: CodegenComms)
    -> Result<CodegenResults, error::Error>
  {
    let (tx, rx) = channel();

    //let host_codegen = host.start_host_query_session();

    let msg = Message::Codegen {
      desc,
      //host_codegen,
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

  pub(crate) unsafe fn sync_comms(self) -> CodegenUnsafeSyncComms {
    let CodegenComms(this) = self;
    UnsafeSyncSender(this)
  }
}
pub(crate) type CodegenUnsafeSyncComms = UnsafeSyncSender<Message>;
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
             accel: Option<&Arc<Accelerator>>)
    -> io::Result<CodegenComms>
  {
    use std::thread::Builder;

    use passes::lang_item::LangItemPass;
    use passes::compiler_builtins::CompilerBuiltinsReplacerPass;

    use rustc::middle::dependency_format::Dependencies;

    let (tx, rx) = channel();
    let accel = accel.map(Arc::downgrade);
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
            accels: accel.into_iter().collect(),
            sess,
            passes: vec![
              Box::new(LangItemPass),
              Box::new(CompilerBuiltinsReplacerPass),
            ],
            intrinsics: Default::default(),
          };
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

    /// Our code here runs amok of Rust's borrow checker. Which is why
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
        let krate = create_empty_hir_crate();
        let dep_graph = rustc::dep_graph::DepGraph::new_disabled();
        let mut forest = rustc::hir::map::Forest::new(krate, &dep_graph);
        let mut defs = rustc::hir::map::definitions::Definitions::new();
        let disambiguator = self.sess.crate_disambiguator
          .borrow()
          .clone();
        defs.create_root_def("jit-methods", disambiguator);
        let defs = defs;

        'msg: loop {
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

          // Recreate this every loop. You'll have to transmute to get around
          // lifetime error false positives.
          let mut arena = rustc::ty::AllArenas::new();

          match msg {
            Message::AddAccel(accel) => {
              break 'inner InternalMessage::AddAccel(accel);
            },
            Message::StartHostQuery { rx, } => {
              // ignore any errors
              let _ = {
                self.host_queries(context.clone(),
                                  context.cstore(),
                                  unsafe {
                                    ::std::mem::transmute(&mut arena)
                                  },
                                  unsafe {
                                    ::std::mem::transmute(&mut forest)
                                  },
                                  unsafe {
                                    ::std::mem::transmute(&defs)
                                  },
                                  &dep_graph,
                                  rx)
              };
            },
            Message::Codegen {
              desc,
              //host_codegen,
              ret,
            } => {
              let (host_codegen, _) = channel();
              let result = {
                self.codegen_kernel(desc,
                                    context.clone(),
                                    context.cstore(),
                                    unsafe {
                                      ::std::mem::transmute(&mut arena)
                                    },
                                    unsafe {
                                      ::std::mem::transmute(&mut forest)
                                    },
                                    unsafe {
                                      ::std::mem::transmute(&defs)
                                    },
                                    &dep_graph,
                                    host_codegen)
              };

              let _ = ret.send(result);
            },
          }

          continue 'msg;
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

  fn host_queries<'a, 'b>(&'a self,
                          _context: Context,
                          cstore: &'b CStore,
                          arena: &'b mut ty::AllArenas<'b>,
                          forest: &'b mut rustc::hir::map::Forest,
                          defs: &'b rustc::hir::map::definitions::Definitions,
                          _dep_graph: &rustc::dep_graph::DepGraph,
                          query_rx: Receiver<HostQueryMessage>)
    -> Result<(), error::Error>
    where 'a: 'b,
  {
    use rustc_driver::get_codegen_backend;

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

    let (tx, _) = channel();

    let tmpdir = TempDir::new("host-query-codegen")?;

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
      glob_map: Default::default(),
    };

    let mut intrinsics = self.intrinsics.clone();
    let (k, v) = LegionellaMirGen::new(ExeModel(None),
                                       &DefIdFromKernelIdGetter);
    let k = Symbol::intern(k.as_ref()).as_interned_str();
    assert!(intrinsics.insert(k, v).is_none());

    let driver_data = DriverData::new(self.context().unwrap(),
                                      self.accels.as_ref(),
                                      None,
                                      &self.accel_desc,
                                      vec![],
                                      self.passes.as_ref(),
                                      &intrinsics);
    let driver_data: DriverData<'static> = unsafe {
      ::std::mem::transmute(driver_data)
    };
    let driver_data = Box::new(driver_data) as Box<dyn Any + Send + Sync>;

    TyCtxt::create_and_enter(&self.sess, cstore,
                             local_providers,
                             extern_providers,
                             arena,
                             resolutions,
                             map_crate,
                             disk_cache,
                             "", tx, &out,
                             Some(driver_data), move |tcx: TyCtxt| {
        // Do some initialization of the DepGraph that can only be done with the
        // tcx available.
        rustc_incremental::dep_graph_tcx_init(tcx);

        for msg in query_rx.iter() {
          match msg {
            HostQueryMessage::TyLayout {
              ty,
              wait,
              ret,
            } => {
              error!("host query msg: TyLayout {{ ty: {:?}, }}", ty);
              let layout = tcx
                .layout_of(ParamEnv::reveal_all().and(ty))
                .map(|layout| layout.details )
                .map(clone_layout_details)
                .map_err(|err| {
                  format!("{}", err).into()
                });

              let _ = ret.send(layout);
              wait.wait();
            },
          }
        }
      });

    Ok(())
  }

  fn codegen_kernel<'a, 'b>(&'a self,
                            desc: CodegenDesc,
                            context: Context,
                            cstore: &'b CStore,
                            arena: &'b mut ty::AllArenas<'b>,
                            forest: &'b mut rustc::hir::map::Forest,
                            defs: &'b rustc::hir::map::definitions::Definitions,
                            dep_graph: &rustc::dep_graph::DepGraph,
                            host_codegen: Sender<HostQueryMessage>)
    -> Result<CodegenResults, error::Error>
    where 'a: 'b,
  {
    use rustc::hir::def_id::{DefId, DefIndex};
    use rustc_driver::get_codegen_backend;

    let id = desc.id;

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
      glob_map: Default::default(),
    };

    let mut intrinsics = self.intrinsics.clone();
    let (k, v) = LegionellaMirGen::new(ExeModel(Some(desc.exe_model)),
                                       &DefIdFromKernelIdGetter);
    let k = Symbol::intern(k.as_ref()).as_interned_str();
    assert!(intrinsics.insert(k, v).is_none());


    let conditions = vec![Condition::ExeModel(desc.exe_model.into())];

    let driver_data = DriverData::new(self.context().unwrap(),
                                      self.accels.as_ref(),
                                      Some(host_codegen),
                                      &self.accel_desc,
                                      conditions,
                                      self.passes.as_ref(),
                                      &intrinsics);
    let driver_data: DriverData<'static> = unsafe {
      ::std::mem::transmute(driver_data)
    };
    let driver_data = Box::new(driver_data) as Box<dyn Any + Send + Sync>;

    let kernel_symbol = TyCtxt::create_and_enter(&self.sess, cstore,
                                                 local_providers,
                                                 extern_providers,
                                                 arena,
                                                 resolutions,
                                                 map_crate,
                                                 disk_cache,
                                                 "", tx, &out,
                                                 Some(driver_data), |tcx: TyCtxt| -> Result<String, error::Error> {
        info!("output dir for {:?}: {}", def_id, out.out_directory.display());

        // Do some initialization of the DepGraph that can only be done with the
        // tcx available.
        rustc_incremental::dep_graph_tcx_init(tcx);

        let root = legionella_root_attrs(tcx,
                                         def_id,
                                         desc.exe_model,
                                         false);
        DriverData::with(tcx, move |_tcx, dd| {
          dd.init_root(root);
        });

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
      symbol: kernel_symbol,
      desc,
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

fn clone_layout_details(details: &LayoutDetails) -> LayoutDetails {
  LayoutDetails {
    variants: match details.variants {
      Variants::Single {
        index,
      } => Variants::Single {
        index,
      },
      Variants::Tagged {
        ref tag, ref variants,
      } => Variants::Tagged {
        tag: tag.clone(),
        variants: variants.iter()
          .map(clone_layout_details)
          .collect(),
      },
      Variants::NicheFilling {
        ref dataful_variant,
        ref niche_variants,
        ref niche,
        niche_start,
        ref variants,
      } => Variants::NicheFilling {
        dataful_variant: dataful_variant.clone(),
        niche_variants: niche_variants.clone(),
        niche: niche.clone(),
        niche_start,
        variants: variants.iter()
          .map(clone_layout_details)
          .collect(),
      },
    },
    fields: match details.fields {
      FieldPlacement::Union(u) => FieldPlacement::Union(u),
      FieldPlacement::Array {
        stride, count,
      } => FieldPlacement::Array {
        stride, count,
      },
      FieldPlacement::Arbitrary {
        ref offsets,
        ref memory_index,
      } => FieldPlacement::Arbitrary {
        offsets: offsets.clone(),
        memory_index: memory_index.clone(),
      },
    },
    abi: details.abi.clone(),
    align: details.align.clone(),
    size: details.size,
  }
}

fn output_types() -> rustc::session::config::OutputTypes {
  use rustc::session::config::*;

  let output = (OutputType::Bitcode, None);
  let ir_out = (OutputType::LlvmAssembly, None);
  let mut out = Vec::new();
  out.push(output);
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
  opts.crate_types.push(CrateType::Cdylib);
  opts.output_types = output_types();
  opts.optimize = OptLevel::No;
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
  opts.debugging_opts.polly = (opts.optimize == OptLevel::Aggressive);
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
  let modules = BTreeMap::new();

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
    modules,
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
  use rustc::hir::CodegenFnAttrs;
  use rustc::session::config::EntryFnType;

  *providers = Providers {
    fn_sig,
    reachable_non_generics,
    custom_intrinsic_mirgen,
    upstream_monomorphizations,
    upstream_monomorphizations_for,
    codegen_fn_attrs,
    item_attrs,
    entry_fn,
    .. *providers
  };

  fn entry_fn<'a, 'tcx>(_tcx: TyCtxt<'a, 'tcx, 'tcx>, _cnum: CrateNum) -> Option<(DefId, EntryFnType)> {
    None
  }
  fn fn_sig<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>, def_id: DefId) -> ty::PolyFnSig<'tcx> {
    use rustc::ty::{Binder, FnSig, };

    let mut providers = Providers::default();
    rustc_metadata::cstore::provide_extern(&mut providers);

    let def_id = replace(tcx, def_id);

    let sig = (providers.fn_sig)(tcx, def_id);

    DriverData::with(tcx, |_tcx, dd| {
      if dd.is_root(def_id) {
        // modify the abi:
        let sig = FnSig {
          abi: dd.target_desc.kernel_abi.clone(),
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
    Lrc::new(Default::default())
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

  fn codegen_fn_attrs<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>, id: DefId)
    -> CodegenFnAttrs
  {
    use rustc::hir::{SpirVAttrs, };
    let mut providers = Providers::default();
    rustc_typeck::provide(&mut providers);

    let id = replace(tcx, id);

    let mut attrs = (providers.codegen_fn_attrs)(tcx, id);

    DriverData::with(tcx, move |tcx, dd| {
      if dd.is_root(id) {
        let modes = dd.root()
          .execution_modes
          .iter()
          .map(|&mode| {
            match mode {
              ExecutionMode::LocalSize {
                x, y, z,
              } => {
                ("LocalSize".into(), vec![x as u64, y as _, z as _, ])
              },
              ExecutionMode::Xfb => {
                ("Xfb".into(), vec![])
              },
            }
          })
          .collect();

        attrs.spirv = Some(SpirVAttrs {
          storage_class: None,
          metadata: None,
          exe_model: Some(format!("{:?}", dd.root().exe_model())),
          exe_mode: Some(modes),
        });
      } else if tcx.is_static(id).is_some() {
        let inst = Instance::mono(tcx, id);
        let ty = inst.ty(tcx);
        // check for function types. We don't attach any spirv
        // metadata to functions.
        match ty.sty {
          FnDef(..) => {
            attrs.spirv = None;
          },
          _ => {
            let global_attrs = legionella_global_attrs(tcx,
                                                       dd.root().exe_model(),
                                                       inst,
                                                       false);

            if let Some(_) = global_attrs.spirv_builtin {
              // remove the linkage attr. This is used on the host to
              // prevent linker errors.
              attrs.linkage = None;
            }

            warn!("building metadata for {:?}", id);
            warn!("{:?} attrs: {:#?}", id, global_attrs);

            let mut metadata = build_spirv_metadata(tcx, dd, inst,
                                                &global_attrs);

            let sc = global_attrs.storage_class(&dd.root_conditions);
            match sc {
              Some(StorageClass::Uniform) |
              Some(StorageClass::StorageBuffer) => {
                if let Some(ref mut metadata) = metadata {
                  metadata.decorations.push(("Block".into(), vec![]));
                }
              },
              _ => {},
            }

            attrs.spirv = Some(SpirVAttrs {
              storage_class: sc.map(|sc| format!("{:?}", sc) ),
              metadata,
              exe_model: None,
              exe_mode: None,
            });
          },
        }
      }

      warn!("spirv metadata for {:?}: {:#?}", id, attrs.spirv);

      attrs
    })
  }
  /// Technically, this is never called for local DefIds (local DefIds use the
  /// HIR map to compute item attrs).
  fn item_attrs<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                          id: DefId)
    -> Lrc<[ast::Attribute]>
  {
    let mut providers = Providers::default();
    rustc_metadata::cstore::provide_extern(&mut providers);

    // Note: no replace here. For one, we'll introduce a cycle. And
    // we don't want to use the attributes of a different item anyway.

    let attrs = (providers.item_attrs)(tcx, id);

    DriverData::with(tcx, move |tcx, dd| {
      let out = legionella_cfg_attrs(tcx, &attrs,
                                     &dd.root_conditions);
      out
    })
  }
}

fn default_node() -> SpirVAttrNode {
  SpirVAttrNode {
    type_spec: default_ty_spec(),
    builtin: None,
    decorations: vec![],
  }
}
fn default_ty_spec() -> SpirVTypeSpec {
  SpirVTypeSpec::Struct(vec![])
}

fn build_spirv_adt_metadata<'a, 'b, 'tcx>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                                          dd: &'b DriverData<'tcx>,
                                          inst: Instance<'tcx>,

                                          ty: ty::Ty<'tcx>,
                                          adt_def: &'tcx AdtDef,
                                          substs: &'tcx Substs<'tcx>)
  -> SpirVAttrNode
{
  //let layout = dd.host_layout_of(ty);
  let layout = tcx.layout_of(ParamEnv::reveal_all().and(ty))
    .unwrap();

  warn!("{:?} layout: {:#?}", ty, layout);

  let cx = ty::layout::LayoutCx {
    tcx,
    param_env: ParamEnv::reveal_all(),
  };

  // TODO won't be true for enums. Not sure what the layout would
  // look like.
  assert_eq!(adt_def.variants.len(), 1, "TODO: {:?}, layout: {:?}",
             adt_def, layout);

  let mut nodes = vec![];

  for i in layout.fields.index_by_increasing_offset() {
    let offset = layout.details.fields.offset(i as usize);
    let field = layout.field(&cx, i).unwrap();
    let ty = field.ty;
    let ty = tcx
      .subst_and_normalize_erasing_regions(substs,
                                           ParamEnv::reveal_all(),
                                           &ty);
    let mut node = build_spirv_ty_metadata(tcx, dd, inst, ty);

    node.decorations.push(("Offset".into(), vec![offset.bytes() as _]));

    nodes.push(node);
  }

  let decorations = vec![];

  SpirVAttrNode {
    type_spec: SpirVTypeSpec::Struct(nodes),
    builtin: None,
    decorations,
  }
}

fn build_spirv_ty_metadata<'a, 'b, 'tcx>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                                         dd: &'b DriverData<'tcx>,
                                         inst: Instance<'tcx>,

                                         ty: ty::Ty<'tcx>)
  -> SpirVAttrNode
{
  use rustc::ty::*;

  // we have to be careful of recursion here
  // we don't maintain a set of visited types but in this case we can skip
  // some types: we require `T: Sized + Copy + 'static` (TODO pointers?).

  // TODO: should we allow builtins as struct members? I want to say no.

  info!("build_spirv_ty_metadata: ty.sty = {:?}", ty.sty);

  let ty = tcx
    .subst_and_normalize_erasing_regions(inst.substs,
                                         ParamEnv::reveal_all(),
                                         &ty);

  match ty.sty {
    Bool | Char | Int(_) | Uint(_) | Float(_) => {
      default_node()
    },
    Adt(adt_def, _substs) if adt_def.repr.simd() => {
      let layout = tcx.layout_of(ParamEnv::reveal_all().and(ty))
        .unwrap();

      warn!("{:?} layout: {:#?}", ty, layout);
      // TODO someday we'll want to allow `RowMajor` and `ColMajor` etc
      default_node()
    },
    Adt(adt_def, substs) if adt_def.repr.transparent() => {
      // extract the inner type which this type wraps. This can be important
      // for wrappers which wrap SIMD types, for example SPIRV Vector and Matrix
      // types.
      let idx = VariantIdx::new(0);
      let field = adt_def.variants[idx].fields[0].did;
      let field_ty = tcx.type_of(field);

      let field_ty = tcx
        .subst_and_normalize_erasing_regions(substs,
                                             ParamEnv::reveal_all(),
                                             &field_ty);
      trace!("repr(transparent): extracted {:?}", field_ty);
      build_spirv_ty_metadata(tcx, dd, inst, field_ty)
    },
    Tuple(elements) => {
      //let layout = dd.host_layout_of(ty);
      let layout = tcx
        .layout_of(ParamEnv::reveal_all().and(ty))
        .unwrap()
        .details;

      let mut nodes = vec![];

      for (idx, element) in elements.iter().enumerate() {
        let offset = layout.fields.offset(idx);
        let mut node = build_spirv_ty_metadata(tcx, dd, inst, element);
        node.decorations.push(("Offset".into(), vec![offset.bytes() as _]));

        nodes.push(node);
      }

      SpirVAttrNode {
        type_spec: SpirVTypeSpec::Struct(nodes),
        builtin: None,
        decorations: vec![],
      }
    },
    Adt(adt_def, substs) => {
      build_spirv_adt_metadata(tcx, dd, inst, ty, adt_def, substs)
    },
    Array(inner, _count) => {
      let type_spec = SpirVTypeSpec::Array(Box::new(build_spirv_ty_metadata(tcx, dd, inst,
                                                                            inner)));

      let layout = tcx.layout_of(ParamEnv::reveal_all().and(inner))
        .unwrap();

      let node = SpirVAttrNode {
        type_spec,
        builtin: None,
        decorations: vec![("ArrayStride".into(),
                           vec![layout.details.size.bytes() as _])],
        //decorations: vec![],
      };
      node
    },

    _ => {
      panic!("shouldn't be allowed or unimplemented TODO: {:?}", ty);
    },
  }
}

fn build_spirv_metadata<'a, 'b, 'tcx>(tcx: TyCtxt<'_, 'tcx, 'tcx>,
                                      dd: &'b DriverData<'tcx>,
                                      inst: Instance<'tcx>,
                                      attrs: &GlobalAttrs)
  -> Option<SpirVAttrNode>
{
  let ty = inst.ty(tcx);

  // extract the pointer
  let ty = if let ty::RawPtr(tm) = ty.sty {
    tm.ty
  } else {
    // this probably isn't correct...
    ty
  };

  let mut node = build_spirv_ty_metadata(tcx, dd, inst, ty);

  // descriptor set/binding is only allowed on top level global statics.
  // so this is only processed in this non-recursive function.
  let (opt_set, opt_binding) =
    optional_descriptor_set_binding_nums(tcx, inst.def_id());

  if let Some(builtin) = attrs.spirv_builtin {
    node.decorations.push(("BuiltIn".into(), vec![builtin as _]));
  }
  if let Some(set_num) = opt_set {
    node.decorations.push(("DescriptorSet".into(), vec![set_num as _]));
  }
  if let Some(binding_num) = opt_binding {
    node.decorations.push(("Binding".into(), vec![binding_num as _]));
  }

  Some(node)
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
