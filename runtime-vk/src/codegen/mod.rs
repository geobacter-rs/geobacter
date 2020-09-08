use std::env::var_os;
use std::fs::File;
use std::geobacter::platform::{Platform, spirv};
use std::io::{Write, Read};
use std::num::NonZeroU32;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use tracing::*;

use grt_core::AcceleratorTargetDesc;
use grt_core::codegen::*;
use grt_core::codegen::help::LlvmBuildRoot;
use grt_core::codegen::products::*;

use rustc_data_structures::sync::Lrc;
use rustc_geobacter::intrinsics::IntrinsicName;
use rustc_geobacter::intrinsics::platform::PlatformIntrinsic;
use rustc_geobacter::intrinsics::arch::spirv::SpirvSuicide;
use rustc_hir::def_id::DefId;
use rustc_hir::lang_items::*;
use rustc_middle::middle::codegen_fn_attrs::*;
use rustc_middle::mir::*;
use rustc_middle::ty::*;
use rustc_middle::ty::layout::{LayoutCx, TyAndLayout};
use rustc_target::abi::{FieldsShape, Size};
use rustc_target::spec::*;

use crate::error::Error;
use crate::module::*;

pub mod attrs;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CodegenShaderInterface {
  pub(crate) input: StaticShaderInterfaceDef,
  pub(crate) output: StaticShaderInterfaceDef,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VkEntryDesc {
  pub exe_model: spirv::ExeModel,
  pub workgroup_size: Option<(NonZeroU32, NonZeroU32, NonZeroU32)>,
  pub pipeline: StaticPipelineLayoutDesc,
  pub interface: Option<CodegenShaderInterface>,
}

impl PlatformCodegenDesc for VkEntryDesc {}

impl PlatformKernelDesc for VkEntryDesc {}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VkPlatformCodegen;

impl PlatformCodegen for VkPlatformCodegen {
  type Device = super::VkAccel;
  type KernelDesc = VkEntryDesc;
  type CodegenDesc = VkEntryDesc;
  type Condition = attrs::Condition;

