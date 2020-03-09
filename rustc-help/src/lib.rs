//! Common driver code shared between all three drivers. This can not depend on
//! geobacter_core.

#![feature(rustc_private)]
#![feature(generators, generator_trait)]
#![feature(never_type)]
#![feature(specialization)]

#![recursion_limit="256"]

#[macro_use]
extern crate rustc;
extern crate rustc_data_structures;
extern crate rustc_hir;
extern crate rustc_index;
extern crate rustc_target;
extern crate rustc_span;
extern crate serialize as rustc_serialize;
extern crate syntax;
#[macro_use]
extern crate log;

extern crate geobacter_shared_defs as shared_defs;

use std::borrow::Cow;
use std::fmt;
use std::iter::{repeat, };

use crate::codec::GeobacterDecoder;

use crate::shared_defs::{kernel::KernelDesc, };

use rustc::middle::lang_items::{self, LangItem, };
use crate::rustc::mir::{Constant, Operand, Rvalue, Place, };
use crate::rustc::mir::interpret::{ConstValue, Scalar, Pointer,
                                   ScalarMaybeUndef, AllocId,
                                   Allocation, };
use crate::rustc::mir::{self, CustomIntrinsicMirGen, };
use crate::rustc::ty::{self, TyCtxt, layout::Size, Instance, InstanceDef, };
use crate::rustc::ty::{Const, ParamEnv, Tuple, Array, ConstKind, };
use rustc_hir::def_id::DefId;
use rustc_index::vec::Idx;
use crate::rustc_serialize::Decodable;
use crate::rustc_target::abi::{FieldPlacement, Align, HasDataLayout, };
use rustc_span::{DUMMY_SP, Span, };

pub mod codec;
pub mod driver_data;
pub mod intrinsics;
pub mod stubbing;

pub fn call_device_func<'tcx, F>(tcx: TyCtxt<'tcx>,
                                 mir: &mut mir::BodyAndCache<'tcx>,
                                 f: F)
  where F: FnOnce() -> Option<Instance<'tcx>>,
{
  redirect_or_panic(tcx, mir, "Device function called on the host",
                    move || Some((f()?, vec![])) );
}
pub fn call_device_func_args<'tcx, F>(tcx: TyCtxt<'tcx>,
                                      mir: &mut mir::BodyAndCache<'tcx>,
                                      f: F)
  where F: FnOnce() -> Option<(Instance<'tcx>, Vec<Operand<'tcx>>)>,
{
  redirect_or_panic(tcx, mir, "Device function called on the host",
                    f);
}

