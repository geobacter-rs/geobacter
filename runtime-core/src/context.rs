
use std::error::Error;
use std::ffi::{OsStr, CString, CStr, };
use std::sync::{Arc, RwLock, Weak, atomic::AtomicUsize,
                atomic::Ordering, };
use std::path::{Component};

use rustc_metadata::cstore::CStore;
use rustc::middle::cstore::MetadataLoader;
use syntax;

use indexvec::{Idx, IndexVec};

use {Accelerator, AcceleratorId, AcceleratorTargetDesc, };
use metadata::{Metadata, MetadataLoadingError, CrateSource, CrateNameHash,
               CrateMetadata, CrateMetadataLoader, DummyMetadataLoader, };
use platform::os::{get_mapped_files, dylib_search_paths};
use codegen::worker::{CodegenComms, CodegenUnsafeSyncComms, CodegenDesc,
                      CodegenShaderInterface, };
use codegen::products::{CodegenResults, };
use accelerators::{RustBuildRoot, DeviceLibsStaging, };
use utils::{HashMap, new_hash_set, };

use vk;

use lcore::*;
use lstd::vk_help::{StaticPipelineLayoutDesc, StaticShaderInterfaceDef, };

pub use rustc::session::config::OutputType;

type Translators = HashMap<Arc<AcceleratorTargetDesc>, CodegenUnsafeSyncComms>;
type CodegenCacheKey = (Arc<AcceleratorTargetDesc>, CodegenDesc);
type CodegenCache = HashMap<CodegenCacheKey, Arc<CodegenResults>>;

const FRAMEWORK_DATA_SUBDIR: &'static str = ".legionella";

pub struct ContextData {
  syntax_globals: syntax::Globals,
  cstore: CStore,

  m: RwLock<ContextDataMut>,
}
/// Data that will be wrapped in a rw mutex.
pub struct ContextDataMut {
  next_accel_id: AcceleratorId,

  /// unfortunate that we have to initialize this after we create the full
  /// instance.
  host_codegen: Option<CodegenUnsafeSyncComms>,

  accelerators: IndexVec<AcceleratorId, Option<Arc<Accelerator>>>,

  translators: Translators,
  codegen_cache: CodegenCache,
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

    let accelerators = IndexVec::new();
    let translators: Translators = Default::default();

    let data = ContextDataMut {
      next_accel_id: AcceleratorId::new(0),
      host_codegen: None,
      accelerators,
      translators,
      codegen_cache: Default::default(),
    };
    let data = ContextData {
      syntax_globals,
      cstore,

      m: RwLock::new(data),
    };
    let data = Arc::new(data);
    let context = Context(data);

    context.init_host_codegen()?;

