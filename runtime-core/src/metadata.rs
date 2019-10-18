
use std::error::Error;
use std::io;
use std::path::{PathBuf, Path};
use std::str::Utf8Error;
use std::{fmt};
use std::ops::{Deref};
use std::sync::{Arc, };

use rustc_data_structures::fx::{FxHashMap, };
use rustc_data_structures::sync::{Lock, RwLock, Lrc, MetadataRef, AtomicCell, };
use rustc_data_structures::owning_ref::{OwningRef, };
use rustc::hir::def_id::{CrateNum,};
use rustc::middle::cstore::{MetadataLoader, CrateSource as RustcCrateSource, };
use rustc_metadata::cstore::{self, MetadataBlob, CStore, };
use rustc_metadata::schema::{self, METADATA_HEADER};
use rustc::mir::interpret::AllocDecodingState;
use rustc_target;
use syntax_pos::symbol::{Symbol};
use flate2::read::DeflateDecoder;

use crate::utils::{new_hash_set, };

pub use crate::gintrinsics::CNums;

#[derive(Debug)]
pub enum MetadataLoadingError {
  Generic(Box<dyn Error + Send + Sync + 'static>),
  Io(io::Error),
  SectionMissing,
  SymbolUtf(Utf8Error),
  Deflate(PathBuf, String, io::Error),
  Elf(goblin::error::Error),
}
impl Error for MetadataLoadingError {
  fn description(&self) -> &str {
    match self {
      &MetadataLoadingError::Generic(ref e) => e.description(),
      &MetadataLoadingError::Io(ref e) => e.description(),
      &MetadataLoadingError::SectionMissing => "metadata section missing from file",
      &MetadataLoadingError::SymbolUtf(ref e) => e.description(),
      &MetadataLoadingError::Deflate(_, _, ref e) => e.description(),
      &MetadataLoadingError::Elf(ref e) => e.description(),
    }
  }
}
impl fmt::Display for MetadataLoadingError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.pad(self.description())
  }
}
impl From<Box<dyn Error + Send + Sync + 'static>> for MetadataLoadingError {
  fn from(v: Box<dyn Error + Send + Sync + 'static>) -> Self {
    MetadataLoadingError::Generic(v)
  }
}
impl From<Utf8Error> for MetadataLoadingError {
  fn from(v: Utf8Error) -> Self {
    MetadataLoadingError::SymbolUtf(v)
  }
}
impl From<io::Error> for MetadataLoadingError {
  fn from(v: io::Error) -> Self {
    MetadataLoadingError::Io(v)
  }
}
impl From<goblin::error::Error> for MetadataLoadingError {
  fn from(v: goblin::error::Error) -> Self {
    MetadataLoadingError::Elf(v)
  }
}
#[derive(Hash, Clone, PartialEq, Eq, Debug)]
pub struct CrateNameHash {
  pub name: Symbol,
  pub hash: u64,
}

pub struct CrateMetadataLoader {
  name_to_blob: FxHashMap<CrateNameHash, (CrateSource, String, SharedMetadataBlob)>,

  crate_nums: FxHashMap<CrateNameHash, CrateNum>,
  roots: FxHashMap<CrateNameHash, Option<schema::CrateRoot<'static>>>,
}
impl CrateMetadataLoader {
  fn process_crate_metadata(&mut self,
                            src: &PathBuf,
                            symbol_name: &str,
                            krate: &MetadataBlob,
                            cstore: &CStore)
    -> (CrateNum, CrateNameHash, bool)
  {
    use std::collections::hash_map::Entry;

    let root = krate.get_root();
    let name = CrateNameHash {
      name: root.name,
      hash: root.hash.as_u64(),
    };
    let cnum = match self.crate_nums.entry(name.clone()) {
      Entry::Occupied(o) => {
        return (o.get().clone(), name, false);
      },
      Entry::Vacant(v) => {
        let cnum = cstore.alloc_new_crate_num();
        info!("crate_num: {}, source: {}, symbol_name: {}",
              cnum,
              Path::new(src.file_name().unwrap()).display(),
              symbol_name);
        v.insert(cnum);
        cnum
      },
    };

    let prev = self.roots
      .insert(name.clone(), Some(root));
    assert!(prev.is_none());

    (cnum, name, true)
  }