/// Either call the instance returned from `f` or insert code to panic.
/// TODO this should probably be turned into an attribute so it's more systematic.
pub fn redirect_or_panic<'tcx, F>(tcx: TyCtxt<'tcx>,
                                  mir: &mut mir::BodyAndCache<'tcx>,
                                  msg: &str, f: F)
  where F: FnOnce() -> Option<(Instance<'tcx>, Vec<Operand<'tcx>>)>,
{
  fn langcall(tcx: TyCtxt,
                  span: Option<Span>,
                  msg: &str,
                  li: LangItem)
    -> DefId
  {
    tcx.lang_items().require(li).unwrap_or_else(|s| {
      let msg = format!("{} {}", msg, s);
      match span {
        Some(span) => tcx.sess.span_fatal(span, &msg[..]),
        None => tcx.sess.fatal(&msg[..]),
      }
    })
  }

  let source_info = mir::SourceInfo {
    span: DUMMY_SP,
    scope: mir::OUTERMOST_SOURCE_SCOPE,
  };

  let mut bb = mir::BasicBlockData {
    statements: Vec::new(),
    terminator: Some(mir::Terminator {
      source_info: source_info.clone(),
      kind: mir::TerminatorKind::Return,
    }),

    is_cleanup: false,
  };

  let (real_instance, args, term_kind) = match f() {
    Some((instance, args)) => {
      (instance, args, mir::TerminatorKind::Return)
    },
    None => {
      // call `panic` from `libcore`
      let lang_item = lang_items::PanicFnLangItem;

      let def_id = langcall(tcx, None, "", lang_item);
      let mut instance = Instance::mono(tcx, def_id);
      instance.def = InstanceDef::ReifyShim(def_id);
      let desc = tcx.mk_static_str_operand(source_info, msg);

      (instance, vec![desc, ], mir::TerminatorKind::Unreachable)
    },
  };

  debug!("mirgen intrinsic into {}", real_instance);

  let success = mir::BasicBlock::new(mir.basic_blocks().next_index().index() + 1);
  let fn_ty = real_instance.monomorphic_ty(tcx);
  bb.terminator.as_mut()
    .unwrap()
    .kind = mir::TerminatorKind::Call {
    func: tcx.mk_const_op(source_info.clone(),
                          *ty::Const::zero_sized(tcx, fn_ty)),
    args,
    destination: Some((Place::return_place(), success)),
    cleanup: None,
    from_hir_call: false,
  };
  mir.basic_blocks_mut().push(bb);
  let bb = mir::BasicBlockData {
    statements: Vec::new(),
    terminator: Some(mir::Terminator {
      source_info: source_info.clone(),
      kind: term_kind,
    }),

    is_cleanup: false,
  };
  mir.basic_blocks_mut().push(bb);
}

// TODO report a helpful message if a closure is given.

pub fn extract_fn_instance<'tcx>(tcx: TyCtxt<'tcx>,
                                 instance: ty::Instance<'tcx>,
                                 local_ty: ty::Ty<'tcx>)
  -> ty::Instance<'tcx>
{
  let reveal_all = ParamEnv::reveal_all();

  let mut local_ty = tcx
    .subst_and_normalize_erasing_regions(instance.substs,
                                         reveal_all,
                                         &local_ty);

  loop {
    let instance = match local_ty.kind {
      ty::Ref(_, &ty::TyS {
        kind: ty::FnDef(def_id, subs),
        ..
      }, ..) |
      ty::FnDef(def_id, subs) => {
        let subs = tcx
          .subst_and_normalize_erasing_regions(instance.substs,
                                               reveal_all,
                                               &subs);
        ty::Instance::resolve(tcx, reveal_all, def_id, subs)
          .expect("must be resolvable")
      },
      ty::Ref(_, inner @ &ty::TyS {
        kind: ty::Ref(..),
        ..
      }, ..) => {
        local_ty = inner;
        continue;
      },
      _ => {
        unreachable!("unexpected param type: {:?}", local_ty);
      },
    };

   return instance;
  }
}

pub fn extract_opt_fn_instance<'tcx>(tcx: TyCtxt<'tcx>,
                                     instance: ty::Instance<'tcx>,
                                     local_ty: ty::Ty<'tcx>)
  -> Option<ty::Instance<'tcx>>
{
  let reveal_all = ParamEnv::reveal_all();

  let mut local_ty = tcx
    .subst_and_normalize_erasing_regions(instance.substs,
                                         reveal_all,
                                         &local_ty);

  loop {
    if local_ty == tcx.types.unit { return None; }

    let instance = match local_ty.kind {
      ty::Ref(_, &ty::TyS {
        kind: ty::FnDef(def_id, subs),
        ..
      }, ..) |
      ty::FnDef(def_id, subs) => {
        let subs = tcx
          .subst_and_normalize_erasing_regions(instance.substs,
                                               reveal_all,
                                               &subs);
        ty::Instance::resolve(tcx, reveal_all, def_id, subs)
          .expect("must be resolvable")
      },
      ty::Ref(_, inner @ &ty::TyS {
        kind: ty::Ref(..),
        ..
      }, ..) => {
        local_ty = inner;
        continue;
      },
      _ => {
        unreachable!("unexpected param type: {:?}", local_ty);
      },
    };

    return Some(instance);
  }
}

