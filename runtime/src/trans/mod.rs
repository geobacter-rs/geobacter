
use std::error::Error;
use std::ffi::{CString};

use self::target_options::{TargetOptions, CodeModel};

use ir;
use indexvec::IndexVec;

use llvm;

pub mod target_options;

pub struct Module {
  pub llcx: llvm::ContextRef,
  pub llmod: llvm::ModuleRef,
}
impl Module {
  pub fn new(name: &str, llvm: &Llvm,
             target_options: &TargetOptions) -> Self {
    let llcx = unsafe { llvm::LLVMContextCreate() };
    let name = CString::new(name).unwrap();
    let llmod = unsafe {
      llvm::LLVMModuleCreateWithNameInContext(name.as_ptr(), llcx)
    };

    unsafe {
      llvm::LLVMRustSetDataLayoutFromTargetMachine(llmod, llvm.tm);
    }

    let triple = CString::new(target_options.target_triple.as_bytes())
      .unwrap();
    unsafe {
      llvm::LLVMRustSetNormalizedTarget(llmod, triple.as_ptr());
    }

    Module {
      llcx, llmod,
    }
  }
}

pub struct Llvm {
  pub tm: llvm::TargetMachineRef,
}
impl Llvm {
  pub fn new(target_options: &TargetOptions) -> Result<Self, Box<Error>> {
    use self::target_options::OptLevel;
    let opt_level = match target_options.opt_level {
      OptLevel::None => llvm::CodeGenOptLevel::None,
      OptLevel::Less => llvm::CodeGenOptLevel::Less,
      OptLevel::Default => llvm::CodeGenOptLevel::Default,
      OptLevel::Aggressive => llvm::CodeGenOptLevel::Aggressive,
    };

    let triple = CString::new(target_options.target_triple.as_bytes()).unwrap();
    let processor = CString::new(target_options.target_processor.as_bytes()).unwrap();
    let features = CString::new("".as_bytes()).unwrap();

    let is_pie_binary = false;
    let reloc_model = llvm::RelocMode::Default;
    let code_model = match target_options.code_mode {
      CodeModel::Small => llvm::CodeModel::Small,
      CodeModel::Large => llvm::CodeModel::Large,
    };

    let ffunction_sections = false;
    let fdata_sections = true;
    let use_softfp = false;

    let tm = unsafe {
      llvm::LLVMRustCreateTargetMachine(
        triple.as_ptr(),
        processor.as_ptr(),
        features.as_ptr(),
        code_model,
        reloc_model,
        opt_level,
        use_softfp,
        is_pie_binary,
        ffunction_sections,
        fdata_sections,
      )
    };

    if tm.is_null() {
      Err(format!("Could not create LLVM TargetMachine for triple: {}",
                  triple.to_str().unwrap()))?;
    }

    Ok(Llvm {
      tm,
    })
  }
}

impl Drop for Llvm {
  fn drop(&mut self) {
    if !self.tm.is_null() {
      unsafe {
        llvm::LLVMRustDisposeTargetMachine(self.tm);
      }

      self.tm = 0 as _;
    }
  }
}

pub struct ModuleBuilder {
  module: Module,
  llvm: Llvm,

  llfns: IndexVec<ir::Function, llvm::ValueRef>,
}
impl ModuleBuilder {
  pub fn new(name: &str, target_options: &TargetOptions) -> Result<Self, Box<Error>> {
    let llvm = Llvm::new(target_options)?;
    let m = Module::new(name, &llvm, target_options);
    Ok(ModuleBuilder {
      module: m,
      llvm,

      llfns: Default::default(),
    })
  }

  pub fn build_module(&mut self, module: &ir::Module) {
    for function in module.funcs.iter() {

    }
  }
}

pub struct FunctionBuilder<'mbb> {
  mbb: &'mbb mut ModuleBuilder,
  fid: ir::Function,
  llfn: llvm::ValueRef,

  llbbs: IndexVec<ir::BasicBlock, llvm::BasicBlockRef>,
}
impl<'mbb> FunctionBuilder<'mbb> {
}