  pub fn build(&mut self, allmd: &[Metadata],
               cstore: &CStore)
    -> Result<CrateMetadata, String>
  {
    use std::collections::hash_map::Entry;
    let mut out = CrateMetadata::default();

    self.name_to_blob.clear();
    'outer: for object in allmd.iter() {
      info!("scanning object {:?}", object.src);
      // First we need to find the owning crate for this object and
      // see if it is a rustc plugin. If so, we must skip it!
      let root = object.owner_blob().get_root();
      if root.plugin_registrar_fn.is_some() ||
        root.proc_macro_decls_static.is_some() {
        info!("looks like a rustc plugin, skipping");
        continue 'outer;
      }

      for (symbol_name, dep_blob) in object.all.iter() {
        info!("parsing metadata from {}", symbol_name);
        let root = dep_blob.get_root();
        let name = CrateNameHash {
          name: root.name,
          hash: root.hash.as_u64(),
        };

        let value = (object.src.clone(),
                     symbol_name.clone(),
                     dep_blob.clone());
        match self.name_to_blob.entry(name) {
          Entry::Occupied(_) => { },
          Entry::Vacant(v) => {
            v.insert(value);
          },
        }
      }
    }

    let required_names: Vec<_> = self.name_to_blob
      .iter()
      .filter_map(|(name, &(ref src, _, _))| {
        match src {
          &CrateSource::SearchPaths(_) => None,
          _ => Some(name.clone()),
        }
      })
      .collect();

    for name in required_names.into_iter() {
      self.build_impl(name, &mut out,
                      cstore)?;
    }

    info!("finished loading metadata");

    Ok(out)
  }

  fn build_impl(&mut self,
                what: CrateNameHash,
                into: &mut CrateMetadata,
                cstore: &CStore)
    -> Result<CrateNum, String>
  {
    use rustc::dep_graph::DepNodeIndex;
    use rustc::session::search_paths::PathKind;
    use rustc::middle::cstore::DepKind;

    let (src, symbol_name, shared_krate) = {
      let &(ref src, ref symbol_name, ref krate) =
        self.name_to_blob
          .get(&what)
          .ok_or_else(|| {
            format!("failed to find metadata for {}-{:x}",
                    what.name, what.hash)
          })?;
      (src.clone(), symbol_name.clone(), krate.clone())
    };

    let (cnum, _, is_new) = self
      .process_crate_metadata(&src,
                              symbol_name.as_str(),
                              &*shared_krate,
                              cstore);
    if !is_new { return Ok(cnum); }

    let root = shared_krate.get_root();

    info!("loading from: {}, name: {}, cnum: {}",
           Path::new(src.file_name().unwrap()).display(),
           root.name, cnum);

    // Some notes: a specific dep can be found inside an arbitrary dylib.
    // On top of that, we won't get any linkage to a dep crate if all of
    // a dependee crate's symbols are inlined into the dependent crate.
    // This means we have to load all possible dylibs in our search paths
    // and look inside everyone.
    let cnum_map: cstore::CrateNumMap = {
      let mut map: cstore::CrateNumMap = Default::default();
      map.push(cnum);

      for dep in root.crate_deps.decode(&*shared_krate) {
        if dep.kind.macros_only() {
          map.push(cnum);
          continue;
        }

        let name = CrateNameHash {
          name: dep.name,
          hash: dep.hash.as_u64(),
        };
        match self.crate_nums.get(&name) {
          Some(&v) => {
            map.push(v);
            continue;
          },
          None => {
            info!("crate num not found for `{:?}`, finding manually", dep.name);
          },
        }

        let cnum = self.build_impl(name,
                                   into, cstore)?;
        map.push(cnum);
      }

      map.into_iter().collect()
    };

    let dependencies: Vec<CrateNum> = cnum_map.iter().cloned().collect();

    let def_path_table = root
      .def_path_table
      .decode(&*shared_krate);

    let interpret_alloc_index: Vec<u32> = root
      .interpret_alloc_index
      .decode(&*shared_krate)
      .collect();

    let trait_impls = root
      .impls
      .decode(&*shared_krate)
      .map(|impls| (impls.trait_id, impls.impls) )
      .collect();

    let cmeta = cstore::CrateMetadata {
      extern_crate: Lock::new(None),
      def_path_table: Lrc::new(def_path_table),
      trait_impls,
      raw_proc_macros: None,
      root,
      blob: shared_krate.clone().unwrap(), // XXX cloned
      cnum_map,
      cnum,
      dependencies: Lock::new(dependencies),
      source_map_import_info: RwLock::new(vec![]),
      alloc_decoding_state: AllocDecodingState::new(interpret_alloc_index),
      dep_kind: Lock::new(DepKind::Explicit),
      source: RustcCrateSource {
        // Not sure PathKind::Crate is correct.
        dylib: Some((src.to_path_buf(), PathKind::Crate)),
        rlib: None,
        rmeta: None,
      },
      private_dep: false,
      dep_node_index: AtomicCell::new(DepNodeIndex::INVALID),
    };
    let cmeta = Lrc::new(cmeta);
    into.0.push(cmeta);

    Ok(cnum)
  }

  pub fn lookup_cnum(&self, name: &CrateNameHash) -> Option<CrateNum> {
    self.crate_nums.get(name)
      .map(|&v| v )
  }
}
impl Default for CrateMetadataLoader {
  fn default() -> Self {
    CrateMetadataLoader {
      name_to_blob: Default::default(),
      crate_nums: Default::default(),
      roots: Default::default(),
    }
  }
}

