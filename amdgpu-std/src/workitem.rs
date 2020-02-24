
use crate::DispatchPacket;

extern "rust-intrinsic" {
  fn __geobacter_workitem_x_id() -> u32;
  fn __geobacter_workitem_y_id() -> u32;
  fn __geobacter_workitem_z_id() -> u32;
  fn __geobacter_workgroup_x_id() -> u32;
  fn __geobacter_workgroup_y_id() -> u32;
  fn __geobacter_workgroup_z_id() -> u32;
}

pub trait WorkType<T> { }
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct WorkItem;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct WorkGroup;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum AxisDim {
  X,
  Y,
  Z,
}

#[derive(Default, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct AxisDimX;
#[derive(Default, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct AxisDimY;
#[derive(Default, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct AxisDimZ;

pub trait WorkItemAxis {
  fn workitem_id(&self) -> u32;
}
impl WorkItemAxis for AxisDim {
  fn workitem_id(&self) -> u32 {
    match self {
      &AxisDim::X => AxisDimX.workitem_id(),
      &AxisDim::Y => AxisDimY.workitem_id(),
      &AxisDim::Z => AxisDimZ.workitem_id(),
    }
  }
}
impl WorkItemAxis for AxisDimX {
  fn workitem_id(&self) -> u32 {
    unsafe { __geobacter_workitem_x_id() as _ }
  }
}
impl WorkItemAxis for AxisDimY {
  fn workitem_id(&self) -> u32 {
    unsafe { __geobacter_workitem_y_id() as _ }
  }
}
impl WorkItemAxis for AxisDimZ {
  fn workitem_id(&self) -> u32 {
    unsafe { __geobacter_workitem_z_id() as _ }
  }
}

pub trait WorkGroupAxis {
  fn workgroup_id(&self) -> u32;
  fn workgroup_size(&self, p: DispatchPacket) -> u32;
}
impl WorkGroupAxis for AxisDim {
  fn workgroup_id(&self) -> u32 {
    match self {
      &AxisDim::X => AxisDimX.workgroup_id(),
      &AxisDim::Y => AxisDimY.workgroup_id(),
      &AxisDim::Z => AxisDimZ.workgroup_id(),
    }
  }
  fn workgroup_size(&self, p: DispatchPacket) -> u32 {
    match self {
      &AxisDim::X => AxisDimX.workgroup_size(p),
      &AxisDim::Y => AxisDimY.workgroup_size(p),
      &AxisDim::Z => AxisDimZ.workgroup_size(p),
    }
  }
}
impl WorkGroupAxis for AxisDimX {
  fn workgroup_id(&self) -> u32 {
    unsafe { __geobacter_workgroup_x_id() as _ }
  }
  fn workgroup_size(&self, p: DispatchPacket) -> u32 {
    p.0.workgroup_size_x as _
  }
}
impl WorkGroupAxis for AxisDimY {
  fn workgroup_id(&self) -> u32 {
    unsafe { __geobacter_workgroup_y_id() as _ }
  }
  fn workgroup_size(&self, p: DispatchPacket) -> u32 {
    p.0.workgroup_size_y as _
  }
}
impl WorkGroupAxis for AxisDimZ {
  fn workgroup_id(&self) -> u32 {
    unsafe { __geobacter_workgroup_z_id() as _ }
  }
  fn workgroup_size(&self, p: DispatchPacket) -> u32 {
    p.0.workgroup_size_z as _
  }
}
pub trait GridAxis {
  fn grid_size(&self, p: DispatchPacket) -> u32;
}
impl GridAxis for AxisDim {
  fn grid_size(&self, p: DispatchPacket) -> u32 {
    match self {
      &AxisDim::X => AxisDimX.grid_size(p),
      &AxisDim::Y => AxisDimY.grid_size(p),
      &AxisDim::Z => AxisDimZ.grid_size(p),
    }
  }
}
impl GridAxis for AxisDimX {
  fn grid_size(&self, p: DispatchPacket) -> u32 {
    p.0.grid_size_x
  }
}
impl GridAxis for AxisDimY {
  fn grid_size(&self, p: DispatchPacket) -> u32 {
    p.0.grid_size_y
  }
}
impl GridAxis for AxisDimZ {
  fn grid_size(&self, p: DispatchPacket) -> u32 {
    p.0.grid_size_z
  }
}

