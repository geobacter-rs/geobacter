
/// mirgen for various Vulkan related queries.

use std::fmt;

use crate::vko::descriptor::descriptor::{DescriptorDesc,
                                         DescriptorDescTy,
                                         ShaderStages,
                                         DescriptorImageDesc,
                                         DescriptorImageDescArray,
                                         DescriptorBufferDesc, };
use crate::vko::format::Format;

use crate::rustc::mir::{Statement, StatementKind, };
use crate::rustc::mir::interpret::{ConstValue, Scalar, Pointer,  };
use crate::rustc::mir::{self, mono::MonoItem, Local,
                        CustomIntrinsicMirGen, };
use crate::rustc::ty::{self, TyCtxt, Instance, };
use crate::rustc_index::vec::*;
use crate::rustc_data_structures::fx::{FxHashSet, };
use crate::rustc_data_structures::sync::{Lrc, };
use rustc_span::{DUMMY_SP, symbol::Symbol, };

use crate::common::{DriverData, GeobacterCustomIntrinsicMirGen,
                    GetDriverData, GeobacterMirGen, stubbing,
                    collector::collect_items_rec, };

use crate::gvk_core::*;
use crate::gvk_core::ss::{CompilerDescriptorImageDims, CompilerDescriptorDescTyKind, };
use crate::attrs::{geobacter_root_attrs, geobacter_global_attrs,
                   require_descriptor_set_binding_nums, };

use crate::grustc_help::*;

pub fn insert_all_intrinsics<F, U>(marker: &U, mut into: F)
  where F: FnMut(String, Lrc<dyn CustomIntrinsicMirGen>),
        U: GetDriverData + Send + Sync + 'static,
{
  let (k, v) = GeobacterMirGen::new(ComputePipelineLayoutDesc,
                                    marker);
  into(k, v);
  let (k, v) = GeobacterMirGen::new(ComputePipelineRequiredCapabilities,
                                    marker);
  into(k, v);
  let (k, v) = GeobacterMirGen::new(ComputePipelineRequiredExtensions,
                                    marker);
  into(k, v);
  let (k, v) = GeobacterMirGen::new(ComputeDescriptorSetBinding,
                                    marker);
  into(k, v);
  let (k, v) = GeobacterMirGen::new(GraphicsPipelineLayoutDesc,
                                    marker);
  into(k, v);
  let (k, v) = GeobacterMirGen::new(GraphicsPipelineRequiredCapabilities,
                                    marker);
  into(k, v);
  let (k, v) = GeobacterMirGen::new(GraphicsPipelineRequiredExtensions,
                                    marker);
  into(k, v);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ComputePipelineLayoutDesc;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ComputePipelineRequiredCapabilities;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ComputePipelineRequiredExtensions;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ComputeDescriptorSetBinding;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct GraphicsPipelineLayoutDesc;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct GraphicsPipelineRequiredCapabilities;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct GraphicsPipelineRequiredExtensions;

impl fmt::Display for ComputePipelineLayoutDesc {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_compute_pipeline_layout_desc")
  }
}
impl fmt::Display for ComputePipelineRequiredCapabilities {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_compute_pipeline_required_capabilities")
  }
}
impl fmt::Display for ComputePipelineRequiredExtensions {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_compute_pipeline_required_extensions")
  }
}
impl fmt::Display for ComputeDescriptorSetBinding {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_compute_descriptor_set_binding")
  }
}
impl fmt::Display for GraphicsPipelineLayoutDesc {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_graphics_pipeline_layout_desc")
  }
}
impl fmt::Display for GraphicsPipelineRequiredCapabilities {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_graphics_pipeline_required_capabilities")
  }
}
impl fmt::Display for GraphicsPipelineRequiredExtensions {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "__geobacter_graphics_pipeline_required_extensions")
  }
}

