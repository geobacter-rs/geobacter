
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::sync::{Arc, Weak, RwLock, Mutex};

use indexvec::{Idx, IndexVec};

use hsa_core::kernel_info::KernelId;
use hsa_rt;

use module::{ModuleData, ErasedModule};
use {Accelerator, AcceleratorId};
use metadata::{Metadata, MetadataLoadingError, CrateSource, CrateNameHash};
use platform::os::{get_mapped_files, dylib_search_paths};
use trans::worker::TranslatorData;

type Translators = IndexVec<AcceleratorId, Option<TranslatorData>>;

pub struct ContextData {
  metadata: Arc<Mutex<Vec<Metadata>>>,
  accelerators: IndexVec<AcceleratorId, Option<Weak<Accelerator>>>,
  modules: HashMap<KernelId, Weak<ErasedModule>>,
  translators: Translators,
}

#[derive(Clone)]
pub struct Context(Arc<RwLock<ContextData>>);

impl Context {
  pub fn new() -> Result<Context, Box<Error>> {
    let mapped = get_mapped_files()?;
    let mut unique_metadata = HashSet::new();
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
      println!("adding dylibs in search path {}", search_dir.display());
      for entry in search_dir.read_dir()? {
        let entry = match entry {
          Ok(v) => v,
          Err(_) => { continue; },
        };

        let path = entry.path();
        if !path.is_file() { continue; }
        let extension = path.extension();
        if extension.is_none() || extension.unwrap() != "so" { continue; }

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

    let data = ContextData {
      metadata: Arc::new(Mutex::new(rust_mapped)),
      accelerators: Default::default(),
      modules: Default::default(),
      translators: Default::default(),
    };

    let data = RwLock::new(data);
    let data = Arc::new(data);
    Ok(Context(data))
  }
  pub fn add_accelerator<T>(&self, accel: T)
    -> Result<Arc<T>, Box<Error>>
    where T: Accelerator + 'static,
  {
    let accel = Arc::new(accel);

    let mut lock = self.0.write().unwrap();
    let id = lock.get_next_accelerator_id();
    lock.add_accelerator(id, accel.clone() as Arc<_>);

    Ok(accel)
  }
  pub fn _compile_function(&self, id: KernelId) {
    let lock = self.0.read().unwrap();
    for trans in lock.translators
      .iter()
      .filter_map(|v| v.as_ref() ) {

      trans.translate(id.clone())
        .unwrap();
    }
  }
}

impl ContextData {
  fn get_next_accelerator_id(&self) -> AcceleratorId {
    AcceleratorId::new(self.accelerators.len())
  }
  fn add_accelerator(&mut self, id: AcceleratorId,
                     accel: Arc<Accelerator>) {
    if self.accelerators.len() <= id.index() {
      self.accelerators.resize(id.index() + 1, None);
    }
    self.accelerators[id] = Some(Arc::downgrade(&accel));

    if self.translators.len() <= id.index() {
      self.translators.resize(id.index() + 1,  None);
    }
    let translator = TranslatorData::new(accel, self.metadata.clone());
    self.translators[id] = Some(translator);
  }
}

// force loading librustc_trans so that
