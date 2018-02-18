
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use indexvec::Idx;

use rustc::hir::def_id::{DefId};
use rustc::infer::TransNormalize;
use rustc::middle::const_val::{self, ConstAggregate};
use rustc::mir::{self};
use rustc::ty::{self, TyS, subst, TyCtxt, Instance, InstanceDef,
                Binder, Slice, ExistentialPredicate};
use rustc::ty::subst::Subst;
use rustc::ty::layout::{LayoutTyper, HasDataLayout,
                        TargetDataLayout, LayoutCx,
                        TyLayout};
use rustc_data_structures::indexed_vec::{Idx as RustcIdx};
use rustc_const_math::{ConstInt, ConstFloat, ConstUsize, ConstIsize};
use rustc_trans::monomorphize;

pub use syntax_pos::{Span, BytePos, SyntaxContext};

pub use tys::{Mutability, Ty, TyData, PrimTy, Signedness, TypeAndMut};
use tys;
use rustc_wrappers::{DefIdDef};

use super::{Substs, ConstVal};

pub mod ty_util;

pub type VTableKey<'tcx> = (ty::Ty<'tcx>,
                            Option<ty::PolyExistentialTraitRef<'tcx>>);

#[derive(Clone)]
pub struct LangItems {
  pub panic_fmt: DefId,
}

