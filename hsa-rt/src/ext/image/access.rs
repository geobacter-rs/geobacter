decl! {
  #[repr(u32)]
  pub enum Access {
    ReadOnly = 1,
    WriteOnly = 2,
    ReadWrite = 3,
  }
  pub trait AccessDetail: Default, Copy, { }
}

pub trait ReadAccess: AccessDetail { }
impl ReadAccess for ReadOnly { }
impl ReadAccess for ReadWrite { }

pub trait WriteAccess: AccessDetail { }
impl WriteAccess for WriteOnly { }
impl WriteAccess for ReadWrite { }
