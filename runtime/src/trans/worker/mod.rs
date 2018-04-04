
use std::cell::{RefCell};
use std::collections::{BTreeMap};
use std::error::Error;
use std::sync::mpsc::{channel, Sender, Receiver, RecvTimeoutError};
use std::sync::{Arc, Weak, RwLock, Mutex};
use std::ops::{Deref};
use std::time::Duration;
use std::thread::{spawn};

use rustc;
use rustc::hir::def_id::{CrateNum, DefId};
use rustc::ty::maps::Providers;
use rustc::ty::{TyCtxt};
use rustc_driver;
use rustc_data_structures::fx::{FxHashMap};
use rustc_metadata;
use rustc_trans_utils;
use rustc_mir;
use rustc_incremental;

use tempdir::TempDir;

use hsa_core::kernel_info::KernelId;

use module::{ModuleData, ErasedModule};
use {Accelerator, AcceleratorId};
use metadata::{Metadata, DummyMetadataLoader, CrateMetadata, CrateMetadataLoader,
               CrateNameHash};

use passes::{Pass, PassType};

mod tls;

pub enum Message {
  AddAccel(Weak<Accelerator>),
  Trans {
    id: KernelId,
    ret: Sender<Result<(), ()>>,
  }
}

#[derive(Clone)]
pub struct TranslatorData(Sender<Message>);
impl TranslatorData {
  pub fn new(accel: Arc<Accelerator>, metadata: Arc<Mutex<Vec<Metadata>>>)
    -> TranslatorData
  {
    WorkerTranslatorData::new(accel, metadata)
  }
  pub fn translate(&self, id: KernelId) -> Result<(), Box<Error>> {
    let (tx, rx) = channel();
    let msg = Message::Trans {
      id,
      ret: tx,
    };

    self.0.send(msg)?;

    rx.recv()?
      .map_err(|()| {
        format!("generic translation error :^(")
      })?;

    Ok(())
  }
}

#[derive(Clone, Copy)]
pub struct TranslatorCtxtRef<'a, 'b, 'c, 'tcx>
  where 'a: 'c,
        'tcx: 'b,
{
  tcx: TyCtxt<'b, 'tcx, 'tcx>,
  worker: &'c WorkerTranslatorData<'a>,
}

impl<'a, 'b, 'c, 'tcx> TranslatorCtxtRef<'a, 'b, 'c, 'tcx>
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

    let mut new_def_id = None;
    for pass in self.worker.passes.iter() {
      let ty = pass.pass_type();
      let replaced = match ty {
        PassType::Replacer(f) => {
          f(*self, id)
        },
        _ => { continue; }
      };
      match replaced {
        Some(id) => {
          new_def_id = Some(id);
        },
        None => { continue; }
      }
    }

    let new_def_id = new_def_id.unwrap_or(id);

    self.worker.replaced_def_ids.borrow_mut().insert(id, new_def_id);
    new_def_id
  }
}

impl<'a, 'b, 'c, 'tcx> Deref for TranslatorCtxtRef<'a, 'b, 'c, 'tcx>
  where 'a: 'c,
{
  type Target = TyCtxt<'b, 'tcx, 'tcx>;
  fn deref(&self) -> &TyCtxt<'b, 'tcx, 'tcx> { &self.tcx }
}


pub struct WorkerTranslatorData<'a> {
  pub accels: Vec<Weak<Accelerator>>,
  pub sess: &'a rustc::session::Session,
  pub cstore: &'a rustc_metadata::cstore::CStore,
  pub passes: Vec<Box<Pass>>,
  replaced_def_ids: RefCell<FxHashMap<DefId, DefId>>,
}
impl<'a> WorkerTranslatorData<'a> {
  pub fn new(accel: Arc<Accelerator>, metadata: Arc<Mutex<Vec<Metadata>>>)
    -> TranslatorData
  {
    use passes::alloc::AllocPass;
    use passes::lang_item::LangItemPass;
    use passes::panic::PanicPass;
    use passes::compiler_builtins::CompilerBuiltinsReplacerPass;
    let (tx, rx) = channel();

    let f = move || {
      let mut sess = create_rustc_session();
      sess.target.target.llvm_target = accel.llvm_target();
      sess.target.target.arch = accel.target_arch();
      sess.target.target.options.cpu = accel.target_cpu();
      sess.target.target.data_layout = "e-p:64:64-p1:64:64-p2:64:64-p3:32:32-\
                                        p4:32:32-p5:32:32-i64:64-v16:16-v24:32-\
                                        v32:32-v48:64-v96:128-v192:256-v256:256-\
                                        v512:512-v1024:1024-v2048:2048-n32:64-A5"
        .into();

      let md_loader = Box::new(DummyMetadataLoader);
      let md_loader = md_loader as Box<rustc::middle::cstore::MetadataLoader>;
      let cstore = rustc_metadata::cstore::CStore::new(md_loader);

      let mut data = WorkerTranslatorData {
        accels: vec![Arc::downgrade(&accel)],
        sess: &sess,
        cstore: &cstore,
        passes: vec![//Box::new(LangItemPass),
                     Box::new(AllocPass),
                     Box::new(PanicPass),
                     Box::new(CompilerBuiltinsReplacerPass),],
        replaced_def_ids: Default::default(),
      };
      {
        let metadata = metadata.lock().unwrap();
        data.initialize_cstore(metadata.as_ref());
      }

      data.thread(rx);
    };

    let _ = spawn(f);

    TranslatorData(tx)
  }