#[derive(Default)]
pub struct CrateMetadata(pub Vec<Lrc<cstore::CrateMetadata>>);

pub struct SharedMetadataBlob(Arc<Vec<u8>>, MetadataBlob);
impl SharedMetadataBlob {
  pub fn new(data: Vec<u8>) -> SharedMetadataBlob {
    let data = Arc::new(data);
    let inner = SharedMetadataBlob::create_metadata_blob(&data);
    SharedMetadataBlob(data, inner)
  }

  fn create_metadata_blob(data: &Arc<Vec<u8>>) -> MetadataBlob {
    let inner = OwningRef::new(data.clone());
    let inner = inner.map(|v| &v[..] )
      .map_owner_box()
      .erase_send_sync_owner();
    MetadataBlob(inner)
  }

  pub fn unwrap(self) -> MetadataBlob {
    let SharedMetadataBlob(_, inner) = self;
    inner
  }
}
impl Deref for SharedMetadataBlob {
  type Target = MetadataBlob;
  fn deref(&self) -> &MetadataBlob {
    &self.1
  }
}
impl Clone for SharedMetadataBlob {
  fn clone(&self) -> Self {
    let blob = SharedMetadataBlob::create_metadata_blob(&self.0);
    SharedMetadataBlob(self.0.clone(), blob)
  }
}
unsafe impl Send for SharedMetadataBlob { }


#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum CrateSource {
  Mapped(PathBuf),
  SearchPaths(PathBuf),
}
impl Deref for CrateSource {
  type Target = PathBuf;
  fn deref(&self) -> &PathBuf {
    match self {
      &CrateSource::Mapped(ref p) |
      &CrateSource::SearchPaths(ref p) => p,
    }
  }
}