impl GeobacterCustomIntrinsicMirGen for ComputePipelineLayoutDesc {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   stubber: &stubbing::Stubber,
                                   kid_did: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>)
  {
    // Create an empty function:
    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let mut bb = mir::BasicBlockData {
      statements: vec![],
      terminator: Some(mir::Terminator {
        source_info,
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };

    let local = Local::new(1);
    let local_ty = mir.local_decls[local].ty;

    let kernel_instance = extract_fn_instance(tcx, instance, local_ty);

    let did = kernel_instance.def_id();

    // collect all referenced mono items upfront:
    let mono_root = MonoItem::Fn(kernel_instance);
    let mut visited: FxHashSet<_> = Default::default();
    collect_items_rec(tcx, stubber, kid_did,
                      mono_root, &mut visited,
                      &mut None);

    visited.remove(&mono_root);

    let _root_attrs = geobacter_root_attrs(tcx, did,
                                            ExecutionModel::GLCompute,
                                            false);

    let mut desc_set_bindings: Vec<Vec<Option<DescriptorDesc>>> = vec![];

    for mono in visited.into_iter() {
      let instance = match mono {
        MonoItem::Fn(instance) => instance,
        MonoItem::Static(mono_did) => Instance::mono(tcx, mono_did),
        MonoItem::GlobalAsm(..) => {
          bug!("unexpected mono item `{:?}`", mono);
        },
      };

      let attrs = geobacter_global_attrs(tcx,
                                          ExecutionModel::GLCompute,
                                          instance, false);

      if let Some(desc) = attrs.descriptor_set_desc {
        if desc_set_bindings.len() <= desc.set as usize {
          desc_set_bindings.resize(desc.set as usize + 1, vec![]);
        }

        let desc_set = &mut desc_set_bindings[desc.set as usize];
        if desc_set.len() <= desc.binding as usize {
          desc_set.resize(desc.binding as usize + 1, None);
        }

        let desc_set_binding = &mut desc_set[desc.binding as usize];

        *desc_set_binding = Some(DescriptorDesc {
          ty: desc.desc_ty,
          array_count: desc.array_count as u32,
          stages: ShaderStages::compute(),
          readonly: desc.read_only,
        });
      }
    }

    info!("desc set bindings: {:#?}", desc_set_bindings);

    let desc_set_ty = tcx.mk_array(desc_bindings_desc_ty(tcx),
                                   desc_set_bindings.len() as _);
    let mut c_desc_set_bindings = Vec::with_capacity(desc_set_bindings.len());
    for desc_set in desc_set_bindings.iter() {
      let mut c_set_bindings = Vec::with_capacity(desc_set.len());
      let set_bindings_ty = tcx.mk_array(mk_static_slice(tcx, desc_desc_ty(tcx)),
                                         desc_set.len() as _);
      for bindings in desc_set.iter() {
        let bindings = bindings.as_ref().map(|b| b.clone() );
        let v = build_compiler_opt(tcx, bindings,
                                   build_compiler_descriptor_desc);
        c_set_bindings.push(v);
      }

      let (_, alloc, _) = static_tuple_alloc(tcx,
                                             "desc set bindings",
                                             c_set_bindings.into_iter(),
                                             set_bindings_ty);
      let slice = ConstValue::Slice {
        data: alloc,
        start: 0,
        end: desc_set.len(),
      };

      c_desc_set_bindings.push(slice);
    }

    let (_, alloc, _) = static_tuple_alloc(tcx,
                                           "desc sets",
                                           c_desc_set_bindings.into_iter(),
                                           desc_set_ty);
    let slice = ConstValue::Slice {
      data: alloc,
      start: 0,
      end: desc_set_bindings.len(),
    };
    let ret_ty = self.output(tcx);
    let slice = const_value_rvalue(tcx, slice, ret_ty);

    let ret = mir::Place::return_place();
    let stmt_kind = StatementKind::Assign(Box::new((ret, slice)));
    let stmt = Statement {
      source_info,
      kind: stmt_kind,
    };
    bb.statements.push(stmt);
    mir.basic_blocks_mut().push(bb);
  }

  fn generic_parameter_count<'tcx>(&self, _tcx: TyCtxt<'tcx>)
    -> usize
  {
    3
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    let n = 0;
    let p = Symbol::intern(&format!("P{}", n));
    let f = tcx.mk_ty_param(n, p);
    let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                               ty::BrAnon(0)));
    let t = tcx.mk_imm_ref(region, f);
    tcx.intern_type_list(&[t])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    desc_set_bindings_desc_ty(tcx)
  }
}
impl GeobacterCustomIntrinsicMirGen for ComputePipelineRequiredCapabilities {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   _kid_did: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>)
  {
    // Create an empty function:
    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let mut bb = mir::BasicBlockData {
      statements: vec![],
      terminator: Some(mir::Terminator {
        source_info,
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };

    let kernel_ty = instance.substs.type_at(0);
    let kernel = extract_fn_instance(tcx, instance, kernel_ty);

    let did = kernel.def_id();

    let root_attrs = geobacter_root_attrs(tcx, did,
                                           ExecutionModel::GLCompute,
                                           false);

    let caps = root_attrs.capabilities
      .iter()
      .map(|&cap| {
        tcx.mk_u32_cv(cap as u32)
      });
    let caps_len = caps.len();

    let array_ty = tcx.mk_array(tcx.types.u32, caps.len() as _);
    let ret_ty = self.output(tcx);

    let (_, alloc, _) = static_tuple_alloc(tcx, "required caps", caps, array_ty);
    let slice = ConstValue::Slice {
      data: alloc,
      start: 0,
      end: caps_len,
    };
    let caps = const_value_rvalue(tcx, slice, ret_ty);

    let ret = mir::Place::return_place();
    let stmt_kind = StatementKind::Assign(Box::new((ret, caps)));
    let stmt = Statement {
      source_info,
      kind: stmt_kind,
    };
    bb.statements.push(stmt);
    mir.basic_blocks_mut().push(bb);
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    3
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    let n = 0;
    let p = Symbol::intern(&format!("P{}", n));
    let f = tcx.mk_ty_param(n, p);
    let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                               ty::BrAnon(0)));
    let t = tcx.mk_imm_ref(region, f);
    tcx.intern_type_list(&[t])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    mk_static_slice(tcx, tcx.types.u32)
  }
}
impl GeobacterCustomIntrinsicMirGen for ComputePipelineRequiredExtensions {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   _kid_did: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>)
  {
    // Create an empty function:
    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let mut bb = mir::BasicBlockData {
      statements: vec![],
      terminator: Some(mir::Terminator {
        source_info,
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };
    let kernel_ty = instance.substs.type_at(0);
    let kernel = extract_fn_instance(tcx, instance, kernel_ty);

    let did = kernel.def_id();

    let root_attrs = geobacter_root_attrs(tcx, did,
                                           ExecutionModel::GLCompute,
                                           false);

    let exts = root_attrs.required_extensions()
      .into_iter()
      .map(|s| {
        static_str_const_value(tcx, s)
      });
    let exts_len = exts.len();

    let array_ty = tcx.mk_array(tcx.mk_static_str(), exts_len as _);
    let ret_ty = self.output(tcx);

    let (_, alloc, _) = static_tuple_alloc(tcx, "required exts", exts, array_ty);
    let slice = ConstValue::Slice {
      data: alloc,
      start: 0,
      end: exts_len,
    };
    let exts = const_value_rvalue(tcx, slice, ret_ty);

    let ret = mir::Place::return_place();
    let stmt_kind = StatementKind::Assign(Box::new((ret, exts)));
    let stmt = Statement {
      source_info,
      kind: stmt_kind,
    };
    bb.statements.push(stmt);
    mir.basic_blocks_mut().push(bb);
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    3
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    let n = 0;
    let p = Symbol::intern(&format!("P{}", n));
    let f = tcx.mk_ty_param(n, p);
    let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                               ty::BrAnon(0)));
    let t = tcx.mk_imm_ref(region, f);
    tcx.intern_type_list(&[t])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    mk_static_slice(tcx, tcx.mk_static_str())
  }
}
impl GeobacterCustomIntrinsicMirGen for ComputeDescriptorSetBinding {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   _kid_did: &dyn DriverData,
                                   tcx: TyCtxt<'tcx>,
                                   instance: ty::Instance<'tcx>,
                                   mir: &mut mir::BodyAndCache<'tcx>)
  {
    // Create an empty function:
    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let mut bb = mir::BasicBlockData {
      statements: vec![],
      terminator: Some(mir::Terminator {
        source_info,
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };

    let kernel_ty = instance.substs.type_at(0);
    let kernel = extract_fn_instance(tcx, instance, kernel_ty);

    let did = kernel.def_id();

    let (set_num, binding_num) = require_descriptor_set_binding_nums(tcx, did);

    let set_num = tcx.mk_u32_cv(set_num);
    let binding_num = tcx.mk_u32_cv(binding_num);

    let tup_ty = tcx.mk_tup([tcx.types.u32, tcx.types.u32].iter());
    let out_ty = self.output(tcx);
    let tuple = vec![set_num, binding_num, ].into_iter();
    let (alloc_id, ..) =
      static_tuple_alloc(tcx, "compute desc set binding nums",
                         tuple, tup_ty);

    let ptr = Pointer::from(alloc_id);
    let const_val = ConstValue::Scalar(Scalar::Ptr(ptr));
    let rvalue = const_value_rvalue(tcx, const_val, out_ty);

    let ret = mir::Place::return_place();
    let stmt_kind = StatementKind::Assign(Box::new((ret, rvalue)));
    let stmt = Statement {
      source_info,
      kind: stmt_kind,
    };
    bb.statements.push(stmt);
    mir.basic_blocks_mut().push(bb);
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    1
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    tcx.intern_type_list(&[])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    tcx.mk_imm_ref(tcx.lifetimes.re_static,
                   tcx.mk_tup([tcx.types.u32, tcx.types.u32].iter()))
  }
}

impl GeobacterCustomIntrinsicMirGen for GraphicsPipelineLayoutDesc {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   _kid_did: &dyn DriverData,
                                   _tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   _mir: &mut mir::BodyAndCache<'tcx>)
  {
    // Create an empty function:
    /*let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let bb = mir::BasicBlockData {
      statements: vec![],
      terminator: Some(mir::Terminator {
        source_info,
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };
    mir.basic_blocks_mut().push(bb);

    // do the checking:
    let reveal_all = ParamEnv::reveal_all();

    let local = Local::new(1);
    let local_ty = mir.local_decls[local].ty;
    let local_ty = tcx
      .subst_and_normalize_erasing_regions(instance.substs,
                                           reveal_all,
                                           &local_ty);

    let kernel = extract_fn_instance(tcx, instance, kernel_ty);

    let did = instance.def_id();
    let ty = instance.ty(tcx);
    let sig = ty.fn_sig(tcx);
    let sig = tcx.normalize_erasing_late_bound_regions(reveal_all, &sig);
    // check that the function satisfies Fn<(), Output = ()>:
    if sig.inputs().len() != 0 {
      let msg = "shader/kernel function must accept no parameters";
      tcx.sess.span_err(tcx.def_span(did), &msg);
    }
    if sig.output() != tcx.types.unit {
      let msg = format!("shader/kernel function must return `()`; found {:?}",
                        sig.output());
      tcx.sess.span_err(tcx.def_span(did), &msg);
    }

    // collect all referenced mono items upfront:
    let mono_root = MonoItem::Fn(instance);
    let mut visited: FxHashSet<_> = Default::default();
    collect_items_rec(tcx, mono_root, &mut visited,
                      &mut None);

    // TODO Question: should we report a stack trace of an errant item's
    // usage?

    // visit each mono item and check annotated requirements are
    // respected
    visited.remove(&mono_root);

    let root_attrs = geobacter_root_attrs(tcx, did,
                                           self.0,
                                           true);

    for mono in visited.into_iter() {
      let (mono_did, mono_attrs) = match mono {
        MonoItem::Fn(inst) => {
          let did = inst.def_id();
          (did, geobacter_global_attrs(tcx, did, true))
        },
        MonoItem::Static(mono_did) => {
          (mono_did, geobacter_global_attrs(tcx, mono_did, true))
        },
        MonoItem::GlobalAsm(..) => {
          bug!("unexpected `{:?}`", mono);
        },
      };

      if !mono_attrs.capabilities.eval(&|cap| root_attrs.capabilities.contains(cap) ) {
        let msg = "unsatisfied capability (TODO which one???)";
        tcx.sess.span_err(tcx.def_span(mono_did), &msg);
      }

      if !mono_attrs.exe_model.eval(&|model| model == &root_attrs.execution_model ) {
        let msg = "unsatisfied execution model requirement";
        tcx.sess.span_err(tcx.def_span(mono_did), &msg);
      }

      // TODO check Vulkan requirements for builtins, inputs, outputs, etc
    }*/
    unimplemented!();
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    5
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    let p = |n: u32| {
      let p = Symbol::intern(&format!("P{}", n));
      let f = tcx.mk_ty_param(n, p);
      let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                                 ty::BrAnon(n)));
      tcx.mk_imm_ref(region, f)
    };
    tcx.intern_type_list(&[p(0), p(1), p(2), p(3), p(4), ])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    desc_set_bindings_desc_ty(tcx)
  }
}
impl GeobacterCustomIntrinsicMirGen for GraphicsPipelineRequiredCapabilities {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   _kid_did: &dyn DriverData,
                                   _tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   _mir: &mut mir::BodyAndCache<'tcx>)
  {
    /*// Create an empty function:
    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let bb = mir::BasicBlockData {
      statements: vec![],
      terminator: Some(mir::Terminator {
        source_info,
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };
    mir.basic_blocks_mut().push(bb);

    // do the checking:
    let reveal_all = ParamEnv::reveal_all();

    let local = Local::new(1);
    let local_ty = mir.local_decls[local].ty;
    let local_ty = tcx
      .subst_and_normalize_erasing_regions(instance.substs,
                                           reveal_all,
                                           &local_ty);

    let kernel = extract_opt_fn_def_id(tcx, instance, mir, 0)
      .expect("not optional");

    let did = instance.def_id();
    let ty = instance.ty(tcx);
    let sig = ty.fn_sig(tcx);
    let sig = tcx.normalize_erasing_late_bound_regions(reveal_all, &sig);
    // check that the function satisfies Fn<(), Output = ()>:
    if sig.inputs().len() != 0 {
      let msg = "shader/kernel function must accept no parameters";
      tcx.sess.span_err(tcx.def_span(did), &msg);
    }
    if sig.output() != tcx.types.unit {
      let msg = format!("shader/kernel function must return `()`; found {:?}",
                        sig.output());
      tcx.sess.span_err(tcx.def_span(did), &msg);
    }

    // collect all referenced mono items upfront:
    let mono_root = MonoItem::Fn(instance);
    let mut visited: FxHashSet<_> = Default::default();
    collect_items_rec(tcx, mono_root, &mut visited,
                      &mut None);

    // TODO Question: should we report a stack trace of an errant item's
    // usage?

    // visit each mono item and check annotated requirements are
    // respected
    visited.remove(&mono_root);

    let root_attrs = geobacter_root_attrs(tcx, did,
                                           self.0,
                                           true);

    for mono in visited.into_iter() {
      let (mono_did, mono_attrs) = match mono {
        MonoItem::Fn(inst) => {
          let did = inst.def_id();
          (did, geobacter_global_attrs(tcx, did, true))
        },
        MonoItem::Static(mono_did) => {
          (mono_did, geobacter_global_attrs(tcx, mono_did, true))
        },
        MonoItem::GlobalAsm(..) => {
          bug!("unexpected `{:?}`", mono);
        },
      };

      if !mono_attrs.capabilities.eval(&|cap| root_attrs.capabilities.contains(cap) ) {
        let msg = "unsatisfied capability (TODO which one???)";
        tcx.sess.span_err(tcx.def_span(mono_did), &msg);
      }

      if !mono_attrs.exe_model.eval(&|model| model == &root_attrs.execution_model ) {
        let msg = "unsatisfied execution model requirement";
        tcx.sess.span_err(tcx.def_span(mono_did), &msg);
      }

      // TODO check Vulkan requirements for builtins, inputs, outputs, etc
    }*/

    unimplemented!();
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    5
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    let p = |n: u32| {
      let p = Symbol::intern(&format!("P{}", n));
      let f = tcx.mk_ty_param(n, p);
      let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                                 ty::BrAnon(n)));
      tcx.mk_imm_ref(region, f)
    };
    tcx.intern_type_list(&[p(0), p(1), p(2), p(3), p(4), ])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    mk_static_slice(tcx, tcx.types.u32)
  }
}
impl GeobacterCustomIntrinsicMirGen for GraphicsPipelineRequiredExtensions {
  fn mirgen_simple_intrinsic<'tcx>(&self,
                                   _stubs: &stubbing::Stubber,
                                   _kid_did: &dyn DriverData,
                                   _tcx: TyCtxt<'tcx>,
                                   _instance: ty::Instance<'tcx>,
                                   _mir: &mut mir::BodyAndCache<'tcx>)
  {
    /*// Create an empty function:
    let source_info = mir::SourceInfo {
      span: DUMMY_SP,
      scope: mir::OUTERMOST_SOURCE_SCOPE,
    };

    let bb = mir::BasicBlockData {
      statements: vec![],
      terminator: Some(mir::Terminator {
        source_info,
        kind: mir::TerminatorKind::Return,
      }),

      is_cleanup: false,
    };
    mir.basic_blocks_mut().push(bb);

    // do the checking:
    let reveal_all = ParamEnv::reveal_all();

    let local = Local::new(1);
    let local_ty = mir.local_decls[local].ty;
    let local_ty = tcx
      .subst_and_normalize_erasing_regions(instance.substs,
                                           reveal_all,
                                           &local_ty);

    let kernel = extract_opt_fn_def_id(tcx, instance, mir, 0)
      .expect("not optional");

    let did = instance.def_id();
    let ty = instance.ty(tcx);
    let sig = ty.fn_sig(tcx);
    let sig = tcx.normalize_erasing_late_bound_regions(reveal_all, &sig);
    // check that the function satisfies Fn<(), Output = ()>:
    if sig.inputs().len() != 0 {
      let msg = "shader/kernel function must accept no parameters";
      tcx.sess.span_err(tcx.def_span(did), &msg);
    }
    if sig.output() != tcx.types.unit {
      let msg = format!("shader/kernel function must return `()`; found {:?}",
                        sig.output());
      tcx.sess.span_err(tcx.def_span(did), &msg);
    }

    // collect all referenced mono items upfront:
    let mono_root = MonoItem::Fn(instance);
    let mut visited: FxHashSet<_> = Default::default();
    collect_items_rec(tcx, mono_root, &mut visited,
                      &mut None);

    // TODO Question: should we report a stack trace of an errant item's
    // usage?

    // visit each mono item and check annotated requirements are
    // respected
    visited.remove(&mono_root);

    let root_attrs = geobacter_root_attrs(tcx, did,
                                           self.0,
                                           true);

    for mono in visited.into_iter() {
      let (mono_did, mono_attrs) = match mono {
        MonoItem::Fn(inst) => {
          let did = inst.def_id();
          (did, geobacter_global_attrs(tcx, did, true))
        },
        MonoItem::Static(mono_did) => {
          (mono_did, geobacter_global_attrs(tcx, mono_did, true))
        },
        MonoItem::GlobalAsm(..) => {
          bug!("unexpected `{:?}`", mono);
        },
      };

      if !mono_attrs.capabilities.eval(&|cap| root_attrs.capabilities.contains(cap) ) {
        let msg = "unsatisfied capability (TODO which one???)";
        tcx.sess.span_err(tcx.def_span(mono_did), &msg);
      }

      if !mono_attrs.exe_model.eval(&|model| model == &root_attrs.execution_model ) {
        let msg = "unsatisfied execution model requirement";
        tcx.sess.span_err(tcx.def_span(mono_did), &msg);
      }

      // TODO check Vulkan requirements for builtins, inputs, outputs, etc
    }*/

    unimplemented!();
  }

