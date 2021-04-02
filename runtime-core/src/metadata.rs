
use std::convert::TryInto;
use std::error::Error;
use std::io;
use std::path::{PathBuf, Path};
use std::str::Utf8Error;
use std::{fmt};
use std::ops::{Deref};
use std::sync::{Arc, };

use goblin::elf::Elf;
use goblin::pe::PE;

use memmap::Mmap;

use rustc_data_structures::fx::{FxHashMap, };
use rustc_data_structures::sync::MetadataRef;
use rustc_data_structures::owning_ref::{OwningRef, };
use rustc_hir::def_id::{CrateNum, };
use rustc_middle::middle::cstore::{MetadataLoader, CrateSource as RustcCrateSource, };
use rustc_metadata::creader::CStore;
use rustc_metadata::rmeta::{METADATA_HEADER, decoder,
                            decoder::MetadataBlob, CrateRoot,
                            decoder::CrateNumMap, };
use rustc_target;
use rustc_span::symbol::{Symbol};

use snap::read::FrameDecoder;

use crate::utils::{new_hash_set, };

#[derive(Debug)]
pub enum MetadataLoadingError {
  Generic(Box<dyn Error + Send + Sync + 'static>),
  Io(io::Error),
  SectionMissing,
  SymbolUtf(Utf8Error),
  Header,
  Deflate(PathBuf, String, io::Error),
  ObjectFormat(goblin::error::Error),
}
impl Error for MetadataLoadingError { }
impl fmt::Display for MetadataLoadingError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      &MetadataLoadingError::Generic(ref e) => fmt::Display::fmt(e, f),
      &MetadataLoadingError::Io(ref e) => fmt::Display::fmt(e, f),
      &MetadataLoadingError::SectionMissing => {
        f.pad("metadata section missing from file")
      },
      &MetadataLoadingError::SymbolUtf(ref e) => fmt::Display::fmt(e, f),
      &MetadataLoadingError::Header => f.pad("corrupt/unsupported metadata header"),
      &MetadataLoadingError::Deflate(_, _, ref e) => fmt::Display::fmt(e, f),
      &MetadataLoadingError::ObjectFormat(ref e) => fmt::Display::fmt(e, f),
    }
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
    MetadataLoadingError::ObjectFormat(v)
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
  roots: FxHashMap<CrateNameHash, Option<CrateRoot<'static>>>,
}
impl CrateMetadataLoader {
  fn process_crate_metadata(&mut self,
                            src: &PathBuf,
                            symbol_name: &str,
                            krate: &MetadataBlob,
                            cstore: &mut CStore)
    -> (CrateNum, CrateNameHash, bool)
  {
    use std::collections::hash_map::Entry;

    let root = krate.get_root();
    let name = CrateNameHash {
      name: root.name(),
      hash: root.hash().as_u64(),
    };
    let cnum = match self.crate_nums.entry(name.clone()) {
      Entry::Occupied(o) => {
        return (o.get().clone(), name, false);
      },
      Entry::Vacant(v) => {
        let cnum = cstore.alloc_new_crate_num();
        debug!("crate_num: {}, source: {}, symbol_name: {}",
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
               cstore: &mut CStore)
    -> Result<CrateMetadata, String>
  {
    use std::collections::hash_map::Entry;
    let mut out = CrateMetadata::default();

    self.name_to_blob.clear();
    'outer: for object in allmd.iter() {
      debug!("scanning object {:?}", object.src);
      // First we need to find the owning crate for this object and
      // see if it is a rustc plugin. If so, we must skip it!
      let root = object.owner_blob().get_root();
      if root.plugin_registrar_fn.is_some() ||
        root.proc_macro_data.is_some() {
        continue 'outer;
      }

      for (symbol_name, dep_blob) in object.all.iter() {
        debug!("parsing metadata from {}", symbol_name);
        let root = dep_blob.get_root();
        let name = CrateNameHash {
          name: root.name(),
          hash: root.hash().as_u64(),
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

    debug!("finished loading metadata");

    Ok(out)
  }

  fn build_impl(&mut self,
                what: CrateNameHash,
                into: &mut CrateMetadata,
                cstore: &mut CStore)
    -> Result<CrateNum, String>
  {
    use rustc_session::search_paths::PathKind;
    use rustc_middle::middle::cstore::CrateDepKind;

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

    debug!("loading from: {}, name: {}, cnum: {}",
           Path::new(src.file_name().unwrap()).display(),
           root.name(), cnum);

    // Some notes: a specific dep can be found inside an arbitrary dylib.
    // On top of that, we won't get any linkage to a dep crate if all of
    // a dependee crate's symbols are inlined into the dependent crate.
    // This means we have to load all possible dylibs in our search paths
    // and look inside everyone.
    let cnum_map: CrateNumMap = {
      let mut map: CrateNumMap = Default::default();
      map.push(cnum);

      for dep in root.decode_crate_deps(&*shared_krate) {
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
            debug!("crate num not found for `{:?}`, finding manually", dep.name);
          },
        }

        let cnum = self.build_impl(name,
                                   into, cstore)?;
        map.push(cnum);
      }

      map
    };

    let blob = shared_krate.clone().unwrap(); // XXX cloned
    let dep_kind = CrateDepKind::Explicit;
    let source = RustcCrateSource {
      // Not sure PathKind::Crate is correct.
      dylib: Some((src.to_path_buf(), PathKind::Crate)),
      rlib: None,
      rmeta: None,
    };

    let cmeta = decoder::CrateMetadata::new_geobacter(blob, root, None,
                                                      cnum, cnum_map, dep_kind,
                                                      source, false, None);
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
pub struct CrateMetadata(pub Vec<decoder::CrateMetadata>);

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
    MetadataBlob::new(inner)
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


#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
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

    use goblin::Object;
    use memmap::*;

    let src_file = File::open(src.as_path())?;
    let src_buffer = unsafe {
      MmapOptions::new().map(&src_file)?
    };

    match Object::parse(&src_buffer)? {
      Object::Elf(elf) => Metadata::new_elf(src, &src_buffer, elf),
      Object::PE(pe) => Metadata::new_pe(src, &src_buffer, pe),

      // TODO?
      _ => panic!("TODO: can't load from this object format: {}", src.as_path().display()),
    }
  }

  fn new_elf(src: CrateSource, src_buffer: &Mmap, object: Elf)
    -> Result<Metadata, MetadataLoadingError>
  {
    use std::io::{Read};

    use crate::rustc_data_structures::rayon::prelude::*;

    let metadata_section = object.section_headers.iter()
      .find(|section_header| {
        if section_header.sh_type == 0 { return false; }

        let name = match object.shdr_strtab.get(section_header.sh_name) {
          Some(Ok(name)) => name,
          _ => { return false; },
        };

        name == METADATA_SECTION_NAME
      });
    if metadata_section.is_none() {
      return Err(MetadataLoadingError::SectionMissing);
    }
    let metadata_section = metadata_section.unwrap();
    let metadata_section = &src_buffer[metadata_section.file_range()];

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

    let owner_index = syms.iter()
      .position(|&(ref sym, _)| sym.st_value == 0 );

    let all: Vec<_> = syms.into_par_iter()
      .map(|(sym, name)| {
        let start = sym.st_value as usize;
        let end = (sym.st_value + sym.st_size) as usize;
        let region = &metadata_section[start..end];

        let md_len = METADATA_HEADER.len();
        if region.len() < md_len + 8 {
          return Err(MetadataLoadingError::Header);
        }
        let header = &region[..md_len];
        if header != METADATA_HEADER {
          return Err(MetadataLoadingError::Header);
        }
        let mut pos = md_len;
        let sym_name_len: usize = {
          let mut le_len = [0u8; 4];
          le_len.copy_from_slice(&region[md_len..md_len+4]);
          pos += 4;
          u32::from_le_bytes(le_len)
            .try_into()
            .map_err(|_| MetadataLoadingError::Header )?
        };
        pos += sym_name_len; // We don't need to read the name for Elf; we get the name from the
        // symbol table.

        let comp_start = pos;
        let comp_len: usize = {
          let encoded_len = &region[comp_start..comp_start + 8];
          let mut le_len = [0u8; 8];
          le_len.copy_from_slice(&encoded_len);
          pos += 8;
          u64::from_le_bytes(le_len)
            .try_into()
            .map_err(|_| MetadataLoadingError::Header)?
        };
        let comp_end = pos + comp_len;
        let compressed_bytes = &region[pos..comp_end];

        let mut inflated = Vec::new();
        FrameDecoder::new(compressed_bytes)
          .read_to_end(&mut inflated)
          .map_err(|e| {
            MetadataLoadingError::Deflate(src.as_path().into(), name.to_string(),
                                          e)
          })?;

        Ok((name.to_string(), SharedMetadataBlob::new(inflated)))
      })
      .collect::<Result<Vec<_>, MetadataLoadingError>>()?;

    Ok(Metadata {
      src,
      owner_index: owner_index.unwrap(),
      all,
    })
  }
  fn new_pe(src: CrateSource, src_buffer: &Mmap, object: PE)
    -> Result<Metadata, MetadataLoadingError>
  {
    use std::io::{Read};

    use crate::rustc_data_structures::rayon::prelude::*;

    let metadata_section = object.sections.iter()
      .find(|section| {
        match section.name() {
          Ok(name) => name == METADATA_SECTION_NAME,
          Err(_) => false,
        }
      });
    if metadata_section.is_none() {
      return Err(MetadataLoadingError::SectionMissing);
    }
    let metadata_section = metadata_section.unwrap();
    let buffer_start = metadata_section.pointer_to_raw_data as usize;
    let buffer_end = buffer_start + metadata_section.size_of_raw_data as usize;
    let region = &src_buffer[buffer_start..buffer_end];

    // The owner is always first.
    let owner_index = 0;

    let mut all_compressed = Vec::new();
    let md_len = METADATA_HEADER.len();
    let check_len = |pos, size| {
      if region.len() <= pos + size {
        Err(MetadataLoadingError::Header)
      } else {
        Ok(())
      }
    };
    let mut pos = 0;
    while (pos + (-(pos as isize) & 0xfff) as usize) < region.len() {
      pos += (-(pos as isize) & 0xf) as usize;
      check_len(pos, md_len)?;
      let header = &region[pos..pos + md_len];
      if header != METADATA_HEADER {
        return Err(MetadataLoadingError::Header);
      }
      pos += md_len;
      check_len(pos, 4)?;
      let sym_name_len: usize = {
        let mut le_len = [0u8; 4];
        le_len.copy_from_slice(&region[pos..pos + 4]);
        pos += 4;
        u32::from_le_bytes(le_len)
          .try_into()
          .map_err(|_| MetadataLoadingError::Header )?
      };
      check_len(pos, sym_name_len)?;
      let sym_name_bytes = &region[pos..pos + sym_name_len];
      pos += sym_name_len;
      let sym_name = std::str::from_utf8(sym_name_bytes)?;

      check_len(pos, 8)?;
      let comp_len: usize = {
        let encoded_len = &region[pos..pos + 8];
        let mut le_len = [0u8; 8];
        le_len.copy_from_slice(&encoded_len);
        pos += 8;
        u64::from_le_bytes(le_len)
          .try_into()
          .map_err(|_| MetadataLoadingError::Header)?
      };
      check_len(pos, comp_len)?;
      let comp_end = pos + comp_len;
      let compressed_bytes = &region[pos..comp_end];
      pos += comp_len;

      all_compressed.push((sym_name, compressed_bytes));
    }

    let all = all_compressed.into_par_iter()
      .map(|(name, compressed_bytes)| {
        let mut inflated = Vec::new();
        FrameDecoder::new(compressed_bytes)
          .read_to_end(&mut inflated)
          .map_err(|e| {
            MetadataLoadingError::Deflate(src.as_path().into(), name.into(), e)
          })?;

        Ok((name.to_string(), SharedMetadataBlob::new(inflated)))
      })
      .collect::<Result<Vec<_>, MetadataLoadingError>>()?;

    Ok(Metadata {
      src,
      owner_index,
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
pub(crate) type LoadedCrateMetadata = Box<[Metadata]>;

/// Loads the Rust metadata for all linked crates. This data isn't light;
/// you'll probably want to store it somewhere and reuse it a lot.
pub(crate) fn context_metadata()
  -> Result<LoadedCrateMetadata, Box<dyn Error + Send + Sync + 'static>>
{
  use crate::platform::os::{dylib_search_paths, self_exe_path};
  use crate::rustc_data_structures::rayon::prelude::*;

  use std::collections::BTreeSet;
  use std::env::consts::DLL_EXTENSION;
  use std::ffi::OsStr;
  use std::path::Component;

  let this = CrateSource::Mapped(self_exe_path()?.canonicalize()?);
  let mut mapped = BTreeSet::new();
  mapped.insert(this);
  let mut unique_metadata = new_hash_set();

  let search_mapped = dylib_search_paths()
    .into_par_iter()
    .flat_map(|search_dir| {
      search_dir.read_dir()
        .map(|read_dir| {
          read_dir
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
              if entry.file_type().ok()?.is_file() {
                entry.path().canonicalize().ok()
              } else {
                None
              }
            })
            .filter(|path| {
              let extension = path.extension();
              if extension != Some(DLL_EXTENSION.as_ref()) { return false; }
              // skip other toolchains
              // XXX revisit this when deployment code is written.
              if path.components().any(|v| v == Component::Normal(OsStr::new(".rustup"))) {
                return false;
              }

              true
            })
            .collect::<Vec<_>>()
        })
        .unwrap_or_default()
    })
    .map(CrateSource::SearchPaths);
  mapped.par_extend(search_mapped);

  let mapped = mapped.into_iter() // XXX using .into_par_iter() causes a deadlock...
    .filter_map(|mapped| {
      match Metadata::new(mapped) {
        Err(MetadataLoadingError::SectionMissing) => { None },
        v => Some(v),
      }
    })
    .collect::<Vec<_>>();

  let mut out_metadata = Vec::with_capacity(mapped.len());
  for metadata in mapped.into_iter() {
    let metadata = metadata?;

    {
      let owner = metadata.owner_blob();
      let owner = owner.get_root();
      let name = CrateNameHash {
        name: owner.name(),
        hash: owner.hash().as_u64(),
      };
      if !unique_metadata.insert(name) { continue; }
    }

    out_metadata.push(metadata);
  }

  Ok(out_metadata.into_boxed_slice())
}