pub struct Metadata {
  pub src: CrateSource,
  owner_index: usize,
  pub all: Vec<(String, SharedMetadataBlob)>,
}
impl Metadata {
  pub fn new(src: CrateSource) -> Result<Metadata, MetadataLoadingError> {
    use std::fs::{File};
    use std::io::{Read};

    use goblin::Object;
    use memmap::*;

    use crate::rustc_data_structures::rayon::prelude::*;

    let src_file = File::open(src.as_path())?;
    let src_buffer = unsafe {
      MmapOptions::new().map(&src_file)?
    };

    let object = match Object::parse(&src_buffer)? {
      Object::Elf(elf) => elf,

      // TODO?
      _ => panic!("can only load from elf files"),
    };

    let mut metadata_section = None;
    for section_header in object.section_headers.iter() {
      if section_header.sh_type == 0 { continue; }

      let name = match object.shdr_strtab.get(section_header.sh_name) {
        Some(Ok(name)) => name,
        _ => continue,
      };

      if name != METADATA_SECTION_NAME { continue; }

      metadata_section = Some(section_header.clone());
    }
    if metadata_section.is_none() {
      return Err(MetadataLoadingError::SectionMissing);
    }
    let metadata_section = metadata_section.unwrap();
    let metadata_section = &src_buffer[metadata_section.file_range()];

    let mut owner_index = None;
    let syms: Vec<_> = object.syms.iter()
      .filter_map(|sym| {
        let name = match object.strtab.get(sym.st_name) {
          Some(Ok(name)) => name,
          _ => return None,
        };

        if !name.starts_with("rust_metadata_") { return None; }

        Some((sym, name.to_string()))
      })
      .collect();

    for (idx, &(ref sym, _)) in syms.iter().enumerate() {
      let start = sym.st_value;
      if owner_index.is_some() {
        assert!(start != 0);
      } else if start == 0 {
        owner_index = Some(idx);
      }
    }

    let all_res: Vec<Result<_, MetadataLoadingError>> = syms.into_par_iter()
      .map(|(sym, name)| {
        let start = sym.st_value as usize;
        let end   = (sym.st_value + sym.st_size) as usize;

        let region = &metadata_section[start..end];
        let compressed = &region[METADATA_HEADER.len()..];

        let mut inflated = Vec::new();
        let mut deflate = DeflateDecoder::new(compressed.as_ref());
        deflate.read_to_end(&mut inflated)
          .map_err(|e| {
            MetadataLoadingError::Deflate(src.as_path().into(), name.to_string(),
                                          e)
          })?;

        Ok((name.to_string(), SharedMetadataBlob::new(inflated)))
      })
      .collect();

    let mut all = Vec::with_capacity(all_res.len());
    for all_res in all_res.into_iter() {
      all.push(all_res?);
    }

    Ok(Metadata {
      src,
      owner_index: owner_index.unwrap(),
      all,
    })
  }

  pub fn owner_blob(&self) -> &MetadataBlob {
    &self.all[self.owner_index].1
  }
}

#[cfg(not(target_os = "macos"))]
const METADATA_SECTION_NAME: &'static str = ".rustc";
#[cfg(target_os = "macos")]
const METADATA_SECTION_NAME: &'static str = "__DATA,.rustc";

pub struct DummyMetadataLoader;
impl MetadataLoader for DummyMetadataLoader {
  fn get_rlib_metadata(&self,
                       _target: &rustc_target::spec::Target,
                       _filename: &Path) -> Result<MetadataRef, String>
  {
    Err("this should never be called".into())
  }
  fn get_dylib_metadata(&self,
                        _target: &rustc_target::spec::Target,
                        _filename: &Path) -> Result<MetadataRef, String>
  {
    Err("this should never be called".into())
  }
}
pub struct LoadedCrateMetadata {
  pub globals: syntax::Globals,
  pub mapped: Vec<Metadata>,
}

/// Loads the Rust metadata for all linked crates. This data isn't light;
/// you'll probably want to store it somewhere and reuse it a lot.
pub fn context_metadata() -> Result<LoadedCrateMetadata, Box<dyn Error>> {
  use crate::platform::os::{get_mapped_files, dylib_search_paths};
  use crate::syntax::source_map::edition::Edition;

  use std::env::consts::DLL_EXTENSION;
  use std::ffi::OsStr;
  use std::path::Component;

  let syntax_globals = syntax::Globals::new(Edition::Edition2018);

  let mapped = syntax_globals.with(|| -> Result<_, Box<dyn Error>> {
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
        if extension != Some(DLL_EXTENSION.as_ref()) { continue; }
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

    Ok(rust_mapped)
  })?;

  Ok(LoadedCrateMetadata {
    globals: syntax_globals,
    mapped,
  })
}
