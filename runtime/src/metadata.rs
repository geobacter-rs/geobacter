
use std::collections::{HashMap, VecDeque};
use std::cell::{Cell, RefCell};
use std::error::Error;
use std::iter::{once, };
use std::path::{PathBuf, Path};
use std::slice;
use std::str::Utf8Error;
use std::rc::Rc;
use std::{ptr, fmt};
use std::ops::{Range, Deref};
use std::sync::{Arc, };

use llvm::{self, ObjectFile, };
use rustc_data_structures::owning_ref::{OwningRef, ErasedBoxRef};
use rustc_data_structures::fx::FxHashSet;
use rustc::hir::def_id::{CrateNum,};
use rustc::{self, session};
use rustc_metadata::cstore::{self, MetadataBlob};
use rustc_metadata::schema::{self, METADATA_HEADER};
use rustc_back;
use syntax_pos::symbol::{Symbol};
use flate2::read::DeflateDecoder;

use util::{path2cstr, };
use platform::os::locate_dylib;

#[derive(Debug)]
pub enum MetadataLoadingError {
  Generic(Box<Error>),
  Read,
  ObjectFile,
  SectionMissing,
  SymbolUtf(Utf8Error),
}
impl Error for MetadataLoadingError {
  fn description(&self) -> &str {
    match self {
      &MetadataLoadingError::Generic(ref e) => e.description(),
      &MetadataLoadingError::Read => "error reading file into memory",
      &MetadataLoadingError::ObjectFile => "object file format error",
      &MetadataLoadingError::SectionMissing => "metadata section missing from file",
      &MetadataLoadingError::SymbolUtf(ref e) => e.description(),
    }
  }
}
impl fmt::Display for MetadataLoadingError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.pad(self.description())
  }
}
impl From<Box<Error>> for MetadataLoadingError {
  fn from(v: Box<Error>) -> Self {
    MetadataLoadingError::Generic(v)
  }
}
impl From<Utf8Error> for MetadataLoadingError {
  fn from(v: Utf8Error) -> Self {
    MetadataLoadingError::SymbolUtf(v)
  }
}
#[derive(Hash, Clone, PartialEq, Eq, Debug)]
pub struct CrateNameHash {
  pub name: Symbol,
  pub hash: u64,
}

pub struct CrateMetadataLoader {
  next_crate_num: CrateNum,

  name_to_blob: HashMap<CrateNameHash, (CrateSource, String,
                                        SharedMetadataBlob)>,

  crate_nums: HashMap<CrateNameHash, CrateNum>,
  roots: HashMap<CrateNameHash, Option<schema::CrateRoot>>,

}
impl CrateMetadataLoader {
  pub fn take_crate_num(&mut self) -> CrateNum {
    let id = self.next_crate_num;
    self.next_crate_num = CrateNum::from_u32(id.as_u32() + 1);
    id
  }