  fn insert_intrinsics<F>(&self, _: &Arc<AcceleratorTargetDesc>,
                          into: &mut F)
    where F: for<'a> FnMut(&'a str, Lrc<dyn CustomIntrinsicMirGen>),
  {
    let suicide = SpirvSuicide::default();
    into(SpirvSuicide::NAME, Lrc::new(suicide));
  }
  fn insert_kernel_intrinsics<F>(&self, kernel: &PKernelDesc<Self>,
                                 into: &mut F)
    where F: for<'a> FnMut(&'a str, Lrc<dyn CustomIntrinsicMirGen>)
  {
    let platform = Platform::Vulkan(kernel.platform_desc.exe_model);
    let platform = PlatformIntrinsic(platform);
    into(PlatformIntrinsic::NAME, Lrc::new(platform));
  }

  fn root<'tcx>(&self, desc: PKernelDesc<Self>,
                instance: Instance<'tcx>,
                _tcx: TyCtxt<'tcx>,
                _dd: &DriverData<'tcx, Self>)
    -> Result<PCodegenDesc<'tcx, Self>, Error>
  {
    Ok(CodegenDesc {
      instance,
      kernel_instance: desc.instance.into(),
      spec_params: desc.spec_params,
      platform_desc: desc.platform_desc,
    })
  }
  fn root_conditions<'tcx>(&self, _root: &PCodegenDesc<Self>,
                           _tcx: TyCtxt<'tcx>,
                           _dd: &DriverData<'tcx, Self>)
    -> Result<Vec<Self::Condition>, Error>
  {
    Ok(vec![attrs::Condition::Platform])
  }
  fn pre_codegen<'tcx>(&self, _tcx: TyCtxt<'tcx>,
                       _dd: &DriverData<'tcx, Self>)
    -> Result<(), Error>
  {
    Ok(())
  }
  fn post_codegen(&self,
                  target_desc: &Arc<AcceleratorTargetDesc>,
                  tdir: &Path,
                  codegen: &mut PCodegenResults<Self>)
    -> Result<(), Error>
  {
    // We don't need to do any sort of linking for SPIRV.
    let exe = if let Some(obj) = codegen.take_object() {
      obj
    } else {
      // fallback to invoking llc manually:

      let bc = codegen.take_bitcode()
        .expect("no object output");

      let linked_bc = tdir.join("linked.bc");
      {
        let mut out = File::create(&linked_bc)?;
        out.write_all(&bc)?;
      }

      // be helpful if this var isn't set:
      if var_os("RUST_BUILD_ROOT").is_none() &&
        var_os("LLVM_BUILD").is_none() {
        println!("Due to some required LLVM patches, I need a build of \
                 Geobacter's LLVM");
        println!("You shouldn't need to build this separately; it should \
                 have been built with Rust");
        println!("Set either LLVM_BUILD or RUST_BUILD_ROOT \
                 (RUST_BUILD_ROOT takes priority)");
        println!("XXX Temporary");
      }

      let llvm = LlvmBuildRoot::default();
      let llc = llvm.llc();
      let llc_cmd = || {
        let mut cmd = Command::new(&llc);
        cmd.current_dir(tdir)
          .arg(&linked_bc)
          .arg(format!("-mcpu={}", target_desc.target.options.cpu))
          .arg(format!("-mattr={}", target_desc.target.options.features))
          .arg("-O3");
        cmd
      };

      let obj = tdir.join("codegen.o");

      let mut llc = llc_cmd();
      llc.arg("-filetype=obj")
        .arg("-o").arg(&obj);
      run_cmd(llc)?;

      info!("finished running llc");

      // run llc again, but write asm this time
      let mut llc = llc_cmd();

      llc.arg("-filetype=asm")
        .arg("-o").arg(tdir.join("codegen.s"));

      run_cmd(llc)?;

      let mut b = Vec::new();
      File::open(&obj)?.read_to_end(&mut b)?;
      b
    };

    codegen.put_exe(exe);

    Ok(())
  }
  /// Modify the provided `attrs` to suit the platforms needs.
  /// `attrs` is generated by vanilla Rustc.
  fn codegen_fn_attrs<'tcx>(&self,
                            tcx: TyCtxt<'tcx>,
                            dd: &DriverData<'tcx, Self>,
                            id: DefId,
                            attrs: &mut CodegenFnAttrs)
  {
    if let Some(desc) = dd.root_desc(id) {
      let mut exe_modes = vec![];
      if let Some((x, y, z)) = desc.platform_desc.workgroup_size {
        let dims = vec![x.get() as u64, y.get() as _, z.get() as _];
        let mode = ("LocalSize".into(), dims);
        exe_modes.push(mode);
      }

      attrs.spirv = Some(SpirVAttrs {
        storage_class: None,
        metadata: None,
        exe_model: Some(format!("{:?}", desc.platform_desc.exe_model)),
        exe_mode: if exe_modes.len() == 0 {
          None
        } else {
          Some(exe_modes)
        },
        pipeline_binding: None,
        pipeline_descriptor_set: None,
      });
    } else if tcx.is_static(id) {
      let lang_items = LangItems::new(tcx);

      let inst = Instance::mono(tcx, id);
      let ty = inst.ty(tcx, ParamEnv::reveal_all());
      // check for function types. We don't attach any spirv
      // metadata to functions.
      let root = match ty.kind() {
        Adt(def, _) => lang_items.get_item(def.did),
        _ => None,
      };

      if root.is_none() {
        warn!("static {:?} is ordinary; not creating SPIRV metadata", inst);
        attrs.spirv = None;
        return;
      }
      let root = root.unwrap();

      let spirv_attrs = Self::build_spirv_metadata(tcx, &lang_items, inst);

      // set the address space:
      if attrs.addr_space.is_none() {
        attrs.addr_space = Some(root.addr_space());
      }

      attrs.spirv = Some(spirv_attrs);
    }
  }
}

fn default_node() -> SpirVAttrNode {
  SpirVAttrNode {
    type_spec: default_ty_spec(),
    decorations: vec![],
  }
}
fn default_ty_spec() -> SpirVTypeSpec {
  SpirVTypeSpec::Struct(vec![])
}

enum SpirvLangItemStorageClass {
  BuiltinInput,
  BuiltinOutput,
  ShaderInput,
  ShaderOutput,
  Buffer,
  Uniform,
}

impl SpirvLangItemStorageClass {
  #[inline]
  fn addr_space(&self) -> AddrSpaceIdx {
    match self {
      SpirvLangItemStorageClass::BuiltinInput |
      SpirvLangItemStorageClass::ShaderInput => {
        AddrSpaceIdx(5)
      }
      SpirvLangItemStorageClass::BuiltinOutput |
      SpirvLangItemStorageClass::ShaderOutput => {
        AddrSpaceIdx(6)
      }
      SpirvLangItemStorageClass::Uniform => {
        AddrSpaceIdx(7)
      }
      SpirvLangItemStorageClass::Buffer => {
        AddrSpaceIdx(8)
      }
    }
  }
}

