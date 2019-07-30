
use std::collections::HashMap;
use std::error::Error;
use std::env::var_os;
use std::fs::{File, };
use std::io::{Write, Read, stderr, };
use std::path::Path;
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;

use crate::log::{info, };

use crate::rustc::ty::{TyCtxt, Instance, };

use amd_comgr::{set::DataSet, data::RelocatableData,
                data::Data, action::*, };

use hsa_core::platform::hsa::AmdGpu;
use hsa_core::platform::{hsa, Platform};

use crate::lrt_core::{AcceleratorTargetDesc, };
use crate::lrt_core::codegen as core_codegen;
use crate::lrt_core::codegen::*;
use crate::lrt_core::codegen::help::LlvmBuildRoot;
use crate::lrt_core::codegen::products::*;

use crate::intrinsics::*;
use crate::intrinsics_common::CurrentPlatform;

use crate::serde::{Serialize, Deserialize, };
use crate::rmps::decode::{from_slice as rmps_from_slice, };

use crate::{HsaAmdGpuAccel, HsaAmdTargetDescHelper, };

#[derive(Clone, Copy, Debug, Default)]
pub struct Codegenner;

impl PlatformCodegen for Codegenner {
  type Device = HsaAmdGpuAccel;
  type KernelDesc = KernelDesc;
  type CodegenDesc = CodegenDesc;
  type Condition = attrs::Condition;

  fn insert_intrinsics<T>(&self,
                          target_desc: &Arc<AcceleratorTargetDesc>,
                          into: &mut T)
    where T: PlatformIntrinsicInsert,
  {
    let gpu = &target_desc.target.options.cpu;
    // can't fail; the device ctor also checks this
    let gpu = AmdGpu::from_str(gpu).unwrap();
    let platform = Platform::Hsa(hsa::Device::AmdGcn(gpu));
    into.insert(CurrentPlatform(platform));

    for intr in AxisId::permutations() {
      into.insert(intr);
    }
    into.insert(DispatchPtr);
    let i: intrinsics_common::WorkItemKill<intrinsics::AmdGcnKillDetail> = Default::default();
    into.insert(i);
  }

  fn root<'tcx>(&self, desc: PKernelDesc<Self>,
                instance: Instance<'tcx>,
                _tcx: TyCtxt<'tcx>,
                _dd: &DriverData<'tcx, Self>)
    -> Result<PCodegenDesc<'tcx, Self>, Box<dyn Error + Send + Sync + 'static>>
  {
    Ok(core_codegen::CodegenDesc {
      instance,
      kernel_instance: desc.instance,
      // filled post-codegen
      platform_desc: Default::default(),
    })
  }

  fn root_conditions<'tcx>(&self,
                           _root: &PCodegenDesc<'tcx, Self>,
                           _tcx: TyCtxt<'tcx>,
                           _dd: &DriverData<'tcx, Self>)
    -> Result<Vec<Self::Condition>, Box<dyn Error + Send + Sync + 'static>>
  {
    Ok(vec![])
  }

  fn pre_codegen<'tcx>(&self,
                       _desc: &PCodegenDesc<'tcx, Self>,
                       _tcx: TyCtxt<'tcx>,
                       _dd: &DriverData<'tcx, Self>)
    -> Result<(), Box<dyn Error + Send + Sync + 'static>>
  {
    Ok(())
  }