  fn process_crate_metadata(&mut self, src: &PathBuf,
                            symbol_name: &str,
                            krate: &MetadataBlob)
    -> (CrateNum, CrateNameHash, bool)
  {
    use std::collections::hash_map::Entry;
    //println!("processing: {}, symbol: {}",
    //         Path::new(src.file_name().unwrap()).display(),
    //         symbol_name);

    let root = krate.get_root();
    let name = CrateNameHash {
      name: root.name,
      hash: root.hash.as_u64(),
    };
    let cnum = match self.crate_nums.entry(name.clone()) {
      Entry::Occupied(o) => {
        return (o.get().clone(), name, false);
      },
      Entry::Vacant(mut v) => {
        //let cnum = self.take_crate_num();
        let cnum = {
          let id = self.next_crate_num;
          self.next_crate_num = CrateNum::from_u32(id.as_u32() + 1);
          id
        };
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
               sess: &session::Session)
    -> Result<CrateMetadata, String>
  {
    let _local_crate = self.take_crate_num();
    let mut out = CrateMetadata::default();

    self.name_to_blob.clear();
    'outer: for object in allmd.iter() {
      //println!("scanning object {:?}", object.src);
      // First we need to find the owning crate for this object and
      // see if it is a rustc plugin. If so, we must skip it!
      let root = object.owner_blob().get_root();
      if root.plugin_registrar_fn.is_some() ||
        root.macro_derive_registrar.is_some() {
        //println!("looks like a rustc plugin, skipping");
        continue 'outer;
      }

      for &(ref symbol_name, ref dep_blob) in object.all.iter() {
        println!("parsing metadata from {}", symbol_name);
        let root = dep_blob.get_root();
        let name = CrateNameHash {
          name: root.name,
          hash: root.hash.as_u64(),
        };

        let value = (object.src.clone(),
                     symbol_name.clone(),
                     dep_blob.clone());
        let prev = self.name_to_blob.insert(name, value);
        assert!(prev.is_none(), "duplicate! first from {}, second from {}",
                prev.unwrap().0.display(), object.src.display());
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
      self.build_impl(sess, name, &mut out)?;
    }

    println!("finished loading metadata");

    Ok(out)
  }

  fn build_impl(&mut self, sess: &session::Session,
                what: CrateNameHash, into: &mut CrateMetadata)
    -> Result<CrateNum, String>
  {
    use rustc::session::search_paths::PathKind;
    use rustc::middle::cstore::DepKind;

    let (src, symbol_name, shared_krate) = {
      let &(ref src, ref symbol_name, ref krate) =
        self.name_to_blob.get(&what)
          .ok_or_else(|| {
            format!("failed to find metadata for {}-{:x}",
                    what.name, what.hash)
          })?;
      (src.clone(), symbol_name.clone(), krate.clone())
    };

    let (cnum, name, is_new) = self
      .process_crate_metadata(&src, symbol_name.as_str(), &*shared_krate);
    if !is_new { return Ok(cnum); }

    let root = shared_krate.get_root();

    println!("loading: {}, name: {}, cnum: {}",
             Path::new(src.file_name().unwrap()).display(),
             root.name, cnum);

    // Some notes: a specific dep can be found inside an arbitrary dylib.
    // On top of that, we won't get any linkage to a dep crate if all of
    // a dependee crate's symbols are inlined into the dependent crate.
    // This means we have to load all possible dylibs in our search paths
    // and look inside everyone.
    let cnum_map = {
      let mut map = vec![cnum];

      for dep in root.crate_deps.decode(&*shared_krate) {
        if dep.kind.macros_only() { continue; }

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
            //println!("crate num not found for `{:?}`, finding manually", dep);
          },
        }

        let cnum = self.build_impl(sess, name, into)?;
        map.push(cnum);
      }

      map.into_iter().collect()
    };

    let def_path_table = root.def_path_table
      .decode((&*shared_krate, sess));
    let exported_symbols = root.exported_symbols
      .decode((&*shared_krate, sess))
      .collect();
    let trait_impls = root.impls
      .decode((&*shared_krate, sess))
      .map(|impls| (impls.trait_id, impls.impls) )
      .collect();

    let cmeta = cstore::CrateMetadata {
      name: root.name,
      extern_crate: Cell::new(None),
      def_path_table: Rc::new(def_path_table),
      exported_symbols,
      trait_impls,
      proc_macros: None,
      root,
      blob: shared_krate.unwrap(),
      cnum_map: RefCell::new(cnum_map),
      cnum,
      codemap_import_info: RefCell::new(vec![]),
      attribute_cache: RefCell::new([vec![], vec![]]),
      dep_kind: Cell::new(DepKind::Explicit),
      source: cstore::CrateSource {
        // Not sure PathKind::Crate is correct.
        dylib: Some((src.to_path_buf(), PathKind::Crate)),
        rlib: None,
        rmeta: None,
      },
      dllimport_foreign_items: FxHashSet(),
    };
    let cmeta = Rc::new(cmeta);
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
    use rustc::hir::def_id::LOCAL_CRATE;
    CrateMetadataLoader {
      next_crate_num: LOCAL_CRATE,
      name_to_blob: Default::default(),
      crate_nums: Default::default(),
      roots: Default::default(),
    }
  }
}

