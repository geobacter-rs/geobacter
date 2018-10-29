
use std::error::Error;
use std::ffi::{OsStr, };
use std::sync::{Arc, RwLock, Weak, };
use std::path::{Component};

use rustc_metadata::cstore::CStore;
use rustc::middle::cstore::MetadataLoader;
use syntax;

use indexvec::{Idx, IndexVec};

use hsa_core::kernel::KernelId;
use hsa_rt::agent::{DeviceType, };

use {Accelerator, AcceleratorId, AcceleratorTargetDesc, };
use metadata::{Metadata, MetadataLoadingError, CrateSource, CrateNameHash,
               CrateMetadata, CrateMetadataLoader, DummyMetadataLoader, };
use platform::os::{get_mapped_files, dylib_search_paths};
use codegen::worker::{CodegenComms, CodegenUnsafeSyncComms, };
use codegen::products::CodegenResults;
use accelerators::{host::HostAccel, amd::AmdGpuAccel, RustBuildRoot,
                   DeviceLibsStaging, };
use utils::{HashMap, new_hash_set, };

pub use rustc::session::config::OutputType;

type Translators = HashMap<Arc<AcceleratorTargetDesc>, CodegenUnsafeSyncComms>;

const FRAMEWORK_DATA_SUBDIR: &'static str = ".legionella";

pub struct ContextData {
  _hsa_ctx: hsa_rt::ApiContext,

  syntax_globals: syntax::Globals,
  cstore: CStore,

  m: RwLock<ContextDataMut>,
}
/// Data that will be wrapped in a rw mutex.
pub struct ContextDataMut {
  local_accels: Vec<Arc<Accelerator>>,
  accelerators: IndexVec<AcceleratorId, Option<Arc<Accelerator>>>,

  translators: Translators,
  // Only None during initialization.
  host_codegen: Option<CodegenComms>,
}

#[derive(Clone)]
pub struct Context(Arc<ContextData>);

unsafe impl Send for Context { }
unsafe impl Sync for Context { }