  fn generic_parameter_count(&self, _tcx: TyCtxt) -> usize {
    5
  }
  /// The types of the input args.
  fn inputs<'tcx>(&self, tcx: TyCtxt<'tcx>)
    -> &'tcx ty::List<ty::Ty<'tcx>>
  {
    let p = |n: u32| {
      let p = Symbol::intern(&format!("P{}", n));
      let f = tcx.mk_ty_param(n, p);
      let region = tcx.mk_region(ty::ReLateBound(ty::INNERMOST,
                                                 ty::BrAnon(n)));
      tcx.mk_imm_ref(region, f)
    };
    tcx.intern_type_list(&[p(0), p(1), p(2), p(3), p(4), ])
  }
  /// The return type.
  fn output<'tcx>(&self, tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
    mk_static_slice(tcx, tcx.mk_static_str())
  }
}

fn build_compiler_descriptor_desc<'tcx>(tcx: TyCtxt<'tcx>,
                                        ty: DescriptorDesc)
  -> ConstValue<'tcx>
{
  let first = build_compiler_descriptor_desc_ty(tcx, ty.ty);
  let second = Some(tcx.mk_u32_cv(ty.array_count)).into_iter();
  let third = build_compiler_shader_stages(tcx, ty.stages);
  let forth = Some(tcx.mk_bool_cv(ty.readonly)).into_iter();

  let mut values = first;
  values.extend(second);
  values.extend(third.into_iter());
  values.extend(forth);

  let ty = desc_desc_ty(tcx);
  static_tuple_const_value(tcx, "compiler_descriptor_desc",
                           values.into_iter(), ty)
}
fn build_compiler_shader_stages<'tcx>(tcx: TyCtxt<'tcx>,
                                      ty: ShaderStages)
  -> Vec<ConstValue<'tcx>>
{
  let tuple = [
    ty.vertex,
    ty.tessellation_control,
    ty.tessellation_evaluation,
    ty.geometry,
    ty.fragment,
    ty.compute,
  ];
  tuple.iter().map(|&b| tcx.mk_bool_cv(b) ).collect()
}
fn build_compiler_descriptor_desc_ty<'tcx>(tcx: TyCtxt<'tcx>,
                                           ty: DescriptorDescTy)
  -> Vec<ConstValue<'tcx>>
{
  println!("desc ty: {:?}", ty);
  let kind = match ty {
    DescriptorDescTy::Sampler => CompilerDescriptorDescTyKind::Sampler,
    DescriptorDescTy::CombinedImageSampler(..) => CompilerDescriptorDescTyKind::CombinedImageSampler,
    DescriptorDescTy::Image(..) => CompilerDescriptorDescTyKind::Image,
    DescriptorDescTy::TexelBuffer { .. } => CompilerDescriptorDescTyKind::TexelBuffer,
    DescriptorDescTy::InputAttachment { .. } => CompilerDescriptorDescTyKind::InputAttachment,
    DescriptorDescTy::Buffer(_) => CompilerDescriptorDescTyKind::Buffer,
  };
  let combined_image_sampler = match ty {
    DescriptorDescTy::CombinedImageSampler(desc) => Some(desc),
    _ => None,
  };
  let image = match ty {
    DescriptorDescTy::Image(desc) => Some(desc),
    _ => None,
  };
  let texel_buffer = match ty {
    DescriptorDescTy::TexelBuffer {
      storage,
      format,
    } => Some((storage, format)),
    _ => None,
  };
  let input_attachment = match ty {
    DescriptorDescTy::InputAttachment {
      multisampled,
      array_layers,
    } => Some((multisampled, array_layers)),
    _ => None,
  };
  let buffer = match ty {
    DescriptorDescTy::Buffer(desc) => Some(desc),
    _ => None,
  };

  let kind = tcx.mk_u32_cv(kind.into());
  let combined_image_sampler = build_compiler_opt(tcx, combined_image_sampler,
                                                  build_compiler_descriptor_img_desc);
  let image = build_compiler_opt(tcx, image,
                                 build_compiler_descriptor_img_desc);
  let texel_buffer = build_compiler_opt(tcx, texel_buffer,
                                        build_compiler_descriptor_texel_buffer);
  let input_attachment = build_compiler_opt(tcx, input_attachment,
                                            build_compiler_descriptor_input_attachment);
  let buffer = build_compiler_opt(tcx, buffer,
                                  build_compiler_descriptor_buffer);

  vec![kind, combined_image_sampler, image,
       texel_buffer, input_attachment, buffer, ]
}
fn build_compiler_descriptor_img_desc<'tcx>(tcx: TyCtxt<'tcx>,
                                            desc: DescriptorImageDesc)
  -> ConstValue<'tcx>
{
  let ty = mk_static_slice(tcx, desc_img_desc_ty(tcx));

  let sampled = tcx.mk_bool_cv(desc.sampled);
  let dims = CompilerDescriptorImageDims::from_vk(desc.dimensions).into();
  let dims = tcx.mk_u32_cv(dims);
  let format: Option<u32> = desc.format
    .map(|f| unsafe { ::std::mem::transmute(f) } );
  let format = build_compiler_opt(tcx, format, |_, v| tcx.mk_u32_cv(v) );
  let multisampled = tcx.mk_bool_cv(desc.multisampled);
  let array_layout = build_compiler_descriptor_img_array(tcx, desc.array_layers);

  let mut tuple = vec![sampled, dims, format, multisampled];
  tuple.extend(array_layout.into_iter());

  static_tuple_const_value(tcx, "compiler_descriptor_img_desc", tuple.into_iter(), ty)
}
fn build_compiler_descriptor_img_array<'tcx>(tcx: TyCtxt<'tcx>,
                                             desc: DescriptorImageDescArray)
  -> Vec<ConstValue<'tcx>>
{
  let (first, second) = match desc {
    DescriptorImageDescArray::NonArrayed => (true, None),
    DescriptorImageDescArray::Arrayed {
      max_layers,
    } => (false, max_layers),
  };

  let tuple = [
    tcx.mk_bool_cv(first),
    build_compiler_opt(tcx, second, |_, v| tcx.mk_u32_cv(v) ),
  ];

  tuple.iter().cloned().collect()
}
fn build_compiler_descriptor_texel_buffer<'tcx>(tcx: TyCtxt<'tcx>,
                                                desc: (bool, Option<Format>))
  -> ConstValue<'tcx>
{
  let storage = tcx.mk_bool_cv(desc.0);
  let format: Option<u32> = desc.1
    .map(|f| unsafe { ::std::mem::transmute(f) } );
  let format = build_compiler_opt(tcx, format, |_, v| tcx.mk_u32_cv(v) );

  let tup = [
    tcx.types.bool,
    mk_static_slice(tcx, vk_format_ty(tcx)),
  ];
  let ty = tcx.mk_tup(tup.iter());

  static_tuple_const_value(tcx, "compiler_descriptor_texel_buffer",
                           vec![storage, format].into_iter(),
                           ty)
}
fn build_compiler_descriptor_input_attachment<'tcx>(tcx: TyCtxt<'tcx>,
                                                    desc: (bool, DescriptorImageDescArray))
  -> ConstValue<'tcx>
{
  let multisampled = tcx.mk_bool_cv(desc.0);
  let array_layers = build_compiler_descriptor_img_array(tcx, desc.1);

  let tup = [
    tcx.types.bool,
    desc_img_array_ty(tcx),
  ];
  let ty = tcx.mk_tup(tup.iter());

  let mut values = vec![multisampled];
  values.extend(array_layers.into_iter());

  static_tuple_const_value(tcx, "compiler_descriptor_input_attachment",
                           values.into_iter(), ty)
}
fn build_compiler_descriptor_buffer<'tcx>(tcx: TyCtxt<'tcx>,
                                          desc: DescriptorBufferDesc)
  -> ConstValue<'tcx>
{
  let dynamic = build_compiler_opt(tcx, desc.dynamic, |_, v| tcx.mk_bool_cv(v) );
  let storage = tcx.mk_bool_cv(desc.storage);

  let ty = desc_buffer_desc_ty(tcx);

  static_tuple_const_value(tcx, "compiler_descriptor_buffer",
                           vec![dynamic, storage].into_iter(),
                           ty)
}