    Ok(context)
  }

  fn init_host_codegen(&self) -> Result<(), Box<Error>> {
    let host_desc = Arc::default();
    let host_codegen = CodegenComms::new_host(self, host_desc)?;

    let mut w = self.0.m.write()
      .map_err(|_| {
        "poisoned lock!"
      })?;

    w.host_codegen = Some(unsafe {
      host_codegen.sync_comms()
    });

    Ok(())
  }

  pub(crate) fn cstore(&self) -> &CStore {
    &self.0.cstore
  }
  pub(crate) fn syntax_globals(&self) -> &syntax::Globals {
    &self.0.syntax_globals
  }

  pub fn host_codegen(&self) -> Result<CodegenComms, Box<Error>> {
    let r = self.0.m.read()
      .map_err(|_| {
        "poisoned lock!"
      })?;
    Ok(r.host_codegen.as_ref().expect("no host codegen").clone_unsync())
  }

  pub fn add_accel<T>(&self, accel: Arc<T>) -> Result<(), Box<Error>>
    where T: Accelerator,
  {
    let gaccel = accel as Arc<Accelerator>;
    let mut w = self.0.m.write()
      .map_err(|_| {
        "poisoned lock!"
      })?;
    w.create_translator_for_accel(self, &gaccel)
  }

  pub fn downgrade_ref(&self) -> WeakContext {
    WeakContext(Arc::downgrade(&self.0))
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

  pub fn take_accel_id(&self) -> Result<AcceleratorId, Box<Error>> {
    let id = self.0.m.write()
      .map_err(|_| {
        "poisoned lock!"
      })?
      .get_next_accelerator_id();
    Ok(id)
  }

  /// This is not meant to be used directly.
  pub fn manually_compile_desc(&self,
                               accel: &Arc<Accelerator>,
                               desc: CodegenDesc)
    -> Result<Arc<CodegenResults>, Box<Error>>
  {
    use std::fs::File;
    use std::io::{Read, Write, };
    use std::process::Command;

    use dirs::home_dir;

    use tempdir::TempDir;

    // technically, if disregarding the possibility for remote accels,
    // this double caches. However, we cache here anyway so that, in the
    // future, we can add clustering where the master controller (ie the
    // machine which invokes the compute jobs) does the codegening and
    // sends the results to the compute nodes en masse (it's likely that
    // such cases would have a large set of identical accels).

    let target_desc = accel.accel_target_desc();
    let cache_key = (target_desc.clone(), desc);
    {
      let read = self.0.m.read()
        .map_err(|_| {
          "poisoned lock!"
        })?;
      if let Some(entry) = read.codegen_cache.get(&cache_key) {
        return Ok(entry.clone());
      }
    }

    let codegen = accel
      .get_codegen()
      .ok_or_else(|| {
        "this accelerator has no codegen attached"
      })?;

    let host = accel.host_codegen();

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

    let mut codegen = codegen.codegen(desc, host)?;

    let tdir = TempDir::new("link-compilation")?;
    let obj_filename = tdir.path().join("obj.bc");
    {
      let mut out = File::create(&obj_filename)?;

      let obj = codegen.outputs
        .remove(&OutputType::Bitcode)
        .expect("no object output");

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

    let spv_filename = tdir.path().join("kernel.spv");
    let mut cmd = Command::new(rbr.llvm_tool("llvm-spirv"));
    cmd
      .arg("-o").arg(&spv_filename)
      .arg(linked_filename);
    info!("codegen-ing: {:?}", cmd);

    if !cmd.spawn()?.wait()?.success() {
      return Err("codegen failed".into());
    }

    tdir.into_path();

    let mut bin = Vec::new();
    {
      let mut file = File::open(&spv_filename)?;
      file.read_to_end(&mut bin)?;
    }

    codegen.outputs.insert(OutputType::Exe, bin);

    let codegen = Arc::new(codegen);
    let mut write = self.0.m.write()
      .map_err(|_| {
        "poisoned lock!"
      })?;
    // don't check to see if the entry already exists
    // (we could race in this function, if called, uhm, manually),
    // just replace the value.
    // `ModuleData::compile` uses a per-function lock, so this can't
    // race there.
    write.codegen_cache.insert(cache_key, codegen.clone());

    Ok(codegen)
  }
}

impl ContextDataMut {
  fn get_next_accelerator_id(&mut self) -> AcceleratorId {
    let id = self.next_accel_id;
    self.next_accel_id.0 += 1;
    id
  }
  fn create_translator_for_accel(&mut self, context: &Context,
                                 accel: &Arc<Accelerator>)
    -> Result<(), Box<Error>>
  {
    use std::collections::hash_map::Entry;

    let accel_desc = accel.accel_target_desc().clone();
    match self.translators.entry(accel_desc.clone()) {
      Entry::Occupied(o) => {
        let comms: CodegenComms = o.get().clone_unsync();
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
impl Eq for Context { }
impl PartialEq for Context {
  fn eq(&self, rhs: &Self) -> bool {
    Arc::ptr_eq(&self.0, &rhs.0)
  }
}
impl<'a> PartialEq<&'a Context> for Context {
  fn eq(&self, rhs: &&Self) -> bool {
    Arc::ptr_eq(&self.0, &rhs.0)
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

#[derive(Debug)]
pub struct SpirvModule {
  entry: CString,
  exe_model: ExecutionModel,
  /// If Some(..), this function is a shader; if None, it is a kernel.
  shader: Option<CodegenShaderInterface>,
  spirv: vk::pipeline::shader::ShaderModule,
  pipeline_desc: StaticPipelineLayoutDesc,
}
impl SpirvModule {
  pub fn descriptor_set(&self) -> &StaticPipelineLayoutDesc {
    &self.pipeline_desc
  }
  pub fn entry(self: Arc<Self>) -> SpirvEntry {
    match self.exe_model {
      ExecutionModel::Kernel |
      ExecutionModel::GLCompute => {
        SpirvEntry::Kernel(SpirvComputeKernel {
          module: self,
        })
      },
      _ => {
        SpirvEntry::Shader(SpirvGraphicsShader {
          module: self,
        })
      },
    }
  }
  pub fn entry_ref<'a>(self: &'a Arc<Self>) -> SpirvEntryRef<'a> {
    if self.shader.is_some() {
      SpirvEntryRef::Shader(SpirvGraphicsShaderRef {
        module: &*self,
      })
    } else {
      SpirvEntryRef::Kernel(SpirvComputeKernelRef {
        module: &*self,
      })
    }
  }

  pub fn graphics_ty(&self) -> vk::pipeline::shader::GraphicsShaderType {
    use vk::pipeline::shader::GraphicsShaderType;
    match self.exe_model {
      ExecutionModel::Vertex => GraphicsShaderType::Vertex,
      ExecutionModel::TessellationControl => GraphicsShaderType::TessellationControl,
      ExecutionModel::TessellationEval => GraphicsShaderType::TessellationEvaluation,
      ExecutionModel::Geometry => unimplemented!("TODO geometry primitive modes"),
      ExecutionModel::Fragment => GraphicsShaderType::Fragment,
      _ => panic!("not a graphics shader exe model"),
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub struct SpirvComputeKernelRef<'a> {
  module: &'a SpirvModule,
}
#[derive(Clone, Copy, Debug)]
pub struct SpirvGraphicsShaderRef<'a> {
  module: &'a SpirvModule,
}
#[derive(Clone, Copy, Debug)]
pub enum SpirvEntryRef<'a> {
  Kernel(SpirvComputeKernelRef<'a>),
  Shader(SpirvGraphicsShaderRef<'a>),
}
#[derive(Clone, Debug)]
pub struct SpirvComputeKernel {
  module: Arc<SpirvModule>,
}
#[derive(Clone, Debug)]
pub struct SpirvGraphicsShader {
  module: Arc<SpirvModule>,
}
#[derive(Clone, Debug)]
pub enum SpirvEntry {
  Kernel(SpirvComputeKernel),
  Shader(SpirvGraphicsShader),
}

unsafe impl<'a> vk::pipeline::shader::EntryPointAbstract for SpirvComputeKernelRef<'a> {
  type PipelineLayout = StaticPipelineLayoutDesc;
  type SpecializationConstants = ();

  fn module(&self) -> &vk::pipeline::shader::ShaderModule {
    &self.module.spirv
  }
  fn name(&self) -> &CStr {
    &self.module.entry
  }
  fn layout(&self) -> &Self::PipelineLayout { &self.module.pipeline_desc }
}
unsafe impl<'a> vk::pipeline::shader::EntryPointAbstract for SpirvGraphicsShaderRef<'a> {
  type PipelineLayout = StaticPipelineLayoutDesc;
  type SpecializationConstants = ();

  fn module(&self) -> &vk::pipeline::shader::ShaderModule {
    &self.module.spirv
  }
  fn name(&self) -> &CStr {
    &self.module.entry
  }
  fn layout(&self) -> &Self::PipelineLayout { &self.module.pipeline_desc }
}
unsafe impl<'a> vk::pipeline::shader::GraphicsEntryPointAbstract for SpirvGraphicsShaderRef<'a> {
  type InputDefinition = StaticShaderInterfaceDef;
  type OutputDefinition = StaticShaderInterfaceDef;

  fn input(&self) -> &Self::InputDefinition { &self.module.shader.as_ref().unwrap().0 }
  fn output(&self) -> &Self::OutputDefinition { &self.module.shader.as_ref().unwrap().1 }
  fn ty(&self) -> vk::pipeline::shader::GraphicsShaderType {
    self.module.graphics_ty()
  }
}
unsafe impl vk::pipeline::shader::EntryPointAbstract for SpirvComputeKernel {
  type PipelineLayout = StaticPipelineLayoutDesc;
  type SpecializationConstants = ();

  fn module(&self) -> &vk::pipeline::shader::ShaderModule {
    &self.module.spirv
  }
  fn name(&self) -> &CStr {
    &self.module.entry
  }
  fn layout(&self) -> &Self::PipelineLayout { &self.module.pipeline_desc }
}
unsafe impl vk::pipeline::shader::EntryPointAbstract for SpirvGraphicsShader {
  type PipelineLayout = StaticPipelineLayoutDesc;
  type SpecializationConstants = ();

  fn module(&self) -> &vk::pipeline::shader::ShaderModule {
    &self.module.spirv
  }
  fn name(&self) -> &CStr {
    &self.module.entry
  }
  fn layout(&self) -> &Self::PipelineLayout { &self.module.pipeline_desc }
}
unsafe impl vk::pipeline::shader::GraphicsEntryPointAbstract for SpirvGraphicsShader {
  type InputDefinition = StaticShaderInterfaceDef;
  type OutputDefinition = StaticShaderInterfaceDef;

  fn input(&self) -> &Self::InputDefinition { &self.module.shader.as_ref().unwrap().0 }
  fn output(&self) -> &Self::OutputDefinition { &self.module.shader.as_ref().unwrap().1 }
  fn ty(&self) -> vk::pipeline::shader::GraphicsShaderType {
    self.module.graphics_ty()
  }
}

impl<'a> SpirvEntryRef<'a> {
  pub fn kernel_entry(self) -> Option<SpirvComputeKernelRef<'a>> {
    match self {
      SpirvEntryRef::Kernel(k) => Some(k),
      _ => None,
    }
  }
  pub fn shader_entry(self) -> Option<SpirvGraphicsShaderRef<'a>> {
    match self {
      SpirvEntryRef::Shader(s) => Some(s),
      _ => None,
    }
  }
}
impl SpirvEntry {
  pub fn kernel_entry(self) -> Option<SpirvComputeKernel> {
    match self {
      SpirvEntry::Kernel(k) => Some(k),
      _ => None,
    }
  }
  pub fn shader_entry(self) -> Option<SpirvGraphicsShader> {
    match self {
      SpirvEntry::Shader(s) => Some(s),
      _ => None,
    }
  }

  pub fn as_ref(&self) -> SpirvEntryRef {
    match self {
      &SpirvEntry::Kernel(ref k) =>
        SpirvEntryRef::Kernel(SpirvComputeKernelRef {
          module: &*k.module,
        }),
      &SpirvEntry::Shader(ref s) =>
        SpirvEntryRef::Shader(SpirvGraphicsShaderRef {
          module: &*s.module,
        }),
    }
  }
}

pub(crate) struct ModuleData(WeakContext, RwLock<ModuleData_>);
impl ModuleData {
  pub(crate) fn compile(&self,
                        context: &Context,
                        accel: &Arc<Accelerator>,
                        desc: CodegenDesc)
    -> Result<Arc<SpirvModule>, Box<Error>>
  {
    {
      let read = self.1.read()
        .map_err(|_| {
          "poisoned lock!"
        })?;
      let entry = read.exes.get(accel.id());
      if let Some(kernel) = entry.and_then(|v| v.as_ref() ) {
        return Ok(kernel.clone());
      }
    }

    // XXX this is not ideal: compiling the same kernel for different
    // accels will be serialized.
    let mut write = self.1.write()
      .map_err(|_| {
        "poisoned lock!"
      })?;
    // check to see if our work was done by someone else:
    {
      let entry = write.exes.get(accel.id());
      if let Some(kernel) = entry.and_then(|v| v.as_ref() ) {
        return Ok(kernel.clone());
      }
    }

    let codegen = context.manually_compile_desc(accel, desc)?;
    let shader = codegen.desc.interface;
    let pipeline_desc = codegen.desc.pipeline;
    let exe_bin = codegen.outputs.get(&OutputType::Exe).unwrap();

    let dev = accel.device();

    let spirv = unsafe {
      vk::pipeline::shader::ShaderModule::new(dev.clone(),
                                              exe_bin)?
    };
    // vulkano puts the ShaderModule in an Arc. Extract it here:
    let spirv = Arc::try_unwrap(spirv)
      .ok()
      .expect("spirv module is already shared? Impossible!");

    println!("symbol: {}", codegen.symbol);

    let kernel = SpirvModule {
      entry: CString::new(codegen.symbol.clone())
        .expect("str -> CString"),
      exe_model: desc.exe_model,
      shader,
      spirv,
      pipeline_desc,
    };
    let kernel = Arc::new(kernel);
    if write.exes.len() <= accel.id().index() {
      write.exes.resize(accel.id().index() + 1, None);
    }
    write.exes[accel.id()] = Some(kernel.clone());
    Ok(kernel)
  }
}
#[derive(Default)]
pub(crate) struct ModuleData_ {
  exes: IndexVec<AcceleratorId, Option<Arc<SpirvModule>>>,
}
#[derive(Clone, Copy, Debug)]
/// No PhantomData on this, this object doesn't own the arguments or return
/// values of the function it represents.
pub(crate) struct ModuleContextData(&'static AtomicUsize);

impl ModuleContextData {
  pub fn upgrade(&self, context: &Context) -> Option<Arc<ModuleData>> {
    let ptr_usize = self.0.load(Ordering::Acquire);
    if ptr_usize == 0 { return None; }
    let ptr = ptr_usize as *const ModuleData;

    let arc = unsafe { Arc::from_raw(ptr) };
    let arc_clone = arc.clone();
    // don't downref the global arc:
    Arc::into_raw(arc);

    let arc = arc_clone;
    let expected_context = arc.0.upgrade();
    if expected_context.is_none() { return None; }
    let expected_context = expected_context.unwrap();
    assert!(expected_context == context,
            "there are two context's live at the same time");

    Some(arc)
  }

  pub fn get_cache_data(&self, context: &Context)
    -> Arc<ModuleData>
  {
    use std::intrinsics::unlikely;

    let mut cached = self.upgrade(context);
    if unsafe { unlikely(cached.is_none()) } {
      let data = RwLock::new(Default::default());
      let data = ModuleData(context.downgrade_ref(), data);
      let data = Arc::new(data);
      let data_ptr = Arc::into_raw(data.clone());
      let data_usize = data_ptr as usize;

      // conservative orderings b/c this isn't the fast path.
      let actual_data_usize = self.0
        .compare_exchange(0, data_usize,
                          Ordering::SeqCst,
                          Ordering::SeqCst);
      match actual_data_usize {
        Ok(0) => {
          cached = Some(data);
        },
        Ok(_) => unreachable!(),
        Err(actual_data_usize) => {
          // either someone beat us, or the data is from an old context.
          let cached2 = self.upgrade(context);
          if cached2.is_some() {
            // someone beat us.
            unsafe { Arc::from_raw(data_ptr) };
            cached = cached2;
          } else {
            // the data is old. we need to clean up, while allowing for
            // possible races in this process.

            let r = self.0
              .compare_exchange(actual_data_usize,
                                data_usize,
                                Ordering::SeqCst,
                                Ordering::SeqCst);
            match r {
              Ok(actual_data_usize) => {
                // do the cleanup:
                let actual_data = actual_data_usize as *const ModuleData;
                let _actual_data = unsafe { Arc::from_raw(actual_data) };
                // let actual_data drop.

                cached = Some(data);
              },
              Err(_) => {
                // someone beat us.
                unsafe { Arc::from_raw(data_ptr) };
                let data = self.upgrade(context)
                  .expect("someone beat us in setting context \
                           module data, but didn't set it to \
                           value data");
                cached = Some(data);
              },
            }
          }
        },
      }
    }

    cached.unwrap()
  }

  pub fn get<F, Args, Ret>(f: &F) -> Self
    where F: Fn<Args, Output = Ret>,
  {
    use hsa_core::kernel::kernel_context_data_id;
    let data_ref = kernel_context_data_id(f);
    ModuleContextData(data_ref)
  }
}
impl Eq for ModuleContextData { }
impl PartialEq for ModuleContextData {
  fn eq(&self, rhs: &Self) -> bool {
    self.0 as *const AtomicUsize == rhs.0 as *const AtomicUsize
  }
}
impl<'a> PartialEq<&'a ModuleContextData> for ModuleContextData {
  fn eq(&self, rhs: &&Self) -> bool {
    self.0 as *const AtomicUsize == rhs.0 as *const AtomicUsize
  }
}