impl Context {
  pub fn new() -> Result<Context, Box<Error>> {
    let syntax_globals = syntax::Globals::new();

    let cstore = syntax_globals.with(|| -> Result<_, Box<Error>> {
      let mapped = get_mapped_files()?;
      debug!("mapped files: {:#?}", mapped);
      let mut unique_metadata = new_hash_set();
      let mut rust_mapped = vec![];

      for mapped in mapped.into_iter() {
        let metadata = match Metadata::new(CrateSource::Mapped(mapped.clone())) {
          Ok(md) => md,
          Err(MetadataLoadingError::SectionMissing) => { continue; },
          e => e?,
        };

        {
          let owner = metadata.owner_blob();
          let owner = owner.get_root();
          let name = CrateNameHash {
            name: owner.name,
            hash: owner.hash.as_u64(),
          };
          if !unique_metadata.insert(name) { continue; }
        }

        rust_mapped.push(metadata);
      }

      for search_dir in dylib_search_paths()?.into_iter() {
        info!("adding dylibs in search path {}", search_dir.display());
        for entry in search_dir.read_dir()? {
          let entry = match entry {
            Ok(v) => v,
            Err(_) => { continue; },
          };

          let path = entry.path();
          if !path.is_file() { continue; }
          let extension = path.extension();
          if extension.is_none() || extension.unwrap() != "so" { continue; }
          // skip other toolchains
          // XXX revisit this when deployment code is written.
          if path.components().any(|v| v == Component::Normal(OsStr::new(".rustup"))) {
            continue;
          }

          let metadata = match Metadata::new(CrateSource::SearchPaths(path.clone())) {
            Ok(md) => md,
            Err(MetadataLoadingError::SectionMissing) => { continue; },
            e => e?,
          };

          {
            let owner = metadata.owner_blob();
            let owner = owner.get_root();
            let name = CrateNameHash {
              name: owner.name,
              hash: owner.hash.as_u64(),
            };
            if !unique_metadata.insert(name) { continue; }
          }

          rust_mapped.push(metadata);
        }
      }

      let md_loader = Box::new(DummyMetadataLoader);
      let md_loader = md_loader as Box<dyn MetadataLoader + Sync>;
      let cstore = CStore::new(md_loader);
      {
        let mut loader = CrateMetadataLoader::default();
        let CrateMetadata(meta) = loader.build(rust_mapped, &cstore)
          .unwrap();
        for meta in meta.into_iter() {
          let name = CrateNameHash {
            name: meta.name,
            hash: meta.root.hash.as_u64(),
          };
          let cnum = loader.lookup_cnum(&name)
            .unwrap();
          cstore.set_crate_data(cnum, meta);
        }
      }

      Ok(cstore)
    })?;

    let hsa_ctx = hsa_rt::ApiContext::try_upref()?;
    let agents = hsa_ctx.agents()?;
    // we take the first CPU we find as the "host" accelerator. This is arbitrary.
    // There should always be at least one CPU accelerator.
    let host = agents.iter()
      .find(|agent| {
        match agent.device_type() {
          Ok(DeviceType::Cpu) => true,
          _ => false,
        }
      })
      .expect("Huh? No CPU agents on this system?")
      .clone();
    let agents: Vec<_> = agents.into_iter()
      .enumerate()
      .filter(|&(_, ref agent)| agent != &host )
      .collect();

    let mut accelerators = IndexVec::with_capacity(agents.len() + 1);
    let mut local_accels = Vec::with_capacity(agents.len() + 1);
    let mut translators: Translators = Default::default();

    let host = Arc::new(HostAccel::new(AcceleratorId::new(0), host)?);
    let host_target_desc = host.accel_target_desc()?;
    let host_target_desc = Arc::new(host_target_desc);

    let host = host as Arc<Accelerator>;

    accelerators.push(Some(host.clone()));
    local_accels.push(host.clone());

    let data = ContextDataMut {
      accelerators,
      local_accels,
      translators,
      host_codegen: None,
    };
    let data = ContextData {
      _hsa_ctx: hsa_ctx,
      syntax_globals,
      cstore,

      m: RwLock::new(data),
    };
    let data = Arc::new(data);
    let context = Context(data);

    {
      let mut data = context.0.m.write().unwrap();

      let host_codegen = CodegenComms::new(&context,
                                           host_target_desc.clone(),
                                           &host)?;
      host.set_codegen(host_codegen.clone());
      data.host_codegen = Some(host_codegen.clone());

      data.translators.insert(host_target_desc, unsafe { host_codegen.clone().sync_comms() });

      // add the rest of the accelerators
      for (id, agent) in agents.into_iter() {
        let accel = match agent.device_type() {
          Err(e) => {
            error!("agent {}: error getting device type: {:?}; ignoring",
                   id, e);
            continue;
          },
          Ok(DeviceType::Cpu) => {
            HostAccel::new(data.get_next_accelerator_id(), agent)
              .map(|v| Arc::new(v) as Arc<Accelerator>)
          },
          Ok(DeviceType::Gpu) => {
            let vendor = agent.vendor_name();
            match vendor {
              Ok(ref vendor) if vendor == "AMD" => {
                AmdGpuAccel::new(data.get_next_accelerator_id(), Arc::downgrade(&host),
                                 agent)
                  .map(|v| Arc::new(v) as Arc<Accelerator>)
              },
              Ok(vendor) => {
                warn!("agent {}: unsupported non-AMD GPU vendor: {}; ignoring",
                      id, vendor);
                continue;
              },
              Err(e) => Err(e.into()),
            }
          },
          Ok(DeviceType::Dsp) => {
            warn!("agent {}: unsupported DSP `{}` (file a bug report?); ignoring",
                  id, agent.name().unwrap_or_else(|_| "<name error>".into()));
            continue;
          },
        };

        let accel = match accel {
          Ok(a) => a,
          Err(e) => {
            error!("agent {}: error creating accelerator: {:?}; ignoring",
                   id, e);
            continue;
          },
        };

        data.accelerators.push(Some(accel.clone()));
        data.local_accels.push(accel.clone());

        data.create_translator_for_accel(&context, &accel)?;
      }
    }

    Ok(context)
  }

  pub(crate) fn cstore(&self) -> &CStore {
    &self.0.cstore
  }
  pub(crate) fn syntax_globals(&self) -> &syntax::Globals {
    &self.0.syntax_globals
  }

  pub fn downgrade_ref(&self) -> WeakContext {
    WeakContext(Arc::downgrade(&self.0))
  }

  pub fn primary_host_accel(&self) -> Result<Arc<Accelerator>, Box<Error>> {
    let b = self.0.m.read()
      .map_err(|_| {
        "poisoned lock!"
      })?;
    Ok(b.local_accels[0].clone())
  }