fn desc_img_dims_ty<'tcx>(tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
  tcx.types.u32
}
fn desc_img_array_ty<'tcx>(tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
  let tup = [tcx.types.bool, mk_static_slice(tcx, tcx.types.u32), ];
  tcx.mk_tup(tup.iter())
}
fn vk_format_ty<'tcx>(tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
  tcx.types.u32
}
fn desc_img_desc_ty<'tcx>(tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
  let tup = [
    tcx.types.bool,
    desc_img_dims_ty(tcx),
    mk_static_slice(tcx, vk_format_ty(tcx)),
    tcx.types.bool,
    desc_img_array_ty(tcx),
  ];
  tcx.mk_tup(tup.iter())
}
fn desc_buffer_desc_ty<'tcx>(tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
  let tup = [
    mk_static_slice(tcx, tcx.types.bool),
    tcx.types.bool,
  ];
  tcx.mk_tup(tup.iter())
}
fn desc_desc_ty_kind_ty<'tcx>(tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
  tcx.types.u32
}
fn desc_desc_ty_ty<'tcx>(tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
  let tup = [
    desc_desc_ty_kind_ty(tcx),
    mk_static_slice(tcx, desc_img_desc_ty(tcx)),
    mk_static_slice(tcx, desc_img_desc_ty(tcx)),
    mk_static_slice(tcx, {
      let tup = [
        tcx.types.bool,
        mk_static_slice(tcx, vk_format_ty(tcx)),
      ];
      tcx.mk_tup(tup.iter())
    }),
    mk_static_slice(tcx, {
      let tup = [
        tcx.types.bool,
        desc_img_array_ty(tcx),
      ];
      tcx.mk_tup(tup.iter())
    }),
    mk_static_slice(tcx, desc_buffer_desc_ty(tcx))
  ];
  tcx.mk_tup(tup.iter())
}
fn shader_stages_ty<'tcx>(tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
  let tup = [tcx.types.bool; 6];
  tcx.mk_tup(tup.iter())
}
fn desc_desc_ty<'tcx>(tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
  let tup = [
    desc_desc_ty_ty(tcx),
    tcx.types.u32,
    shader_stages_ty(tcx),
    tcx.types.bool,
  ];
  tcx.mk_tup(tup.iter())
}
fn desc_bindings_desc_ty<'tcx>(tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
  mk_static_slice(tcx, mk_static_slice(tcx, desc_desc_ty(tcx)))
}
fn desc_set_bindings_desc_ty<'tcx>(tcx: TyCtxt<'tcx>) -> ty::Ty<'tcx> {
  mk_static_slice(tcx, desc_bindings_desc_ty(tcx))
}