pub trait GeobacterTyCtxtHelp<'tcx>: Copy {
  fn as_tcx(self) -> TyCtxt<'tcx>;

  fn mk_const_op(self,
                 src: mir::SourceInfo,
                 c: ty::Const<'tcx>) -> Operand<'tcx> {
    let v = Constant {
      span: src.span,
      literal: self.as_tcx().mk_const(c),
      user_ty: None,
    };
    let v = Box::new(v);
    Operand::Constant(v)
  }

  fn mk_bool_cv(self, v: bool) -> ConstValue<'tcx> {
    let v = Scalar::from_bool(v);
    ConstValue::Scalar(v)
  }
  fn mk_u32_cv(self, v: u32) -> ConstValue<'tcx> {
    let v = Scalar::from_uint(v, Size::from_bytes(4));
    ConstValue::Scalar(v)
  }
  fn mk_u64_cv(self, v: u64) -> ConstValue<'tcx> {
    let v = Scalar::from_uint(v, Size::from_bytes(8));
    ConstValue::Scalar(v)
  }
  fn mk_usize_cv(self, v: impl Into<u128>) -> ConstValue<'tcx> {
    let size = self.as_tcx().data_layout().pointer_size;
    let v = Scalar::from_uint(v, size);
    ConstValue::Scalar(v)
  }
  fn mk_usize_c(self, v: impl Into<u128>) -> &'tcx ty::Const<'tcx> {
    self.as_tcx().mk_const(ty::Const {
      ty: self.as_tcx().types.usize,
      val: ConstKind::Value(self.mk_usize_cv(v)),
    })
  }

  fn mk_static_str_operand(self, source_info: mir::SourceInfo,
                           v: &str)
    -> Operand<'tcx>
  {
    let tcx = self.as_tcx();
    let id = tcx.allocate_bytes(v.as_bytes());
    let v = ConstValue::Slice {
      data: tcx.alloc_map.lock().unwrap_memory(id),
      start: 0,
      end: v.len(),
    };
    let v = tcx.mk_const(Const {
      ty: tcx.mk_static_str(),
      val: ConstKind::Value(v),
    });
    let v = Constant {
      span: source_info.span,
      literal: v,
      user_ty: None,
    };
    Operand::Constant(Box::new(v))
  }

  fn mk_u64_operand(self, source_info: mir::SourceInfo,
                    v: u64)
    -> Operand<'tcx>
  {
    let tcx = self.as_tcx();
    let v = self.mk_u64_cv(v);
    let v = tcx.mk_const(Const {
      ty: tcx.types.u64,
      val: ConstKind::Value(v),
    });
    let v = Constant {
      span: source_info.span,
      literal: v,
      user_ty: None,
    };
    let v = Box::new(v);
    Operand::Constant(v)
  }

  fn convert_kernel_instance<T>(self, k: T)
    -> Option<Instance<'tcx>>
    where T: KernelDesc,
  {
    trace!("converting kernel instance for {}",
           k.name().unwrap());

    let mut alloc_state = None;
    let mut decoder = GeobacterDecoder::new(self.as_tcx(),
                                            k.data(),
                                            &mut alloc_state);

    Instance::decode(&mut decoder).ok()
  }
}
impl<'tcx> GeobacterTyCtxtHelp<'tcx> for TyCtxt<'tcx> {
  fn as_tcx(self) -> TyCtxt<'tcx> { self }
}

// TODO move the following functions into `GeobacterTyCtxtHelp`.