  fn thread(&mut self, rx: Receiver<Message>) {

    let to = Duration::from_secs(1);

    'outer: loop {
      'inner: loop {
        let msg = match rx.recv_timeout(to) {
          Ok(msg) => msg,
          Err(RecvTimeoutError::Timeout) => {
            break 'inner;
          },
          Err(RecvTimeoutError::Disconnected) => { return; },
        };

        match msg {
          Message::AddAccel(accel) => {
            self.accels.push(accel);
          },
          Message::Trans {
            id,
            ret,
          } => {
            let arena = rustc::ty::AllArenas::new();
            let krate = create_empty_hir_crate();
            let dep_graph = rustc::dep_graph::DepGraph::new_disabled();
            let mut forest = rustc::hir::map::Forest::new(krate, &dep_graph);
            let mut defs = rustc::hir::map::definitions::Definitions::new();
            defs.create_root_def("jit-methods",
                                 self.sess.crate_disambiguator.borrow()
                                   .as_ref()
                                   .unwrap()
                                   .clone());
            let defs = defs;

            let result = self.trans_kernel(id, &arena,
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

        continue 'outer;
      }

      let live = self.accels.iter().any(|a| a.upgrade().is_some());
      if !live { return; }
    }
  }

  fn initialize_cstore(&mut self, md: &[Metadata]) {
    let mut loader = CrateMetadataLoader::default();
    let CrateMetadata(meta) = loader.build(md, &self.sess)
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

  fn trans_kernel<'b>(&self, id: KernelId, arena: &'a rustc::ty::AllArenas<'b>,
                      forest: &'b mut rustc::hir::map::Forest,
                      defs: &'b rustc::hir::map::definitions::Definitions,
                      dep_graph: &rustc::dep_graph::DepGraph)
    -> Result<(), ()>
    where 'a: 'b,
  {
    use rustc::hir::def_id::{DefId, DefIndex};
    use rustc::session::config::OutputTypes;

    let crate_num = self.lookup_crate_num(id)
      .ok_or_else(|| {
        println!("crate metadata missing for `{}`", id.crate_name);
        ()
      })?;

    println!("translating defid => {:?}:{}", crate_num, id.index);

    let def_id = DefId {
      krate: crate_num,
      index: DefIndex::from_raw_u32(id.index as u32),
    };

    let trans = super::LlvmTransCrate::new(self.sess, def_id);
    let trans = Box::new(trans) as Box<rustc_trans_utils::trans_crate::TransCrate>;

    // extern only providers:
    let mut extern_providers = rustc::ty::maps::Providers::default();
    stub_providers(&mut extern_providers);
    trans.provide(&mut extern_providers);
    trans.provide_extern(&mut extern_providers);
    rustc::traits::provide(&mut extern_providers);
    rustc::ty::provide(&mut extern_providers);
    rustc_trans_utils::symbol_names::provide(&mut extern_providers);
    rustc_metadata::cstore::provide_extern(&mut extern_providers);
    extern_providers.const_eval = rustc_mir::interpret::const_eval_provider;
    provide_extern_overrides(&mut extern_providers);

    let mut local_providers = extern_providers.clone();
    stub_providers(&mut local_providers);
    trans.provide(&mut local_providers);
    rustc::traits::provide(&mut local_providers);
    rustc_metadata::cstore::provide(&mut local_providers);
    rustc_mir::provide(&mut local_providers);
    providers_local(&mut local_providers);

    let disk_cache = rustc_incremental::load_query_result_cache(self.sess);

    let (tx, rx) = channel();

    let tmpdir = TempDir::new("hsa-trans-runtime")
      .expect("can't create output temp dir");

    let out = rustc::session::config::OutputFilenames {
      out_directory: tmpdir.path().into(),
      out_filestem: "trans.elf".into(),
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
    };

    rustc::ty::TyCtxt::create_and_enter(self.sess,
                                        self.cstore,
                                        local_providers,
                                        extern_providers,
                                        &arena,
                                        Some((resolutions, map_crate)),
                                        disk_cache,
                                        "", tx,
                                        &out, |tcx| -> Result<(), _> {
        use rustc::middle::lang_items::*;

        println!("trans.elf => {}", out.out_directory.display());
        tmpdir.into_path();

        self::tls::enter(self, |tcx| {
          let trans_data = trans.trans_crate(tcx.tcx, rx);

          trans.join_trans_and_link(trans_data, self.sess,
                                    dep_graph, &out)
            .map_err(|err| {
              println!("warning: trans failed: `{:?}`", err);
              ()
            })?;

          Ok(())
        })
      })
  }

  fn lookup_crate_num(&self, kernel_id: KernelId) -> Option<CrateNum> {
    let mut out = None;
    let needed_fingerprint = (kernel_id.crate_hash_hi, kernel_id.crate_hash_lo);
    self.cstore.iter_crate_data(|num, data| {
      if out.is_some() { return; }

      if data.name() != kernel_id.crate_name {
        return;
      }
      let finger = data.disambiguator().to_fingerprint().as_value();
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

pub fn create_rustc_session() -> rustc::session::Session {
  use rustc_back::{AddrSpaceIdx, AddrSpaceProps, AddrSpaceKind};
  use rustc_driver::driver::compute_crate_disambiguator;

  use std::str::FromStr;

  let opts = create_rustc_options();
  let registry = rustc_driver::diagnostics_registry();

  let mut sess = rustc::session::build_session(opts, None, registry);
  sess.target.target.options.requires_lto = false;
  sess.target.target.options.codegen_backend = "llvm".into();
  sess.target.target.options.trap_unreachable = true;
  {
    let addr_spaces = &mut sess.target.target.options.addr_spaces;
    addr_spaces.clear();

    let flat = AddrSpaceKind::Flat;
    let flat_idx = AddrSpaceIdx(0);

    let global = AddrSpaceKind::ReadWrite;
    let global_idx = AddrSpaceIdx(1);

    let constant = AddrSpaceKind::ReadOnly;
    let constant_idx = AddrSpaceIdx(2);

    let local = AddrSpaceKind::from_str("local").unwrap();
    let local_idx = AddrSpaceIdx(3);

    let region = AddrSpaceKind::from_str("region").unwrap();
    let region_idx = AddrSpaceIdx(4);

    let private = AddrSpaceKind::Alloca;
    let private_idx = AddrSpaceIdx(5);

    let props = AddrSpaceProps {
      index: flat_idx,
      shared_with: vec![private, region, local,
                        constant, global]
        .into_iter()
        .collect(),
    };
    addr_spaces.insert(flat, props);

    let insert_as = |addr_spaces: &mut BTreeMap<_, _>, kind,
                     idx| {
      let props = AddrSpaceProps {
        index: idx,
        shared_with: vec![flat].into_iter().collect(),
      };
      addr_spaces.insert(kind, props);
    };
    insert_as(addr_spaces, global, global_idx);
    //insert_as(addr_spaces, constant, constant_idx);
    insert_as(addr_spaces, local, local_idx);
    insert_as(addr_spaces, region, region_idx);
    insert_as(addr_spaces, private, private_idx);
  }

  let dis = compute_crate_disambiguator(&sess);
  *sess.crate_disambiguator.borrow_mut() = Some(dis);

  sess
}

pub fn create_rustc_options() -> rustc::session::config::Options {
  use rustc::session::config::*;
  use rustc_back::*;

  let mut opts = basic_options();
  opts.crate_types.push(CrateType::CrateTypeCdylib);
  opts.output_types = output_types();
  opts.optimize = OptLevel::No;
  //opts.optimize = OptLevel::Default;
  opts.cg.retrans_all_deps = true;
  opts.cg.codegen_units = Some(1);
  opts.cg.lto = Lto::No;
  opts.cg.panic = Some(PanicStrategy::Abort);
  //opts.cg.llvm_args.push("-amdgpu-internalize-symbols".into());
  opts.cg.incremental = None;
  opts.cli_forced_codegen_units = Some(1);
  opts.incremental = None;
  opts.cli_forced_thinlto_off = true;
  opts.debugging_opts.no_verify = true;
  opts.debugging_opts.no_landing_pads = true;
  opts.debugging_opts.incremental_queries = false;
  //opts.debugging_opts.print_llvm_passes = true;
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
  use rustc::hir::svh::Svh;
  use rustc::ty::TyCtxt;

  use std::rc::Rc;

  *providers = Providers {
    crate_hash: |tcx, cnum| {
      assert_eq!(cnum, LOCAL_CRATE);
      // XXX?
      Svh::new(1)
    },
    crate_name: |tcx, id| {
      assert_eq!(id, LOCAL_CRATE);
      "jit-methods".into()
    },
    crate_disambiguator: |tcx, cnum| {
      assert_eq!(cnum, LOCAL_CRATE);
      tcx.sess.crate_disambiguator.borrow()
        .as_ref()
        .unwrap()
        .clone()
    },
    native_libraries: |tcx, cnum| {
      assert_eq!(cnum, LOCAL_CRATE);
      Rc::new(vec![])
    },
    link_args: |tcx, cnum| {
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
}