pub struct Module<'a, 'tcx>
  where 'tcx: 'a,
{
  pub mod_: super::Module,
  lang_items: LangItems,
  extern_substs: &'tcx subst::Substs<'tcx>,
  tcx: TyCtxt<'a, 'tcx, 'tcx>,

  queue: Vec<(Instance<'tcx>, super::Function)>,

  types: HashMap<&'tcx TyS<'tcx>, Ty>,
  substs: HashMap<&'tcx subst::Substs<'tcx>, Substs>,
  const_vals: HashMap<const_val::ConstVal<'tcx>, ConstVal>,
  adt_ids: HashMap<DefIdDef, super::AdtDef>,
  funcs: HashMap<ty::Instance<'tcx>, super::Function>,
  vtables: HashMap<VTableKey<'tcx>, (super::VTable, super::Ty)>,
  dynamic: HashMap<&'tcx Slice<ExistentialPredicate<'tcx>>, super::tys::Dynamic>
}

impl<'a, 'tcx> Module<'a, 'tcx> {
  pub fn new(tcx: TyCtxt<'a, 'tcx, 'tcx>,
             substs: &'tcx subst::Substs<'tcx>,
             lang_items: LangItems)
    -> Module<'a, 'tcx>
  {
    let mut m = super::Module::new();

    Module {
      mod_: m,
      lang_items,
      extern_substs: substs,
      tcx,

      queue: Vec::new(),

      types: Default::default(),
      substs: Default::default(),
      const_vals: Default::default(),
      adt_ids: Default::default(),
      funcs: Default::default(),
      vtables: Default::default(),
    }
  }
  pub fn build(&mut self, item: DefId) {
    let instance = InstanceDef::Item(item);
    let instance = Instance {
      def: instance,
      substs: self.extern_substs,
    };

    self.enter_function(instance, |mut main| {
      main.build();
    });
  }

  pub fn finish(self) -> super::Module {
    let Module {
      mod_,
      ..
    } = self;
    mod_
  }
}

impl<'parent, 'a, 'tcx, FIdx> Function<'parent, 'a, 'tcx, FIdx>
  where FIdx: IdxFunction,
        Self: Deref<Target = super::FunctionKind> + DerefMut,
{
  pub fn id(&self) -> FIdx { self.func.clone() }
  fn tcx(&self) -> TyCtxt<'parent, 'tcx, 'tcx> {
    self.mod_tcx()
  }

  fn build(&mut self) {
    let mir = match self.def.map(|v| v.def ) {
      Some(def) => {
        self.tcx().instance_mir(def)
      },
      _ => bug!("can't build hsa ir for {:?}", self.def),
    };
    self.mir = Some(mir);
    self.build_mir();
  }
  pub fn build_mir(&mut self) {
    use super::{StatementKind, TerminatorKind,
                Statement, Terminator, BasicBlockData,
                VisibilityScopeData, LocalDecl,
                UpvarDecl, BasicBlock, AssertMessage};

    let mir = self.mir
      .expect("can't build our mir without the mir!");

    let tcx = self.tcx();

    self.tcx().sess
      .span_note_without_error(mir.span,
                               "building this");

    let return_ty = self.monomorphize(&mir.return_ty);
    let return_ty = self.convert(&return_ty);
    self.return_ty = Some(return_ty);

    for (bbid, bb) in mir.basic_blocks().iter_enumerated() {
      let mut kstmts = Vec::new();

      for stmt in bb.statements.iter() {
        let kkind = match stmt.kind {
          mir::StatementKind::Assign(ref l, ref r) => {
            StatementKind::Assign(self.convert(l),
                                  self.convert(r))
          },
          mir::StatementKind::SetDiscriminant {
            ref lvalue, variant_index,
          } => StatementKind::SetDiscriminant {
            lvalue: self.convert(lvalue),
            variant_index,
          },
          mir::StatementKind::StorageLive(ref l) => {
            StatementKind::StorageLive(self.convert(l))
          },
          mir::StatementKind::StorageDead(ref l) => {
            StatementKind::StorageDead(self.convert(l))
          },
          mir::StatementKind::InlineAsm { .. } => {
            tcx.sess.span_fatal(stmt.source_info.span,
                                "inline asm not allowed \
                                 for hsa functions");
          },
          mir::StatementKind::Validate(..) => {
            StatementKind::Nop
          },
          mir::StatementKind::EndRegion(_) => {
            unreachable!()
          },
          mir::StatementKind::Nop => {
            StatementKind::Nop
          },
        };

        let mut kstmt = Statement {
          source_info: stmt.source_info.into(),
          kind: kkind,
        };

        kstmts.push(kstmt);
      }

      let terminator = bb.terminator.as_ref().unwrap();
      let ktkind = match &terminator.kind {
        &mir::TerminatorKind::Goto {
          target,
        } => TerminatorKind::Goto { target: target.into() },
        &mir::TerminatorKind::SwitchInt {
          ref discr, ref switch_ty, ref values, ref targets,
        } => TerminatorKind::SwitchInt {
          discr: self.convert(discr),
          switch_ty: self.convert(switch_ty),
          values: values.iter()
            .map(|&c| c.into() )
            .collect(),
          targets: targets.iter()
            .map(|&bb| bb.into() )
            .collect(),
        },
        &mir::TerminatorKind::Resume => TerminatorKind::Resume,
        &mir::TerminatorKind::Return => TerminatorKind::Return,
        &mir::TerminatorKind::Unreachable => TerminatorKind::Unreachable,
        &mir::TerminatorKind::Drop {
          ref location, target, ref unwind,
        } => TerminatorKind::Drop {
          location: self.convert(location),
          target: target.into(),
          unwind: unwind.map(|v| v.into() ),
        },
        &mir::TerminatorKind::DropAndReplace {
          ref location,
          ref value,
          target,
          ref unwind,
        } => TerminatorKind::DropAndReplace {
          location: self.convert(location),
          value: self.convert(value),
          target: target.into(),
          unwind: unwind.map(|v| v.into() ),
        },
        &mir::TerminatorKind::Call {
          ref func, ref args,
          ref destination,
          ref cleanup,
        } => {
          let fty = func.ty(mir, tcx);
          let fty = tcx.trans_apply_param_substs(self.param_substs(),
                                                 &fty);
          let finstance = match fty.sty {
            ty::TyFnDef(callee_def_id, callee_substs) => {
              monomorphize::resolve(tcx, callee_def_id,
                                    callee_substs)
            },
            ty::TyFnPtr(_) => {
              tcx.sess.span_fatal(terminator.source_info.span,
                                  "no function pointer calls allowed");
            },
            _ => bug!("{} is not callable", fty),
          };

          self.enter_function(finstance, |mut nested_fn| {
            match finstance.def {
              InstanceDef::Item(_) |
              InstanceDef::Virtual(..) |
              InstanceDef::ClosureOnceShim { .. } |
              InstanceDef::CloneShim(..) => {
                nested_fn.build()
              },
              InstanceDef::Intrinsic(idef_id) => {
                let name = (&nested_fn.tcx().item_name(idef_id)[..])
                  .to_string();

                let span = nested_fn.tcx().def_span(idef_id);
                nested_fn.tcx().sess
                  .span_note_without_error(span,
                                           "enter_function");

                let sig = func.ty(mir, nested_fn.tcx())
                  .fn_sig(nested_fn.tcx());
                let sig = sig.subst(nested_fn.tcx(),
                                    nested_fn.param_substs());
                let sig = nested_fn.tcx()
                  .erase_late_bound_regions_and_normalize(&sig);
                let sig = sig.inputs_and_output
                  .iter()
                  .map(|ty| nested_fn.convert(ty))
                  .collect();
                let data =
                  super::FunctionData::Intrinsic(name, sig);
                let id = nested_fn.func;
                nested_fn.module()
                  .mod_
                  .funcs[id] = data;
              },
              _ => {
                bug!("TODO: function instance: {:?}", finstance);
              },
            }
          });

          TerminatorKind::Call {
            func: self.convert(func),
            args: args.iter()
              .map(|v| self.convert(v))
              .collect(),
            destination: destination.as_ref()
              .map(|&(ref dest, destbb)| {
                (self.convert(dest), destbb.into())
              }),
            cleanup: cleanup.as_ref()
              .map(|&bb| bb.into()),
          }
        },
        &mir::TerminatorKyind::Assert {
          ref cond,
          expected,
          ref msg,
          ref target,
          ref cleanup,
        } => {
          let cond = self.convert(cond);
          let msg = match msg {
            &mir::AssertMessage::BoundsCheck {
              ref len, ref index,
            } => {
              let len = self.convert(len);
              let index = self.convert(index);
              AssertMessage::BoundsCheck {
                len, index,
              }
            },
            &mir::AssertMessage::Math(ref e) => {
              AssertMessage::Math(e.clone().into())
            },
            &mir::AssertMessage::GeneratorResumedAfterReturn |
            &mir::AssertMessage::GeneratorResumedAfterPanic => {
              unimplemented!("generators. TODO: better error reporting");
            },
          };

          TerminatorKind::Assert {
            cond,
            expected,
            msg,
            target: (*target).into(),
            cleanup: cleanup.map(|v| v.into() )
          }
        },
        &mir::TerminatorKind::Yield {
          ref value, ref resume, ref drop,
        } => {
          TerminatorKind::Yield {
            value: self.convert(value),
            resume: BasicBlock::new(resume.index()),
            drop: drop.map(|v| BasicBlock::new(v.index()) ),
          }
        },
        &mir::TerminatorKind::GeneratorDrop => TerminatorKind::GeneratorDrop,
      };

      let kterm = Terminator {
        source_info: terminator.source_info.into(),
        kind: ktkind,
      };

      let kbb = BasicBlockData {
        statements: kstmts,
        terminator: kterm,
        is_cleanup: bb.is_cleanup,
      };

      let new_idx = self.basic_blocks.push(kbb);
      assert_eq!(new_idx.index(), bbid.index());
    }

    for (id, vis) in mir.visibility_scopes.iter_enumerated() {
      let kvis = VisibilityScopeData {
        span: vis.span.into(),
        parent_scope: vis.parent_scope
          .map(|v| v.into() ),
      };

      let kid = self.visibility_scopes.push(kvis);
      assert_eq!(kid.index(), id.index());
    }

    let substs = self.param_substs();
    let fidx = self.id().function_idx();
    for (id, promoted) in mir.promoted.iter_enumerated() {
      tcx.sess
        .span_note_without_error(promoted.span,
                                 "promoted: building this");
      let f = super::FunctionData::new();
      let pidx = fidx.deref_mut(self.module())
        .promoted
        .push(f);
      assert_eq!(pidx.index(), id.index());

      let mut fun = Function {
        parent: self,
        mir: Some(promoted),
        func: (fidx, super::Promoted(id.index())),
        substs,
        def: None,
      };
      fun.build_mir();
    }

    for (id, ldecl) in mir.local_decls.iter_enumerated() {
      let ldecl_ty = ldecl.ty;
      let ldecl_ty = self.monomorphize(&ldecl_ty);
      let kldecl = LocalDecl {
        mutability: ldecl.mutability.into(),
        is_user_variable: ldecl.is_user_variable,
        ty: self.convert(&ldecl_ty),
        name: ldecl.name.map(|v| v.into()),
        source_info: ldecl.source_info.into(),
      };

      let kid = self.local_decls.push(kldecl);
      assert_eq!(kid.index(), id.index());
    }

    for upvar in mir.upvar_decls.iter() {
      let kupvar = UpvarDecl {
        debug_name: upvar.debug_name.into(),
        by_ref: upvar.by_ref,
      };

      self.upvar_decls.push(kupvar);
    }

    self.spread_arg = mir.spread_arg.map(|v| v.into() );
    self.span = mir.span.into();
  }

  pub fn convert<TI>(&mut self, t: &TI) -> TI::Target
    where TI: RustCConvert<'tcx>,
  {
    t.convert(self)
  }

  //pub fn convert_dynamic_ty(&mut self, )
  pub fn convert_adt_def(&mut self, adt: &'tcx ty::AdtDef,
                         subs: &'tcx subst::Substs<'tcx>)
    -> super::AdtDef
  {
    use tys::*;
    use tys::{VariantDef, FieldDef};

    let did: DefIdDef = adt.did.into();
    if let Some(&adt_id) = self.module_ref().adt_ids.get(&did) {
      return adt_id;
    }

    let idx = self.module().mod_.adts
      .push(None);
    self.module().adt_ids.insert(did, idx);

    let span = self.tcx().def_span(adt.did);
    self.tcx().sess
      .span_note_without_error(span,
                               "ADT: building this");

    let cadt = AdtDefData {
      did,
      variants: adt.variants.iter()
        .map(|v| {
          VariantDef {
            did: v.did.into(),
            name: format!("{}", v.name.as_str()),
            discr: v.discr.into(),
            fields: v.fields.iter()
              .map(|v| {
                let ty = v.ty(self.tcx(), subs);
                let ty =
                  self.convert(&ty);
                FieldDef {
                  did: v.did.into(),
                  ty,
                  name: format!("{}", v.name.as_str()),
                  vis: v.vis.into(),
                }
              })
              .collect(),
            ctor_kind: v.ctor_kind.into(),
          }
        })
        .collect(),
      flags: adt.into(),
      repr: adt.repr.into(),
    };

    self.module().mod_
      .adts[idx] = Some(cadt);
    idx
  }

  pub fn convert_ty(&mut self, ty: &'tcx TyS<'tcx>) -> Ty {
    use rustc::ty::*;
    use syntax::ast::IntTy;
    use syntax::ast::UintTy;
    use syntax::ast::FloatTy;

    if let Some(id) = self.module_ref().types.get(ty) {
      return *id;
    }

    let data = match ty.sty {
      TyBool => {
        TyData::Primitive(PrimTy::Bool)
      },
      TyChar => {
        TyData::Primitive(PrimTy::Char)
      },
      TyInt(v) => {
        TyData::Primitive(PrimTy::Int(v.into()))
      },
      TyUint(v) => {
        TyData::Primitive(PrimTy::Int(v.into()))
      },

      TyFloat(FloatTy::F32) => {
        TyData::Primitive(PrimTy::F32)
      },
      TyFloat(FloatTy::F64) => {
        TyData::Primitive(PrimTy::F64)
      },

      TyStr => TyData::Str,
      TyArray(inner, count) => {
        let ity = self.monomorphize(&inner);
        let ity = self.convert_ty(&ity);
        let count = self.convert(&count.val);
        TyData::Array(ity, count)
      },
      TySlice(inner) => {
        let ity = self.monomorphize(&inner);
        let ity = self.convert_ty(&ity);
        TyData::Slice(ity)
      },
      TyRawPtr(inner) => {
        let ity = self.monomorphize(&inner.ty);
        let ity = self.convert_ty(&ity);
        let inner = super::tys::TypeAndMut {
          ty: ity,
          mutbl: inner.mutbl.into(),
        };
        TyData::RawPtr(inner)
      },
      TyRef(&RegionKind::ReErased, inner) => {
        let is_fat = ty_util::type_is_fat_ptr(self.module_ref(),
                                              ty);
        if is_fat {
          println!("type is fat: {:?}", ty);
        }

        let region = super::tys::Region::Erased;
        let ity = self.convert_ty(inner.ty);
        let inner = super::tys::TypeAndMut {
          ty: ity,
          mutbl: inner.mutbl.into(),
        };
        TyData::Ref(region, inner)
      },
      TyTuple(ref inner, b) => {
        let inners: Vec<_> = inner.iter()
          .map(|v| self.convert(v) )
          .collect();
        TyData::Tuple(inners, b)
      },
      TyFnDef(def_id, subs) => {
        let subs = self.monomorphize(&subs);
        TyData::FnDef(def_id.into(),
                      self.convert(&subs))
      },
      TyClosure(def_id, subs) => {
        TyData::FnDef(def_id.into(),
                      self.convert(&subs.substs))
      },
      TyAdt(adt_def, subs) => {
        let subs = self.monomorphize(&subs);
        let cadt = self.convert_adt_def(adt_def, &subs);
        TyData::Adt(cadt, self.convert(&subs))
      },
      TyDynamic(ref traits, &ty::RegionKind::ReErased) => {
        let unbind = traits.skip_binder();


        let id = loop {
          if let Some(id) = self.module_ref().dynamic.get(unbind) {
            break id;
          }

          let data = tys::DynamicData {
            trait_data: unbind.iter()
              .map(|pred| {
                match pred {
                  rustc::ty::ExistentialPredicate::Trait(trait_ref) => {
                    let tr = tys::ExistentialTraitRef {
                      def_id: trait_ref.def_id.into(),
                      substs: trait_ref.substs.convert(),
                    };
                    let tr = tys::ExistentialPredicate::Trait(tr);
                  },
                  rustc::ty::ExistentialPredicate::Projection(proj) => {
                    bug!("proj: {:?}", proj)
                  },
                  rustc::ty::ExistentialPredicate::AutoTrait(id) => {
                    bug!("id: {:?}", id)
                  },
                }
              })
              .collect(),
          };

          let id = self.module()
            .mod_
            .dynamics
            .push(data);
          self.module()
            .dynamic
            .insert(unbind, id);
          break id;
        };

        let principal = traits.principal();
        let is_fat = ty_util::type_is_fat_ptr(self.module_ref(), ty);
        bug!("principal: {:?}, \n layout: {:?}", principal, is_fat);
      },
      TyNever => TyData::Never,
      _ => {
        bug!("type: {:?}", ty.sty);
      },
    };
    let idx = self.module().mod_.tys
      .push(data);
    self.module().types.insert(ty, idx);
    idx
  }

  pub fn convert_aggregate_kind(&mut self,
                                operand: Option<&mir::Rvalue<'tcx>>,
                                kind: &mir::AggregateKind<'tcx>) -> super::AggregateKind {
    use rustc::ty::subst::Kind;
    use rustc::ty::subst::Subst;
    use rustc_trans::common;

    use super::AggregateKind;
    match kind {
      &mir::AggregateKind::Array(ref ty) => {
        let ty = self.monomorphize(ty);
        return AggregateKind::Array(self.convert(&ty));
      },
      &mir::AggregateKind::Tuple => {
        return AggregateKind::Tuple;
      },
      &mir::AggregateKind::Adt(def, discr, substs, active_field) => {
        let subs = self.monomorphize(&substs);
        let adt_id = self.convert_adt_def(def, &subs);
        return AggregateKind::Adt(adt_id, discr,
                                  self.convert(&subs),
                                  active_field);
      },
      &mir::AggregateKind::Closure(def, substs) => {
        let tcx = self.tcx();
        let msubsts = self.monomorphize(&substs.substs);
        let instance = monomorphize::resolve(tcx, def, &msubsts);
        return AggregateKind::Closure(def.into(),
                                      self.convert(&instance.substs));
      },
      &mir::AggregateKind::Generator(..) => unimplemented!(),
    }

    println!("aggregate kind: {:?}", kind);
    unimplemented!()
  }

  /// Note: The vtable layout MUST match what rustc generates.
  pub fn convert_vtable(&mut self,
                        target_ty: ty::Ty<'tcx>,
                        source_ty: ty::Ty<'tcx>,
                        data_ty: ty::Ty<'tcx>)
    -> (super::VTable, Ty)
  {
    use rustc::ty::*;
    use rustc::traits;
    use rustc_trans::monomorphize::resolve_drop_in_place;
    assert!(!target_ty.needs_subst() && !target_ty.has_escaping_regions() &&
      !source_ty.needs_subst() && !source_ty.has_escaping_regions());

    let tcx = self.tcx();

    if let TyDynamic(ref trait_ty, ..) = target_ty.sty {

      let principle = trait_ty.principal();
      let key = (source_ty, principle);
      if let Some(&vtbl) = self.module().vtables.get(&key) {
        return vtbl;
      }

      let layout = self.module_ref().layout_of(source_ty);
      let mut vtable = super::VTableData {
        drop: {
          let instance = resolve_drop_in_place(tcx, source_ty);
          self.enqueue_function(instance)
        },
        size_of: layout.size(self.module_ref()).bytes(),
        align_of: layout.align(self.module_ref()).abi(),
        components: Vec::new(),
      };

      println!("principle: {:?}", principle);
      if let Some(principle) = principle {
        let poly_trait_ref = principle.with_self_ty(tcx, source_ty);
        println!("poly_trait_ref: {:?}", poly_trait_ref);
        assert!(!poly_trait_ref.has_escaping_regions());

        let methods = traits::get_vtable_methods(tcx, poly_trait_ref);
        let entries = methods
          .map(|opt| {
            opt.map(|(def_id, subs)| {
              monomorphize::resolve(tcx, def_id, subs)
            })
          })
          .map(|opt| {
            if let Some(instance) = opt {
              let f = self.enqueue_function(instance);
              super::VTableDataEntry::Fn(f)
            } else {
              super::VTableDataEntry::Null
            }
          });

        vtable.components.extend(entries);
      }

      let id = self.module().mod_.vtables
        .push(vtable);

      let target_ty = TyData::FatPtr {
        data: self.convert(&data_ty),
        info: id,
      };
      let target_ty_id = self.module().mod_.tys
        .push(target_ty);
      self.module().vtables
        .insert(key, (id, target_ty_id));

      (id, target_ty_id)
    } else {
      unreachable!();
    }
  }

  pub fn convert_rvalue(&mut self,
                        rvalue: &mir::Rvalue<'tcx>) -> super::Rvalue {
    use super::Rvalue;
    match rvalue {
      &mir::Rvalue::Use(ref o) => Rvalue::Use(self.convert(o)),
      &mir::Rvalue::Repeat(ref o, c) =>
        Rvalue::Repeat(self.convert(o), From::from(c)),
      &mir::Rvalue::Ref(&ty::RegionKind::ReErased,
                        borrow, ref v) =>
        Rvalue::Ref(super::RegionKind::Erased,
                    From::from(borrow),
                    self.convert(v)),
      &mir::Rvalue::Ref(..) => unreachable!(),
      &mir::Rvalue::Len(ref v) => Rvalue::Len(self.convert(v)),
      &mir::Rvalue::Cast(mir::CastKind::Unsize, ref operand,
                         ref target_ty) => {
        use tys::TyData;
        use rustc_trans::collector::find_vtable_types_for_unsizing;

        let tcx = self.tcx();
        let target_ty = self.monomorphize(target_ty);
        let target_ty = tcx
          .trans_apply_param_substs(self.param_substs(),
                                    &target_ty);
        let source_ty = operand.ty(self.mir.unwrap(), tcx);
        let source_ty = tcx
          .trans_apply_param_substs(self.param_substs(),
                                    &source_ty);

        let (source_ty, target_ty) =
          find_vtable_types_for_unsizing(tcx, source_ty,
                                         target_ty);

        if let Some(did) = source_ty.ty_to_def_id() {
          let sp = tcx.def_span(did);
          tcx.sess
            .span_note_without_error(sp,
                                     "CastKind::Unsize here");
        }
        println!("source_ty: {:?}", source_ty);
        println!("target_ty: {:?}", target_ty);

        let operand = self.convert(operand);
        if target_ty.is_trait() && !source_ty.is_trait() {
          let (_, target_ty) = self
            .convert_vtable(target_ty, source_ty, source_ty);

          Rvalue::Cast(super::CastKind::Unsize,
                       operand,
                       target_ty)
        } else {
          Rvalue::Cast(super::CastKind::Unsize,
                       operand,
                       self.convert(&target_ty))
        }
      },
      &mir::Rvalue::Cast(kind, ref operand, ref ty) => {
        println!("cast kind => {:?}", kind);
        println!("operand => {:?}", operand);
        let ty = self.monomorphize(ty);
        println!("cast_ty => {:?}", ty);
        println!("type_is_fat_ptr => {}",
                 ty_util::type_is_fat_ptr(self.module_ref(), ty));
        Rvalue::Cast(kind.into(),
                     self.convert(operand),
                     self.convert(&ty))
      },
      &mir::Rvalue::BinaryOp(op, ref l, ref r) =>
        Rvalue::BinaryOp(op.into(), self.convert(l),
                         self.convert(r)),
      &mir::Rvalue::CheckedBinaryOp(op, ref l, ref r) =>
        Rvalue::CheckedBinaryOp(op.into(), self.convert(l),
                                self.convert(r)),
      &mir::Rvalue::NullaryOp(op, ref ty) => {
        let ty = self.monomorphize(ty);
        Rvalue::NullaryOp(From::from(op), self.convert(&ty))
      },
      &mir::Rvalue::UnaryOp(op, ref o) =>
        Rvalue::UnaryOp(From::from(op), self.convert(o)),
      &mir::Rvalue::Discriminant(ref d) =>
        Rvalue::Discriminant(self.convert(d)),
      &mir::Rvalue::Aggregate(ref kind, ref ops) => {
        let agg = self.convert_aggregate_kind(Some(rvalue),
                                              &**kind);
        let ops = ops.iter()
          .map(|v| self.convert(v) )
          .collect();
        Rvalue::Aggregate(Box::new(agg), ops)
      },
    }
  }

  pub fn convert_operand(&mut self,
                         operand: &mir::Operand<'tcx>) -> super::Operand {
    use super::Operand;
    match operand {
      &mir::Operand::Consume(ref l) => {
        Operand::Consume(l.convert(self))
      },
      &mir::Operand::Constant(ref c) => {
        Operand::Constant(Box::new(self.convert(&**c)))
      },
    }
  }

  pub fn convert_constant(&mut self,
                          constant: &mir::Constant<'tcx>) -> super::Constant {
    super::Constant {
      span: constant.span.into(),
      ty: constant.ty.convert(self),
      literal: constant.literal.convert(self),
    }
  }
  pub fn convert_literal(&mut self,
                         literal: &mir::Literal<'tcx>) -> super::Literal {
    use super::Literal;
    match literal {
      &mir::Literal::Value {
        ref value,
      } => Literal::Value {
        value: self.convert(&value.val),
      },
      &mir::Literal::Promoted {
        index,
      } => Literal::Promoted(index.into()),
    }
  }
  pub fn convert_constval(&mut self,
                          val: &const_val::ConstVal<'tcx>) -> super::ConstVal {
    use super::ConstValData;

    if let Some(id) = self.module_ref().const_vals.get(val) {
      return id.clone();
    }

    let data = match val {
      &const_val::ConstVal::Float(f) =>
        ConstValData::Float(f.into()),
      &const_val::ConstVal::Integral(i) =>
        ConstValData::Integral(i.into()),
      &const_val::ConstVal::Str(ref s) =>
        ConstValData::Str(s.to_string()),
      &const_val::ConstVal::ByteStr(ref b) =>
        ConstValData::ByteStr(b.data.to_vec()),
      &const_val::ConstVal::Bool(b) =>
        ConstValData::Bool(b),
      &const_val::ConstVal::Char(c) =>
        ConstValData::Char(c),
      &const_val::ConstVal::Variant(v) =>
        ConstValData::Variant(v.into()),
      &const_val::ConstVal::Function(d, substs) => {
        let span = self.tcx().def_span(d);
        self.tcx().sess
          .span_note_without_error(span,
                                   "ConstVal::Function here");
        let subs = self.monomorphize(&substs);
        let resolved = monomorphize::resolve(self.tcx(),
                                             d, &subs);
        let f = self.enter_function(resolved, |mut nested_fn| {
          match resolved.def {
            InstanceDef::Item(_) => {
              nested_fn.build();
            },
            InstanceDef::Intrinsic(idef_id) => {
              let name = (&nested_fn.tcx().item_name(idef_id)[..])
                .to_string();
              let ty = nested_fn.tcx().type_of(idef_id);
              let sig = ty.fn_sig(nested_fn.tcx());
              let sig = nested_fn.tcx()
                .erase_late_bound_regions_and_normalize(&sig);
              let sig = sig.inputs_and_output
                .iter()
                .map(|ty| nested_fn.convert(ty))
                .collect();
              let data =
                super::FunctionData::Intrinsic(name, sig);

              let id = nested_fn.func;
              nested_fn.module()
                .mod_
                .funcs[id] = data;
            },
            _ => {
              bug!("TODO: function instance: {:?}", resolved);
            },
          }
        });
        ConstValData::Function(f)
      },
      &const_val::ConstVal::Aggregate(ConstAggregate::Struct(map)) => {
        let v: Vec<_> = map.iter()
          .map(|&(ref name, val)| {
            (name.to_string(),
             val.val.convert(self))
          })
          .collect();

        ConstValData::Struct(v)
      },
      &const_val::ConstVal::Aggregate(ConstAggregate::Tuple(t)) => {
        let v = t.iter()
          .map(|v| v.val.convert(self) )
          .collect();
        ConstValData::Tuple(v)
      },
      &const_val::ConstVal::Aggregate(ConstAggregate::Array(a)) => {
        let v = a.iter()
          .map(|v| v.val.convert(self) )
          .collect();
        ConstValData::Array(v)
      },
      &const_val::ConstVal::Aggregate(ConstAggregate::Repeat(r, count)) => {
        ConstValData::Repeat(r.val.convert(self), count)
      },
      &const_val::ConstVal::Unevaluated(..) => unimplemented!(),
    };

    let id = self.module().mod_.const_vals
      .push(data);
    self.module().const_vals.insert(val.clone(),
                                    id);
    id
  }
  pub fn convert_subst(&mut self,
                       subs: &'tcx subst::Substs<'tcx>) -> super::Substs {
    use rustc::ty::RegionKind;

    if let Some(&id) = self.module_ref().substs.get(subs) {
      return id;
    }

    let kinds: Vec<_> = subs.iter()
      .map(|kind| {
        if let Some(ty) = kind.as_type() {
          return super::TsKind::Type(self.convert_ty(ty));
        }

        if let Some(&RegionKind::ReErased) = kind.as_region() {
          return super::TsKind::Region(super::RegionKind::Erased)
        }

        unreachable!();
      })
      .collect();

    let id = self.module().mod_.substs
      .push(kinds);
    self.module().substs.insert(subs, id);
    id
  }

  pub fn convert_lvalue(&mut self, lvalue: &mir::Lvalue<'tcx>) ->  super::Lvalue {
    use super::Lvalue;
    match lvalue {
      &mir::Lvalue::Local(l) => Lvalue::Local(l.into()),
      &mir::Lvalue::Static(ref s) => Lvalue::Static(s.convert(self)),
      &mir::Lvalue::Projection(ref proj) => {
        let proj = self.convert_projection(&**proj);
        Lvalue::Projection(Box::new(proj))
      }
    }
  }
  pub fn convert_static(&mut self, statik: &mir::Static<'tcx>) -> super::Static {
    super::Static {
      def_id: statik.def_id.into(),
      ty: self.convert(&statik.ty),
    }
  }
  pub fn convert_projection<B, V, TT>(&mut self,
                                     proj: &mir::Projection<'tcx, B, V, TT>)
    -> super::Projection<B::Target, V::Target, TT::Target>
  where B: RustCConvert<'tcx>,
        V: RustCConvert<'tcx>,
        TT: RustCConvert<'tcx> + TransNormalize<'tcx>,
  {
    super::Projection {
      base: proj.base.convert(self),
      elem: proj.elem.convert(self),
    }
  }
  pub fn convert_projection_elem<V, TT>(&mut self,
                                        elem: &mir::ProjectionElem<'tcx, V, TT>)
    -> super::ProjectionElem<V::Target, TT::Target>
    where V: RustCConvert<'tcx>,
          TT: RustCConvert<'tcx> + TransNormalize<'tcx>,
  {
    use super::ProjectionElem;
    match elem {
      &mir::ProjectionElem::Deref => ProjectionElem::Deref,
      &mir::ProjectionElem::Field(field, ref ty) => {
        let ty = self.monomorphize(ty);
        ProjectionElem::Field(field.into(),
                              ty.convert(self))
      },
      &mir::ProjectionElem::Index(ref v) => {
        ProjectionElem::Index(v.convert(self))
      },
      &mir::ProjectionElem::ConstantIndex {
        offset,
        min_length,
        from_end,
      } => ProjectionElem::ConstantIdx {
        offset,
        min_length,
        from_end,
      },
      &mir::ProjectionElem::Subslice {
        from, to,
      } => ProjectionElem::Subslice {
        from, to,
      },
      &mir::ProjectionElem::Downcast(adt, disc) => {
        let subs = self.param_substs();
        let id = self.convert_adt_def(adt, subs);
        ProjectionElem::Downcast(id, disc)
      },
    }
  }
}

pub trait RustCConvert<'tcx> {
  type Target;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut;
}
impl<'tcx> RustCConvert<'tcx> for mir::Lvalue<'tcx> {
  type Target = super::Lvalue;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
        Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    builder.convert_lvalue(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Operand<'tcx> {
  type Target = super::Operand;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    builder.convert_operand(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for &'tcx TyS<'tcx> {
  type Target = Ty;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    builder.convert_ty(self)
  }
}

impl<'tcx, B, V, T> RustCConvert<'tcx> for mir::Projection<'tcx, B, V, T>
  where B: RustCConvert<'tcx>,
        V: RustCConvert<'tcx>,
        T: RustCConvert<'tcx> + TransNormalize<'tcx>,
{
  type Target = super::Projection<B::Target, V::Target, T::Target>;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    builder.convert_projection(self)
  }
}
impl<'tcx, V, T> RustCConvert<'tcx> for mir::ProjectionElem<'tcx, V, T>
  where V: RustCConvert<'tcx>,
        T: RustCConvert<'tcx> + TransNormalize<'tcx>,
{
  type Target = super::ProjectionElem<V::Target, T::Target>;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    builder.convert_projection_elem(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::AggregateKind<'tcx> {
  type Target = super::AggregateKind;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    builder.convert_aggregate_kind(None, self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Rvalue<'tcx> {
  type Target = super::Rvalue;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    builder.convert_rvalue(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for const_val::ConstVal<'tcx> {
  type Target = super::ConstVal;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    builder.convert_constval(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Constant<'tcx> {
  type Target = super::Constant;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    builder.convert_constant(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Literal<'tcx> {
  type Target = super::Literal;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    builder.convert_literal(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Static<'tcx> {
  type Target = super::Static;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    builder.convert_static(self)
  }
}
impl<'tcx> RustCConvert<'tcx> for &'tcx subst::Substs<'tcx> {
  type Target = super::Substs;
  fn convert<'parent, 'a, FID>(&self,
                               builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    builder.convert_subst(*self)
  }
}
impl<'tcx> RustCConvert<'tcx> for mir::Local {
  type Target = super::Local;
  fn convert<'parent, 'a, FID>(&self,
                               _builder: &mut Function<'parent, 'a, 'tcx, FID>)
    -> Self::Target
    where 'tcx: 'parent,
          FID: IdxFunction,
          Function<'parent, 'a, 'tcx, FID>: Deref<Target = super::FunctionKind> + DerefMut,
  {
    use indexvec::Idx;
    super::Local::new(self.index())
  }
}

impl<'a, 'tcx> Deref for Module<'a, 'tcx> {
  type Target = super::Module;
  fn deref(&self) -> &Self::Target {
    &self.mod_
  }
}
impl<'a, 'tcx> DerefMut for Module<'a, 'tcx> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.mod_
  }
}

impl<'a, 'tcx, 'b> HasDataLayout for &'b Module<'a, 'tcx>
  where 'tcx: 'a + 'b,
        'a: 'b,
{
  fn data_layout(&self) -> &TargetDataLayout { &self.tcx.data_layout }
}

impl<'a, 'tcx, 'b> LayoutTyper<'tcx> for &'b Module<'a, 'tcx>
  where 'tcx: 'a + 'b,
        'a: 'b,
{
  type TyLayout = TyLayout<'tcx>;

  fn tcx<'c>(&'c self) -> TyCtxt<'c, 'tcx, 'tcx> {
    self.tcx
  }

  fn layout_of(self, ty: ty::Ty<'tcx>) -> Self::TyLayout {
    use rustc::traits;
    use rustc::ty::layout::LayoutError;
    let param_env = ty::ParamEnv::empty(traits::Reveal::All);
    LayoutCx::new(self.tcx, param_env)
      .layout_of(ty)
      .unwrap_or_else(|e| match e {
        LayoutError::SizeOverflow(_) => self
          .tcx()
          .sess
          .fatal(&e.to_string()),
        _ => bug!("failed to get layout for `{}`: {}", ty, e)
      })
  }

  fn normalize_projections(self, ty: ty::Ty<'tcx>) -> ty::Ty<'tcx> {
    self.tcx().normalize_associated_type(&ty)
  }
}

pub trait ModuleCtxt<'a, 'tcx>
  where 'tcx: 'a,
{
  fn mod_tcx(&self) -> TyCtxt<'a, 'tcx, 'tcx>;
  fn module(&mut self) -> &mut Module<'a, 'tcx>;
  fn module_ref(&self) -> &Module<'a, 'tcx>;

  fn param_substs(&self) -> &'tcx subst::Substs<'tcx>;
}
impl<'a, 'tcx> ModuleCtxt<'a, 'tcx> for Module<'a, 'tcx> {
  fn mod_tcx(&self) -> TyCtxt<'a, 'tcx, 'tcx> { self.tcx }
  fn module(&mut self) -> &mut Module<'a, 'tcx> { self }
  fn module_ref(&self) -> &Module<'a, 'tcx> { self }
  fn param_substs(&self) -> &'tcx subst::Substs<'tcx> { self.extern_substs }
}
impl<'parent, 'a, 'tcx, FIdx> ModuleCtxt<'a, 'tcx> for Function<'parent, 'a, 'tcx, FIdx>
{
  fn mod_tcx(&self) -> TyCtxt<'a, 'tcx, 'tcx> { self.parent.mod_tcx() }
  fn module(&mut self) -> &mut Module<'a, 'tcx> { self.parent.module() }
  fn module_ref(&self) -> &Module<'a, 'tcx> { self.parent.module_ref() }

  fn param_substs(&self) -> &'tcx subst::Substs<'tcx> { self.substs }
}

pub trait ExtraModuleCtxt<'a, 'tcx> {
  fn monomorphize<T>(&self, v: &T) -> T
    where T: TransNormalize<'tcx>;
  fn enter_function<'s, F>(&'s mut self,
                           instance: ty::Instance<'tcx>,
                           f: F) -> super::Function
    where F: FnOnce(Function<'s, 'a, 'tcx, super::Function>),
          'tcx: 's + 'a,
          'a: 's;

  fn enqueue_function(&mut self, instance: Instance<'tcx>) -> super::Function;
}
impl<'a, 'tcx, TT> ExtraModuleCtxt<'a, 'tcx> for TT
  where TT: ModuleCtxt<'a, 'tcx>,
        'tcx: 'a,
{
  fn monomorphize<T>(&self, v: &T) -> T
    where T: TransNormalize<'tcx>,
  {
    self.module_ref().tcx
      .trans_apply_param_substs(self.param_substs(), v)
  }

  fn enter_function<'s, F>(&'s mut self,
                           instance: ty::Instance<'tcx>,
                           f: F) -> super::Function
    where F: FnOnce(Function<'s, 'a, 'tcx, super::Function>),
          'tcx: 's + 'a,
          'a: 's,
  {
    use std::collections::hash_map::Entry;
    let id = {
      let tcx = self.module_ref().tcx;
      let m = self.module();

      match m.funcs.entry(instance) {
        Entry::Vacant(v) => {
          let f = super::FunctionData::new();

          let (f, is_stub) = match instance {
            Instance {
              def: InstanceDef::Item(ref did),
              ..
            } if tcx.absolute_item_path_str(*did) == "std::panicking::begin_panic" => {
              (super::FunctionData::Intrinsic("abort".into(), vec![]), true)
            },
            _ => (super::FunctionData::new(), false),
          };
          let id = m.mod_.funcs
            .push(f);
          v.insert(id);

          if is_stub { return id; }

          id
        },
        Entry::Occupied(o) => {
          return *o.get();
        },
      }
    };

    let fun = Function {
      parent: self,
      mir: None,
      func: id,
      substs: instance.substs,
      def: Some(instance),
    };
    f(fun);

    id
  }
  fn enqueue_function(&mut self,
                      instance: ty::Instance<'tcx>) -> super::Function
  {
    use std::collections::hash_map::Entry;
    let id = {
      let m = self.module();

      match m.funcs.entry(instance) {
        Entry::Vacant(v) => {
          let f = super::FunctionData::new();
          let id = m.mod_.funcs
            .push(f);
          m.queue.push((instance, id));
          v.insert(id);
          id
        },
        Entry::Occupied(o) => {
          return *o.get();
        },
      }
    };

    id
  }
}

pub struct Function<'parent, 'a, 'tcx, FIdx>
  where 'tcx: 'parent + 'a,
        'a: 'parent,
{
  parent: &'parent mut ModuleCtxt<'a, 'tcx>,
  mir: Option<&'a mir::Mir<'tcx>>,
  func: FIdx,
  substs: &'tcx subst::Substs<'tcx>,
  def: Option<ty::Instance<'tcx>>,
}

pub trait IdxFunction: Clone {
  fn function_idx(&self) -> super::Function;

  type Target;
  fn deref<'parent, 'a, 'tcx>(&self, m: &'parent ModuleCtxt<'a, 'tcx>)
    -> &'parent Self::Target
    where 'tcx: 'parent + 'a,
          'a: 'parent;
  fn deref_mut<'parent, 'a, 'tcx>(&self, m: &'parent mut ModuleCtxt<'a, 'tcx>)
    -> &'parent mut Self::Target
    where 'tcx: 'parent + 'a,
          'a: 'parent;
}
impl IdxFunction for super::Function {
  fn function_idx(&self) -> super::Function { *self }

  type Target = super::FunctionKind;
  fn deref<'parent, 'a, 'tcx>(&self, m: &'parent ModuleCtxt<'a, 'tcx>)
    -> &'parent Self::Target
    where 'tcx: 'parent + 'a,
          'a: 'parent
  {
    let m = m.module_ref();
    m.mod_.funcs[*self].fkr()
  }
  fn deref_mut<'parent, 'a, 'tcx>(&self, m: &'parent mut ModuleCtxt<'a, 'tcx>)
    -> &'parent mut Self::Target
    where 'tcx: 'parent + 'a,
          'a: 'parent
  {
    let m = m.module();
    m.mod_.funcs[*self].fkm()
  }
}
impl IdxFunction for (super::Function, super::Promoted) {
  fn function_idx(&self) -> super::Function { self.0 }

  type Target = super::FunctionKind;
  fn deref<'parent, 'a, 'tcx>(&self, m: &'parent ModuleCtxt<'a, 'tcx>)
    -> &'parent Self::Target
    where 'tcx: 'parent + 'a,
          'a: 'parent
  {
    let m = m.module_ref();
    m.mod_.funcs[self.0].fkr()
      .promoted[self.1].fkr()
  }
  fn deref_mut<'parent, 'a, 'tcx>(&self, m: &'parent mut ModuleCtxt<'a, 'tcx>)
    -> &'parent mut Self::Target
    where 'tcx: 'parent + 'a,
          'a: 'parent
  {
    let m = m.module();
    m.mod_.funcs[self.0].fkm()
      .promoted[self.1].fkm()
  }
}
impl<'parent, 'a, 'tcx, FIdx> Deref for Function<'parent, 'a, 'tcx, FIdx>
  where FIdx: IdxFunction,
{
  type Target = FIdx::Target;
  fn deref(&self) -> &Self::Target {
    let id = self.func.clone();
    id.deref(self)
  }
}
impl<'parent, 'a, 'tcx, FIdx> DerefMut for Function<'parent, 'a, 'tcx, FIdx>
  where FIdx: IdxFunction,
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    let id = self.func.clone();
    id.deref_mut(self)
  }
}
