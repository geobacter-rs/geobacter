
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum CodeModel {
  Small,
  Large,
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum OptLevel {
  None,
  Less,
  Default,
  Aggressive,
}
impl Default for OptLevel {
  fn default() -> Self {
    OptLevel::Default
  }
}

#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct TargetOptions {
  pub opt_level: OptLevel,
  pub code_mode: CodeModel,
  pub target_triple: String,
  pub target_processor: String,
}