pub fn build_compiler_opt<'tcx, F, T>(tcx: TyCtxt<'tcx>,
                                      val: Option<T>,
                                      some_val: F)
  -> ConstValue<'tcx>
  where F: FnOnce(TyCtxt<'tcx>, T) -> ConstValue<'tcx>,
{
  if let Some(val) = val {
    let val = some_val(tcx, val);
    let alloc = match val {
      ConstValue::Scalar(Scalar::Ptr(ptr)) => {
        tcx.alloc_map.lock().unwrap_memory(ptr.alloc_id)
      },
      ConstValue::Scalar(Scalar::Raw { size, .. }) => {
        // create an allocation for this

        let scalar = match val {
          ConstValue::Scalar(s) => s,
          _ => unreachable!(),
        };

        let size = Size::from_bytes(size as _);
        let align = Align::from_bytes(1).unwrap();
        let mut alloc = Allocation::undef(size, align);
        let alloc_id = tcx.alloc_map.lock().reserve();

        let ptr = Pointer::from(alloc_id);
        alloc.write_scalar(&tcx, ptr,
                           ScalarMaybeUndef::Scalar(scalar),
                           size)
          .expect("allocation write failed");

        let alloc = tcx.intern_const_alloc(alloc);
        tcx.alloc_map.lock().set_alloc_id_memory(alloc_id, alloc);

        alloc
      },
      val => unimplemented!("scalar type {:?}", val),
    };
    ConstValue::Slice {
      data: alloc,
      start: 0,
      end: 1,
    }
  } else {
    // Create an empty slice to represent a None value:
    const C: &'static [u8] = &[];
    let alloc = Allocation::from_byte_aligned_bytes(Cow::Borrowed(C));
    let alloc = tcx.intern_const_alloc(alloc);
    tcx.alloc_map.lock().create_memory_alloc(alloc);
    ConstValue::Slice {
      data: alloc,
      start: 0,
      end: 0,
    }
  }
}

pub fn const_value_rvalue<'tcx>(tcx: TyCtxt<'tcx>,
                                const_val: ConstValue<'tcx>,
                                ty: ty::Ty<'tcx>)
  -> Rvalue<'tcx>
{
  let source_info = mir::SourceInfo {
    span: DUMMY_SP,
    scope: mir::OUTERMOST_SOURCE_SCOPE,
  };

  let constant = tcx.mk_const(Const {
    ty,
    val: ConstKind::Value(const_val),
  });
  let constant = Constant {
    span: source_info.span,
    literal: constant,
    user_ty: None,
  };
  let constant = Box::new(constant);
  let constant = Operand::Constant(constant);

  Rvalue::Use(constant)
}

pub fn static_str_const_value<'tcx>(tcx: TyCtxt<'tcx>, s: &str)
  -> ConstValue<'tcx>
{
  let id = tcx.allocate_bytes(s.as_bytes());
  ConstValue::Slice {
    data: tcx.alloc_map.lock().unwrap_memory(id),
    start: 0,
    end: s.len(),
  }
}

pub fn static_tuple_const_value<'tcx, I>(tcx: TyCtxt<'tcx>,
                                         what: &str,
                                         tuple: I,
                                         ty: ty::Ty<'tcx>)
  -> ConstValue<'tcx>
  where I: ExactSizeIterator<Item = ConstValue<'tcx>>,
{
  let (alloc_id, ..) = static_tuple_alloc(tcx, what, tuple, ty);
  let ptr = Pointer::from(alloc_id);
  let scalar = Scalar::Ptr(ptr);
  ConstValue::Scalar(scalar)
}