#[derive(Default)]
pub struct CrateMetadata(pub Vec<Rc<cstore::CrateMetadata>>);

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
      .erase_owner();
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

    let cpath = path2cstr(src.as_path());
    let mb = unsafe {
      llvm::LLVMRustCreateMemoryBufferWithContentsOfFile(cpath.as_ptr())
    };
    if mb as isize == 0 {
      return Err(MetadataLoadingError::Read);
    }

    let obj = ObjectFile::new(mb)
      .map(|o| {
        OwningRef::new(Box::new(o))
      })
      .ok_or_else(|| MetadataLoadingError::ObjectFile )?;

    let obj = obj
      .try_map(|o| {
        metadata_section_search(o)
      })?;

    let mut all = vec![];
    let mut owner_index = None;
    {
      let mut symbol_iter = obj.owner().symbol_iter();
      let section = obj.as_ref();
      loop {
        {
          if let Ok(name) = symbol_iter.name().to_str() {
            if name.starts_with("rust_metadata_") {
              let start = symbol_iter.address() as usize;
              let size = symbol_iter.size() as usize;

              if owner_index.is_some() {
                assert!(start != 0);
              } else if start == 0 {
                owner_index = Some(all.len());
              }

              let region = &section[start..start + size];
              let compressed = &region[METADATA_HEADER.len()..];
              let mut inflated = Vec::new();
              let mut deflate = DeflateDecoder::new(compressed.as_ref());
              let region = match deflate.read_to_end(&mut inflated) {
                Ok(_) => {
                  SharedMetadataBlob::new(inflated)
                }
                Err(e) => {
                  let e = format!("failed to decompress metadata in {}: {}: {}",
                                  src.display(),
                                  name, e);
                  return Err(MetadataLoadingError::Generic(e.into()));
                }
              };

              all.push((name.into(), region));
            }
          }
        }
        if !symbol_iter.move_next() {
          break;
        }
      }
    }

    Ok(Metadata {
      src,
      owner_index: owner_index.unwrap(),
      all,
    })
  }

  pub fn owner_symbol(&self) -> &str {
    self.all[self.owner_index].0.as_str()
  }
  pub fn owner_blob(&self) -> &SharedMetadataBlob {
    &self.all[self.owner_index].1
  }
}

fn metadata_section_search<'a>(obj: &'a ObjectFile) -> Result<&'a [u8], MetadataLoadingError> {
  unsafe {
    let si = llvm::mk_section_iter(obj.llof);
    while llvm::LLVMIsSectionIteratorAtEnd(obj.llof, si.llsi) == llvm::False {
      let mut name_buf = ptr::null();
      let name_len = llvm::LLVMRustGetSectionName(si.llsi, &mut name_buf);
      let name = slice::from_raw_parts(name_buf as *const u8,
                                       name_len as usize).to_vec();
      let name = String::from_utf8(name).unwrap();
      if METADATA_SECTION_NAME == name {
        let cbuf = llvm::LLVMGetSectionContents(si.llsi);
        let csz = llvm::LLVMGetSectionSize(si.llsi) as usize;
        // The buffer is valid while the object file is around
        let buf: &'a [u8] = slice::from_raw_parts(cbuf as *const u8, csz);
        return Ok(buf);
      }
      llvm::LLVMMoveToNextSection(si.llsi);
    }
  }
  Err(MetadataLoadingError::SectionMissing)
}

#[cfg(not(target_os = "macos"))]
const METADATA_SECTION_NAME: &'static str = ".rustc";
#[cfg(target_os = "macos")]
const METADATA_SECTION_NAME: &'static str = "__DATA,.rustc";

pub struct DummyMetadataLoader;
impl rustc::middle::cstore::MetadataLoader for DummyMetadataLoader {
  fn get_rlib_metadata(&self,
                       _target: &rustc_back::target::Target,
                       _filename: &Path) -> Result<ErasedBoxRef<[u8]>, String>
  {
    Err("this should never be called".into())
  }
  fn get_dylib_metadata(&self,
                        _target: &rustc_back::target::Target,
                        _filename: &Path) -> Result<ErasedBoxRef<[u8]>, String>
  {
    Err("this should never be called".into())
  }
}