  fn post_codegen(&self,
                  target_desc: &Arc<AcceleratorTargetDesc>,
                  tdir: &Path,
                  codegen: &mut PCodegenResults<Self>)
    -> Result<(), Box<dyn Error + Send + Sync + 'static>>
  {
    use goblin::Object;

    // add the .kd suffix:
    for entry in codegen.entries.iter_mut() {
      entry.symbol.push_str(".kd");
    }

    let obj_data = if let Some(obj) = codegen.take_object() {
      obj
    } else {
      // fallback to invoking llc manually:
      // TODO send LLVM patches upstream so that amd-comgr is useful for this.

      let bc = codegen.take_bitcode()
        .ok_or("no object output")?;

      let linked_bc = tdir.join("linked.bc");
      {
        let mut out = File::create(&linked_bc)?;
        out.write_all(&bc)?;
      }

      // be helpful if this var isn't set:
      if var_os("RUST_BUILD_ROOT").is_none() &&
        var_os("LLVM_BUILD").is_none() {
        println!("Due to some required LLVM patches, I need a build of \
                Legionella's LLVM");
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
          .arg("-relocation-model=pic");
        cmd
      };

      let obj = tdir.join("obj.o");

      let mut llc = llc_cmd();
      llc.arg("-filetype=obj")
        .arg("-o").arg(&obj);
      run_cmd(llc)?;

      info!("finished running llc");

      if log::log_enabled!(log::Level::Debug) {
        // run llc again, but write asm this time
        let mut llc = llc_cmd();

        llc.arg("-filetype=asm")
          .arg("-o").arg(tdir.join("obj.S"));

        run_cmd(llc)?;
      }

      let mut b = Vec::new();
      File::open(&obj)?.read_to_end(&mut b)?;
      b
    };

    // link using amd-comgr:
    let mut data = RelocatableData::new()?;
    data.set_data(&obj_data)?;
    data.set_name("obj-for-linking.o".into())?;
    let mut set = DataSet::new()?;
    set.add_data(&data)?;

    let mut action = Action {
      kind: ActionKind::LinkRelocToExe,
      info: ActionInfo::new()?,
    };
    action.set_working_path(Some(tdir.into()))?;
    action.set_logging(true)?;
    // amd-comgr requires an isa, even for linking:
    action.set_isa_name(Some(target_desc.isa_name()))?;
    let mut out_set = DataSet::new()?;
    match set.perform_into(&action, &mut out_set) {
      Ok(_) => { },
      Err(err) => {
        if out_set.logs_len()? > 0 {
          let stderr = stderr();
          let mut stderr = stderr.lock();
          writeln!(stderr, "Link error; logs follow:").expect("stderr write");
          // dump the logs:
          for log in out_set.log_iter()? {
            let log = log.expect("unwrap set log data");
            let name = log.name().expect("non-utf8 log name");
            let data = log.data_str().expect("non-utf8 log data");
            writeln!(stderr, "log entry `{}`:", name).expect("stderr write");
            stderr.write_all(data.as_ref()).expect("stderr write");
            writeln!(stderr).expect("stderr write");
          }
        }

        return Err(err.into());
      },
    }

    assert_eq!(out_set.executables_len()?, 1);
    let exe = out_set.get_executable(0)?;
    let exe = exe.data()?;

    {
      info!("attempting to parse HSA metadata note for {}",
            codegen.root().symbol);
      // parse the code object metadata from a special note section
      let object = match Object::parse(&exe)? {
        Object::Elf(elf) => elf,
        // LLVM should never give us anything other than an ELF image.
        _ => unreachable!("can only load from elf files"),
      };

      let mut metadata: Option<HsaMetadataMap> = None;
      if let Some(notes) = object.iter_note_sections(&exe, None) {
        for note in notes {
          let note = note?;
          if note.n_type != NT_AMDGPU_METADATA { continue; }

          let desc = note.desc;
          let md = rmps_from_slice(desc)?;
          info!("found NT_AMDGPU_METADATA note: {:#?}", md);
          metadata = Some(md);
          break;
        }
      }

      let metadata = metadata
        .ok_or("missing NT_AMDGPU_METADATA note")?;

      let name_to_idx: HashMap<_, _> = metadata
        .kernels
        .iter()
        .enumerate()
        .map(|(idx, kernel_md)| {
          (kernel_md.kernel_desc_symbol, idx)
        })
        .collect();

      for root in codegen.entries.iter_mut() {
        let &idx = name_to_idx.get(&root.symbol[..])
          .ok_or("missing metadata kernel record")?;
        let kernel_md = metadata.kernels
          .get(idx)
          .unwrap();

        root.platform.group_segment_size = kernel_md
          .group_segment_size as _;
        root.platform.kernarg_segment_size = kernel_md
          .kernarg_segment_size as _;
        root.platform.private_segment_size = kernel_md
          .private_segment_size as _;
        root.platform.workgroup_fbarrier_count = 0;
        // 2^5 = 32
        root.platform.group_segment_p2align = 5; // XXX
        root.platform.private_segment_p2align = 5; // XXX
        root.platform.kernarg_segment_p2align = kernel_md
          .kernarg_segment_align
          .trailing_zeros() as _;
      }
    }

    codegen.put_exe(exe);

    Ok(())
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[derive(Hash)]
pub struct KernelDesc {
  // TODO
}
impl KernelDesc {
  pub fn new<F, Args, Ret>(_f: &F) -> Self
    where F: Fn<Args, Output = Ret>,
  {
    KernelDesc { }
  }
}
impl PlatformKernelDesc for KernelDesc { }

/// Most fields are filled in during `post_codegen`, after the worker
/// asks us to create this info.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[derive(Default, Hash)]
pub struct CodegenDesc {
  pub group_segment_size: usize,
  pub kernarg_segment_size: usize,
  pub private_segment_size: usize,
  /// TODO always zero. Not present in NT_AMDGPU_METADATA
  pub workgroup_fbarrier_count: u32,

  // grouped for packing

  pub group_segment_p2align: u8,
  pub kernarg_segment_p2align: u8,
  pub private_segment_p2align: u8,
}

impl PlatformCodegenDesc for CodegenDesc { }

// See https://llvm.org/docs/AMDGPUUsage.html#code-object-v3-metadata-mattr-code-object-v3
const NT_AMDGPU_METADATA: u32 = 32;
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct HsaMetadataMap<'a> {
  #[serde(rename = "amdhsa.version")]
  version: (u32, u32),
  // skip "amdhsa.printf"
  #[serde(borrow, rename = "amdhsa.kernels")]
  kernels: Vec<HsaKernelMetadataMap<'a>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct HsaKernelMetadataMap<'a> {
  #[serde(borrow, rename = ".name")]
  name: &'a str,
  #[serde(borrow, rename = ".symbol")]
  kernel_desc_symbol: &'a str,
  #[serde(rename = ".kernarg_segment_size")]
  kernarg_segment_size: u32,
  #[serde(rename = ".group_segment_fixed_size")]
  group_segment_size: u32,
  #[serde(rename = ".private_segment_fixed_size")]
  private_segment_size: u32,
  #[serde(rename = ".kernarg_segment_align")]
  kernarg_segment_align: u32,
  #[serde(rename = ".wavefront_size")]
  wavefront_size: u32,
  #[serde(rename = ".sgpr_count")]
  sgpr_count: u32,
  #[serde(rename = ".vgpr_count")]
  vgpr_count: u32,
  #[serde(rename = ".max_flat_workgroup_size")]
  max_flat_workgroup_size: u32,
}

pub fn run_cmd(mut cmd: Command) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
  info!("running command {:?}", cmd);
  let mut child = cmd.spawn()?;
  if !child.wait()?.success() {
    Err(format!("command failed: {:?}", cmd).into())
  } else {
    Ok(())
  }
}
