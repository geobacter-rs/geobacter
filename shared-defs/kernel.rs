
pub trait KernelDesc {
  fn instance_name(&self) -> Option<&str>;
  fn instance_data(&self) -> &[u8];
}