  pub fn filter_accels<F>(&self, f: F) -> Result<Vec<Arc<Accelerator>>, Box<Error>>
    where F: FnMut(&&Arc<Accelerator>) -> bool,
  {
    let b = self.0.m.read()
      .map_err(|_| {
        "poisoned lock!"
      })?;
    let r = b.accelerators.iter()
      .filter_map(|a| a.as_ref() )
      .filter(f)
      .cloned()
      .collect();
    Ok(r)
  }
  pub fn find_accel<F>(&self, f: F) -> Result<Option<Arc<Accelerator>>, Box<Error>>
    where F: FnMut(&&Arc<Accelerator>) -> bool,
  {
    let b = self.0.m.read()
      .map_err(|_| {
        "poisoned lock!"
      })?;
    let r = b.accelerators.iter()
      .filter_map(|a| a.as_ref() )
      .find(f)
      .map(|accel| accel.clone() );

    Ok(r)
  }
  pub fn manually_compile(&self, accel: &Arc<Accelerator>, id: KernelId)
    -> Result<CodegenResults, Box<Error>>
  {
    use std::fs::File;
    use std::io::{Read, Write, };
    use std::process::Command;

    use dirs::home_dir;

    use tempdir::TempDir;

    let codegen = accel
      .get_codegen()
      .ok_or_else(|| {
        "this accelerator has no codegen attached"
      })?;

    let host = accel.host_accel()
      .and_then(|host| host.get_codegen() );

    let target_desc = accel.accel_target_desc()?;

    let objs = if let Some(builder) = accel.device_libs_builder() {
      // create the target specific cache dir:
      let fw_dir = home_dir()
        .ok_or_else(|| {
          "no home dir available; please set $HOME"
        })?
        .join(FRAMEWORK_DATA_SUBDIR)
        .join("device-libs");

      let mut staging = DeviceLibsStaging::new(fw_dir.as_path(),
                                               &target_desc,
                                               builder);
      {
        let mut build = staging.create_build()?;
        build.build()?;
      }
      staging.into_bc_objs()
    } else {
      vec![]
    };

    let mut codegen = codegen.codegen(id, host)?;

    let obj_len;
    let tdir = TempDir::new("link-compilation")?;
    let obj_filename = tdir.path().join("obj.bc");
    {
      let mut out = File::create(&obj_filename)?;

      let obj = codegen.outputs.remove(&OutputType::Object)
        .expect("no object output");
      obj_len = obj.len();

      out.write_all(&obj[..])?;
    }

    let mut linked_filename = tdir.path().join("linked.bc");

    let rbr = RustBuildRoot::default();
    if objs.len() > 0 {
      let mut cmd = Command::new(rbr.llvm_tool("llvm-link"));
      cmd.arg("-only-needed")
        .arg("-o").arg(&linked_filename)
        .arg(obj_filename)
        .args(objs);
      if !cmd.spawn()?.wait()?.success() {
        return Err("linking failed".into());
      }
      info!("linking: {:?}", cmd);
    } else {
      linked_filename = obj_filename;
    }

    let obj_filename = tdir.path().join("codegen.obj");
    let mut cmd = Command::new(rbr.llvm_tool("llc"));
    cmd.arg("-filetype=obj")
      .arg(format!("-mcpu={}", target_desc.target.options.cpu))
      .arg(format!("-mtriple={}", target_desc.target.llvm_target))
      .arg(format!("-mattr={}", target_desc.target.options.features))
      .arg(format!("-relocation-model={}", target_desc.target.options.relocation_model))
      .arg("-O3")
      .arg("-o").arg(&obj_filename)
      .arg(linked_filename);
    info!("codegen-ing: {:?}", cmd);

    if !cmd.spawn()?.wait()?.success() {
      return Err("codegen failed".into());
    }

    let elf_filename = tdir.path().join("elf.so");

    let mut cmd = Command::new(rbr.lld());
    cmd.arg(obj_filename)
      .arg("-E")
      .arg("-e").arg("0")
      .arg("-O2")
      .arg("--shared")
      .arg("-o").arg(&elf_filename);
    info!("linking: {:?}", cmd);

    if !cmd.spawn()?.wait()?.success() {
      return Err("linking failed".into());
    }

    tdir.into_path();

    let mut elf = Vec::with_capacity(obj_len);
    {
      let mut file = File::open(&elf_filename)?;
      file.read_to_end(&mut elf)?;
    }

    {
      let mut file = File::create(elf_filename.file_name().unwrap())?;
      file.write_all(elf.as_ref())?;
    }

    codegen.outputs.insert(OutputType::Exe, elf);

    Ok(codegen)
  }
}

impl ContextDataMut {
  fn get_next_accelerator_id(&self) -> AcceleratorId {
    AcceleratorId::new(self.accelerators.len())
  }
  fn create_translator_for_accel(&mut self, context: &Context,
                                 accel: &Arc<Accelerator>)
    -> Result<(), Box<Error>>
  {
    use std::collections::hash_map::Entry;

    let desc = accel.accel_target_desc()?;
    let accel_desc = Arc::new(desc);
    match self.translators.entry(accel_desc.clone()) {
      Entry::Occupied(o) => {
        let comms: CodegenComms = o.get().clone_into();
        comms.add_accel(accel);
        accel.set_codegen(comms);
      },
      Entry::Vacant(v) => {
        let codegen = CodegenComms::new(context, accel_desc, accel)?;
        v.insert(unsafe { codegen.clone().sync_comms() });
        accel.set_codegen(codegen);
      },
    }

    Ok(())
  }
}

#[derive(Clone)]
pub struct WeakContext(Weak<ContextData>);

unsafe impl Send for WeakContext { }
unsafe impl Sync for WeakContext { }

impl WeakContext {
  pub fn upgrade(&self) -> Option<Context> {
    self.0.upgrade()
      .map(|v| Context(v) )
  }
}