pub fn static_tuple_alloc<'tcx, I>(tcx: TyCtxt<'tcx>,
                                   what: &str,
                                   tuple: I,
                                   ty: ty::Ty<'tcx>)
  -> (AllocId, &'tcx Allocation, Size)
  where I: ExactSizeIterator<Item = ConstValue<'tcx>>,
{
  let env = ParamEnv::reveal_all()
    .and(ty);
  let layout = tcx.layout_of(env)
    .expect("layout failure");
  let size = layout.details.size;
  let align = layout.details.align.pref;

  let data = vec![0; size.bytes() as usize];
  let mut alloc = Allocation::from_bytes(&data, align);
  let alloc_id = tcx.alloc_map.lock().reserve();

  let mut tuple = tuple.enumerate();

  write_static_tuple(tcx, what, &mut tuple, alloc_id, &mut alloc,
                     Size::ZERO, ty);

  assert_eq!(tuple.next(), None);

  let alloc = tcx.intern_const_alloc(alloc);
  tcx.alloc_map.lock().set_alloc_id_memory(alloc_id, alloc);
  (alloc_id, alloc, size)
}
pub fn write_static_tuple<'tcx, I>(tcx: TyCtxt<'tcx>,
                                   what: &str,
                                   tuple: &mut I,
                                   alloc_id: AllocId,
                                   alloc: &mut Allocation,
                                   base: Size,
                                   ty: ty::Ty<'tcx>)
  where I: ExactSizeIterator<Item = (usize, ConstValue<'tcx>)>,
{
  let env = ParamEnv::reveal_all()
    .and(ty);
  let layout = tcx.layout_of(env)
    .expect("layout failure");

  let fields = match layout.details.fields {
    FieldPlacement::Arbitrary {
      ref offsets,
      ..
    } => {
      offsets.clone()
    },
    FieldPlacement::Array {
      stride, count,
    } => {
      let offsets: Vec<_> = (0..count)
        .map(|idx| stride * idx )
        .collect();
      offsets
    },
    _ => unimplemented!("layout offsets {:?}", layout),
  };

  let ty_fields: Box<dyn Iterator<Item = ty::Ty<'tcx>>> = match ty.kind {
    Tuple(tuple_fields) => {
      assert_eq!(tuple_fields.len(), fields.len());
      Box::new(tuple_fields.types()) as Box<_>
    },
    Array(element, _count) => {
      Box::new(repeat(element)) as Box<_>
    },
    _ => unimplemented!("non tuple type: {:?}", ty),
  };

  for (mut offset, field_ty) in fields.into_iter().zip(ty_fields) {
    match field_ty.kind {
      Tuple(_) => {
        write_static_tuple(tcx, what, tuple, alloc_id, alloc,
                           base + offset, field_ty);
        continue;
      },
      Array(..) => {
        write_static_tuple(tcx, what, tuple, alloc_id, alloc,
                           base + offset, field_ty);
        continue;
      },
      _ => { },
    }

    let (index, element) = tuple.next()
      .expect("missing tuple field value");

    trace!("write tuple: {}, index {} at offset {}, ty: {:?}",
             what, index, (base + offset).bytes(), field_ty);

    let mut write_scalar = |scalar| {
      let ptr = Pointer::new(alloc_id, base + offset);
      let size = match scalar {
        Scalar::Raw { size, .. } => {
          Size::from_bytes(size as _)
        },
        Scalar::Ptr(_) => {
          tcx.data_layout().pointer_size
        },
      };
      offset += size;

      let scalar = ScalarMaybeUndef::Scalar(scalar);
      alloc.write_scalar(&tcx, ptr, scalar, size)
        .expect("allocation write failed");
    };

    match element {
      ConstValue::Scalar(scalar) => {
        write_scalar(scalar);
      },
      ConstValue::Slice { data, start, end, } => {
        // this process follows the same procedure as in rustc_codegen_ssa
        let id = tcx.alloc_map.lock().create_memory_alloc(data);
        let offset = Size::from_bytes(start as u64);
        let ptr = Pointer::new(id, offset);
        write_scalar(ptr.into());
        let size = Scalar::from_uint((end - start) as u128,
                                     tcx.data_layout().pointer_size);
        write_scalar(size);
      },
      _ => {
        bug!("unhandled ConstValue: {:?}", element);
      },
    }
  }
}

pub fn mk_static_slice<'tcx>(tcx: TyCtxt<'tcx>, elem: ty::Ty<'tcx>) -> ty::Ty<'tcx> {
  tcx.mk_imm_ref(tcx.lifetimes.re_static, tcx.mk_slice(elem))
}