pub fn workitem_id() -> [u32; 3] {
  [
    AxisDimX.workitem_id(),
    AxisDimY.workitem_id(),
    AxisDimZ.workitem_id(),
  ]
}
pub fn workgroup_id() -> [u32; 3] {
  [
    AxisDimX.workgroup_id(),
    AxisDimY.workgroup_id(),
    AxisDimZ.workgroup_id(),
  ]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum WorkDims<T> {
  One([T; 1]),
  Two([T; 2]),
  Three([T; 3]),
}

impl<T> WorkDims<T> {
  pub fn len(&self) -> usize {
    match self {
      &WorkDims::One(_) => 1,
      &WorkDims::Two(_) => 2,
      &WorkDims::Three(_) => 3,
    }
  }
  pub fn drop_t(&self) -> WorkDims<()> {
    match self {
      &WorkDims::One(_) => WorkDims::One([(); 1]),
      &WorkDims::Two(_) => WorkDims::Two([(); 2]),
      &WorkDims::Three(_) => WorkDims::Three([(); 3]),
    }
  }
}

impl<T> WorkDims<T>
  where T: Default + Copy,
{
  pub fn one_default() -> Self {
    WorkDims::One([Default::default(); 1])
  }
  pub fn two_default() -> Self {
    WorkDims::Two([Default::default(); 2])
  }
  pub fn three_default() -> Self {
    WorkDims::Three([Default::default(); 3])
  }
}

impl DispatchPacket {
  pub fn work_dims(self) -> WorkDims<()> {
    let dims = self.0.setup as u16;
    unsafe { ::core::intrinsics::assume(dims <= 3 && dims > 0) };

    match dims {
      1 => WorkDims::One([(); 1]),
      2 => WorkDims::Two([(); 2]),
      3 => WorkDims::Three([(); 3]),
      _ => unreachable!("dims is out of range: {:?}", dims),
    }
  }
  pub fn workgroup_size(self) -> [u32; 3] {
    [
      AxisDimX.workgroup_size(self),
      AxisDimY.workgroup_size(self),
      AxisDimZ.workgroup_size(self),
    ]
  }
  pub fn grid_size(self) -> [u32; 3] {
    [
      AxisDimX.grid_size(self),
      AxisDimY.grid_size(self),
      AxisDimZ.grid_size(self),
    ]
  }
  pub fn global_linear_id(self) -> usize {
    let [l0, l1, l2] = workitem_id();
    let [g0, g1, g2] = workgroup_id();
    let [s0, s1, s2] = self.workgroup_size();
    let [n0, n1, _n2] = self.grid_size();

    let n0 = n0 as usize;
    let n1 = n1 as usize;

    let i0 = (g0 * s0 + l0) as usize;
    let i1 = (g1 * s1 + l1) as usize;
    let i2 = (g2 * s2 + l2) as usize;
    (i2 * n1 + i1) * n0 + i0
  }
  pub fn global_id_x(self) -> u32 {
    self.global_id(AxisDimX)
  }
  pub fn global_id_y(self) -> u32 {
    self.global_id(AxisDimY)
  }
  pub fn global_id_z(self) -> u32 {
    self.global_id(AxisDimZ)
  }
  pub fn global_id<T>(self, axis: T) -> u32
    where T: WorkItemAxis + WorkGroupAxis,
  {
    let l = axis.workitem_id();
    let g = axis.workgroup_id();
    let s = axis.workgroup_size(self);
    g * s + l
  }
  pub fn global_id_dim(self) -> (u32, u32, u32) {
    (self.global_id_x(), self.global_id_y(), self.global_id_z())
  }
}

pub trait ReadFirstLane {
  unsafe fn read_first_lane(self) -> Self;
}
extern "rust-intrinsic" {
  fn __geobacter_amdgpu_readfirstlane(v: u32) -> u32;
}
macro_rules! read_first_lane_sprim {
  ($($prim:ty, )*) => {$(
    impl ReadFirstLane for $prim {
      unsafe fn read_first_lane(self) -> Self {
        __geobacter_amdgpu_readfirstlane(self as _) as _
      }
    }
  )*}
}
read_first_lane_sprim!(i8, u8, i16, u16, i32, u32, );

#[cfg(target_pointer_width = "32")]
read_first_lane_sprim!(usize, isize, );

macro_rules! read_first_lane_x64 {
  ($($prim:ty, )*) => {$(
    impl ReadFirstLane for $prim {
      unsafe fn read_first_lane(self) -> Self {
        let h1 = (self & (0xffffffff << 32)) >> 32;
        let h2 = self & 0xffffffff;

        let h1 = __geobacter_amdgpu_readfirstlane(h1 as _) as Self;
        let h2 = __geobacter_amdgpu_readfirstlane(h2 as _) as Self;

        (h1 << 32) | h2
      }
    }
  )*}
}
read_first_lane_x64!(i64, u64, );
#[cfg(target_pointer_width = "64")]
read_first_lane_x64!(usize, isize, );
