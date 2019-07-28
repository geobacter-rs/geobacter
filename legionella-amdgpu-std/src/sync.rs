
pub fn barrier() {
  extern "rust-intrinsic" {
    fn __legionella_amdgpu_barrier();
  }
  unsafe { __legionella_amdgpu_barrier() }
}
pub fn wave_barrier() {
  extern "rust-intrinsic" {
    fn __legionella_amdgpu_wave_barrier();
  }
  unsafe { __legionella_amdgpu_wave_barrier() }
}
