
//! The results of a successful codegen.

use std::any::{Any, };
use std::collections::{BTreeMap, };
use std::fmt::{Debug, };
use std::hash;

use any_key::AnyHash;

use rustc_session::config::{OutputType, };

use super::{PlatformCodegen, CodegenKernelInstance, };

#[derive(Debug)]
pub struct EntryDesc<CD> {
  pub kernel_instance: CodegenKernelInstance,
  /// Rust uses a hash of a bunch of different values in `Session`
  /// in all symbols, which means we'll have different symbol names
  /// for different accelerators.
  pub symbol: String,
  pub platform: CD,
}

#[derive(Debug)]
pub struct CodegenResults<CD> {
  pub outputs: BTreeMap<OutputType, Vec<u8>>,
  pub entries: Vec<EntryDesc<CD>>,
}
pub type PCodegenResults<P> = CodegenResults<<P as PlatformCodegen>::CodegenDesc>;

impl<P> CodegenResults<P>
  where P: PlatformCodegenDesc,
{
  pub fn new() -> Self {
    CodegenResults {
      outputs: Default::default(),
      entries: Vec::new(),
    }
  }
  pub fn take_bitcode(&mut self) -> Option<Vec<u8>> {
    self.outputs.remove(&OutputType::Bitcode)
  }
  pub fn bitcode_ref(&self) -> Option<&[u8]> {
    self.outputs.get(&OutputType::Bitcode)
      .map(|b| &b[..] )
  }
  pub fn take_object(&mut self) -> Option<Vec<u8>> {
    self.outputs.remove(&OutputType::Object)
  }
  pub fn object_ref(&self) -> Option<&[u8]> {
    self.outputs.get(&OutputType::Object)
      .map(|b| &b[..] )
  }
  pub fn assembly_bytes_ref(&self) -> Option<&[u8]> {
    self.outputs.get(&OutputType::Assembly)
      .map(|b| &b[..] )
  }
  pub fn assembly_str_ref(&self) -> Option<&str> {
    self.assembly_bytes_ref()
      .and_then(|b| ::std::str::from_utf8(b).ok() )
  }
  pub fn llvm_assembly_bytes_ref(&self) -> Option<&[u8]> {
    self.outputs.get(&OutputType::LlvmAssembly)
      .map(|b| &b[..] )
  }
  pub fn llvm_assembly_str_ref(&self) -> Option<&str> {
    self.llvm_assembly_bytes_ref()
      .and_then(|b| ::std::str::from_utf8(b).ok() )
  }
  pub fn put_exe(&mut self, data: Vec<u8>) -> Option<Vec<u8>> {
    self.outputs.insert(OutputType::Exe, data)
  }
  pub fn exe_ref(&self) -> Option<&[u8]> {
    self.outputs.get(&OutputType::Exe)
      .map(|b| &b[..] )
  }

  pub fn root(&self) -> &EntryDesc<P> {
    self.entries
      .get(0)
      .expect("internal error: no kernel root?")
  }
}

impl<CD> hash::Hash for CodegenResults<CD>
  where CD: PlatformCodegenDesc + Sized,
{
  fn hash<H>(&self, hasher: &mut H)
    where H: hash::Hasher,
  {
    ::std::hash::Hash::hash(&self.outputs, hasher);
    ::std::hash::Hash::hash(&self.entries, hasher);
  }
}

impl<CD> hash::Hash for EntryDesc<CD>
  where CD: PlatformCodegenDesc + Sized,
{
  fn hash<H>(&self, hasher: &mut H)
    where H: hash::Hasher,
  {
    ::std::hash::Hash::hash(&self.kernel_instance, hasher);
    ::std::hash::Hash::hash(&self.symbol, hasher);
    let platform = &self.platform as &dyn AnyHash;
    AnyHash::hash(platform, hasher);
  }
}

/// Codegen specific data. This is generated in the codegen worker context,
/// with access to the `TyCtxt`.
pub trait PlatformCodegenDesc
  where Self: Debug + Any + Send + Sync,
        Self: AnyHash + 'static,
{ }