#[derive(Debug)]
struct LangItems {
  // used only internally within other marker types.
  maybe_uninit: DefId,
  unsafe_cell: DefId,
  builtin: DefId,

  builtin_input: DefId,
  builtin_output: DefId,
  shader_input: DefId,
  shader_output: DefId,
  buffer: DefId,
  uniform: DefId,
}

impl LangItems {
  fn new(tcx: TyCtxt) -> Self {
    LangItems {
      maybe_uninit: tcx.require_lang_item(LangItem::MaybeUninit, None),
      unsafe_cell: tcx.require_lang_item(LangItem::UnsafeCell, None),
      builtin: tcx.require_lang_item(LangItem::SpirvBuiltin, None),

      builtin_input: tcx.require_lang_item(LangItem::SpirvInput, None),
      builtin_output: tcx.require_lang_item(LangItem::SpirvOutput, None),
      shader_input: tcx.require_lang_item(LangItem::SpirvShaderInput, None),
      shader_output: tcx.require_lang_item(LangItem::SpirvShaderOutput, None),
      buffer: tcx.require_lang_item(LangItem::SpirvBufferObject, None),
      uniform: tcx.require_lang_item(LangItem::SpirvUniformObject, None),
    }
  }
  fn get_item(&self, did: DefId) -> Option<SpirvLangItemStorageClass> {
    if self.builtin_input == did {
      return Some(SpirvLangItemStorageClass::BuiltinInput);
    }
    if self.builtin_output == did {
      return Some(SpirvLangItemStorageClass::BuiltinOutput);
    }
    if self.shader_input == did {
      return Some(SpirvLangItemStorageClass::ShaderInput);
    }
    if self.shader_output == did {
      return Some(SpirvLangItemStorageClass::ShaderOutput);
    }
    if self.buffer == did {
      return Some(SpirvLangItemStorageClass::Buffer);
    }
    if self.uniform == did {
      return Some(SpirvLangItemStorageClass::Uniform);
    }

    None
  }
}

impl VkPlatformCodegen {
  fn build_spirv_ty_metadata<'tcx>(tcx: TyCtxt<'tcx>,
                                   lang_items: &LangItems,
                                   inst: Instance<'tcx>,
                                   layout: TyAndLayout<'tcx>)
    -> SpirVAttrNode
  {
    info!("build_spirv_ty_metadata: ty.kind = {:?}", layout.ty.kind());

    let reveal_all = ParamEnv::reveal_all();
    let lcx = LayoutCx { tcx, param_env: reveal_all, };

    let node = match *layout.ty.kind() {
      Bool | Char | Int(_) | Uint(_) | Float(_) => {
        default_node()
      }
      Adt(adt_def, _substs) if adt_def.repr.simd() => {
        // TODO someday we'll want to allow `RowMajor` and `ColMajor` etc
        default_node()
      }
      Adt(adt_def, _) if adt_def.repr.transparent() => {
        // extract the inner type which this type wraps. This can be important
        // for wrappers which wrap SIMD types, for example SPIRV Vector and Matrix
        // types.
        let field_ty = layout.field(&lcx, 0).unwrap();

        warn!("repr(transparent): extracted {:#?}", field_ty);
        Self::build_spirv_ty_metadata(tcx, lang_items,
                                      inst, field_ty)
      }
      Tuple(_) | Adt(..) => {
        // TODO: this is not complete, or even correct in all cases. Need to fix upstream things
        //  first.
        let with_padded_indices = move |count: usize,
                                        iter: &mut dyn Iterator<Item = (Size, u32)>| {
          let mut nodes: Vec<SpirVStructMember> = vec![];
          nodes.reserve(count);
          for (field_idx, (offset, padded_index)) in iter.enumerate() {
            for _ in field_idx..(padded_index as usize) {
              // TODO: offsets here
              nodes.push(SpirVStructMember {
                node: default_node(),
                decorations: vec![],
              });
            }

            let field = layout.field(&lcx, field_idx).unwrap();
            let mut node = SpirVStructMember {
              node: Self::build_spirv_ty_metadata(tcx, lang_items,
                                                  inst, field),
              decorations: vec![],
            };
            /*let offset_decor = || {
              let mut field = field;
              loop {
                match field.ty {
                  Adt(def, ..) if def.repr.transparent() => {
                    field = layout.field(&lcx, 0).unwrap();
                    continue;
                  },
                  Adt(def, ..) if def.repr.simd() => return true,
                  // nested ADTs will already offset
                  Tuple(_) | Adt(..) | Array(..) => return false,
                  _ => return true,
                }
              }
            };*/
            node.decorations.push(("Offset".into(), vec![offset.bytes() as _]));
            nodes.push(node);
          }
          // XXX trailing padding
          nodes
        };
        let nodes = match layout.fields {
          FieldsShape::Arbitrary {
            padded_indices: Some((_count, ref indices)),
            ref offsets, ..
          } => {
            let _iter = offsets.iter().cloned().zip(indices.iter().cloned());
            unimplemented!();
            //with_padded_indices(count as usize, &mut iter)
          }
          FieldsShape::Arbitrary { padded_indices: None, ref offsets, .. } => {
            let count = offsets.len();
            with_padded_indices(count, &mut offsets.iter().cloned().zip(0..count as u32))
          }
          _ => unreachable!("{:#?}", layout),
        };

        SpirVAttrNode {
          type_spec: SpirVTypeSpec::Struct(nodes),
          decorations: vec![],
        }
      }
      Array(..) => {
        let inner = layout.field(&lcx, 0).unwrap();
        let ts = Self::build_spirv_ty_metadata(tcx, lang_items,
                                                   inst, inner);
        let stride = match layout.fields {
          FieldsShape::Array { ref stride, .. } => stride.bytes(),
          _ => unreachable!("{:#?}", layout),
        };
        let type_spec = SpirVTypeSpec::Array(Box::new(ts));

        SpirVAttrNode {
          type_spec,
          decorations: vec![("ArrayStride".into(), vec![stride as _])],
        }
      }

      _ => panic!("shouldn't be allowed or unimplemented TODO: {:?}", layout.ty),
    };

    node
  }

