#![feature(rustc_private, platform_intrinsics,
           rustc_diagnostic_macros)]
#![feature(core_intrinsics, std_internals)]

#[macro_use]
extern crate rustc;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_metadata;
extern crate rustc_mir;
extern crate rustc_codegen_utils;
extern crate rustc_data_structures;
extern crate rustc_target;
extern crate syntax;
extern crate syntax_pos;

extern crate core;

#[macro_use]
extern crate log;
extern crate env_logger;
extern crate num_traits;
extern crate vulkano as vko;

extern crate hsa_core;
extern crate legionella_core as lcore;
extern crate rustc_intrinsics;

// Note: don't try to depend on `legionella_std`.

use std::fmt;
use std::marker::PhantomData;
use std::str::{FromStr, };

use hsa_core::kernel::{KernelId, };

use self::rustc::hir::def_id::{DefId, DefIndex, CrateNum, };
use self::rustc::mir::{self, CustomIntrinsicMirGen, };
use self::rustc::ty::{self, TyCtxt, };
use self::rustc_data_structures::sync::{Lrc, };

pub use rustc_intrinsics::*;
pub use self::intrinsics::*;

pub mod intrinsics;
pub mod collector;
pub mod attrs;
pub mod stubbing;

pub trait DefIdFromKernelId {
  fn get_cstore(&self) -> &rustc_metadata::cstore::CStore;

  fn lookup_crate_num(&self, kernel_id: KernelId) -> Option<CrateNum> {
    let mut out = None;
    let needed_fingerprint =
      (kernel_id.crate_hash_hi,
       kernel_id.crate_hash_lo);
    self.get_cstore().iter_crate_data(|num, data| {
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
  fn convert_kernel_id(&self, id: KernelId) -> Option<DefId> {
    self.lookup_crate_num(id)
      .map(|crate_num| DefId {
        krate: crate_num,
        index: DefIndex::from_raw_u32(id.index as u32),
      } )
  }
}
pub trait GetDefIdFromKernelId {
  fn with_self<'a, 'tcx, F, R>(tcx: TyCtxt<'a, 'tcx, 'tcx>, f: F) -> R
    where F: FnOnce(&dyn DefIdFromKernelId) -> R,
          'tcx: 'a;
}

pub fn insert_all_intrinsics<F, U>(marker: &U, mut into: F)
  where F: FnMut(String, Lrc<dyn CustomIntrinsicMirGen>),
        U: GetDefIdFromKernelId + Send + Sync + 'static,
{
  intrinsics::shader::insert_all_intrinsics(marker, &mut into);
  intrinsics::vk::insert_all_intrinsics(marker, &mut into);
}

pub trait LegionellaCustomIntrinsicMirGen {
  fn mirgen_simple_intrinsic<'a, 'tcx>(&self,
                                       _stubs: &stubbing::Stubber,
                                       kid_did: &dyn DefIdFromKernelId,
                                       tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       instance: ty::Instance<'tcx>,
                                       mir: &mut mir::Mir<'tcx>)
    where 'tcx: 'a;

  fn generic_parameter_count<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>) -> usize;
  /// The types of the input args.
  fn inputs<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>>;
  /// The return type.
  fn output<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>) -> ty::Ty<'tcx>;
}

pub struct LegionellaMirGen<T, U>(T, PhantomData<U>)
  where T: LegionellaCustomIntrinsicMirGen + Send + Sync + 'static,
        U: GetDefIdFromKernelId + Send + Sync + 'static;

impl<T, U> LegionellaMirGen<T, U>
  where T: LegionellaCustomIntrinsicMirGen + fmt::Display + Send + Sync + 'static,
        U: GetDefIdFromKernelId + Send + Sync + 'static,
{
  pub fn new(intrinsic: T, _: &U) -> (String, Lrc<dyn CustomIntrinsicMirGen>) {
    let name = format!("{}", intrinsic);
    let mirgen: Self = LegionellaMirGen(intrinsic, PhantomData);
    let mirgen = Lrc::new(mirgen) as Lrc<_>;
    (name, mirgen)
  }
}

impl<T, U> CustomIntrinsicMirGen for LegionellaMirGen<T, U>
  where T: LegionellaCustomIntrinsicMirGen + Send + Sync + 'static,
        U: GetDefIdFromKernelId + Send + Sync,
{
  fn mirgen_simple_intrinsic<'a, 'tcx>(&self,
                                       tcx: TyCtxt<'a, 'tcx, 'tcx>,
                                       instance: ty::Instance<'tcx>,
                                       mir: &mut mir::Mir<'tcx>)
    where 'tcx: 'a,
  {

    U::with_self(tcx, |s| {
      let stubs = stubbing::Stubber::default(); // TODO move into the drivers
      self.0.mirgen_simple_intrinsic(&stubs, s, tcx,
                                     instance, mir)
    })
  }

  fn generic_parameter_count<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>) -> usize {
    self.0.generic_parameter_count(tcx)
  }
  /// The types of the input args.
  fn inputs<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>) -> &'tcx ty::List<ty::Ty<'tcx>> {
    self.0.inputs(tcx)
  }
  /// The return type.
  fn output<'a, 'tcx>(&self, tcx: TyCtxt<'a, 'tcx, 'tcx>) -> ty::Ty<'tcx> {
    self.0.output(tcx)
  }
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum LegionellaLangItemTypes {
  Uniform,
  UniformArray,
  Buffer,
  BufferArray,
}
impl FromStr for LegionellaLangItemTypes {
  type Err = &'static str;
  fn from_str(v: &str) -> Result<Self, &'static str> {
    match v {
      "Uniform" => Ok(LegionellaLangItemTypes::Uniform),
      "UniformArray" => Ok(LegionellaLangItemTypes::UniformArray),
      "Buffer" => Ok(LegionellaLangItemTypes::Buffer),
      "BufferArray" => Ok(LegionellaLangItemTypes::BufferArray),
      _ => Err("unknown Legionella lang item type"),
    }
  }
}