  fn build_spirv_metadata<'tcx>(tcx: TyCtxt<'tcx>,
                                lang_items: &LangItems,
                                inst: Instance<'tcx>)
    -> SpirVAttrs
  {
    let reveal_all = ParamEnv::reveal_all();
    let ty = inst.ty(tcx, reveal_all);
    let layout = tcx.layout_of(reveal_all.and(ty))
      .unwrap();
    let mut node = Self::build_spirv_ty_metadata(tcx, lang_items,
                                             inst, layout);

    let mut attrs = SpirVAttrs::default();

    // The following things are only permissible on top level statics/globals
    match *layout.ty.kind() {
      Adt(adt_def, substs) if adt_def.did == lang_items.builtin => {
        // This is the `RawBuiltin` structure. We need to extract the `ID` constant
        // param and pass that to the builtin metadata.
        let substs = tcx
          .subst_and_normalize_erasing_regions(inst.substs, reveal_all, &substs);
        let id = substs.consts().next().expect("expecting at least one const param");
        let id = id.eval_bits(tcx, reveal_all, tcx.types.u32) as u32;

        node.decorations.push(("BuiltIn".into(), vec![id as _]));
      }
      Adt(adt_def, substs)
      if adt_def.did == lang_items.uniform
        || adt_def.did == lang_items.buffer
      => {
        // This is one of the pipeline objects. We need to extract the `SET` and `BINDING`
        // constants and pass those to the codegen backend.
        let substs = tcx
          .subst_and_normalize_erasing_regions(inst.substs, reveal_all, &substs);
        let mut consts = substs.consts()
          .map(|c| {
            c.eval_bits(tcx, reveal_all, tcx.types.u32) as u32
          });

        let set = consts.next().expect("expected constant param; got none");
        let binding = consts.next().expect("expected constant param; got none");

        attrs.pipeline_binding = Some(binding);
        attrs.pipeline_descriptor_set = Some(set);

        // These also need the block decoration:
        node.decorations.push(("Block".into(), vec![]));
      }
      _ => {}
    }

    attrs.metadata = Some(node);

    attrs
  }
}

pub fn run_cmd(mut cmd: Command) -> Result<(), Error> {
  info!("running command {:?}", cmd);
  let mut child = cmd.spawn()?;
  if !child.wait()?.success() {
    Err(Error::Cmd(format!("command failed: {:?}", cmd).into()))
  } else {
    Ok(())
  }
}
