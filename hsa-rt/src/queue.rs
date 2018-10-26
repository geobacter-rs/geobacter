use std::any::Any;
use std::error::Error;
use std::ffi::c_void;
use std::fmt;
use std::intrinsics::atomic_store_rel;
use std::marker::PhantomData;
use std::mem::{transmute, transmute_copy, };
use std::slice::from_raw_parts_mut;

use nd::{self, Dimension, };

use ffi;
use ApiContext;
use agent::Agent;
use mem::region::Region;
use signal::{Signal, ConditionOrdering, WaitState, SignalRef, };

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum QueueType {
  Multiple,
  Single,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum FenceScope {
  Agent,
  System,
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SoftQueue<T = Signal>
  where T: AsRef<Signal> + Send + Sync,
{
  sys: RawQueue,

  doorbell: T,

  _ctxt: ApiContext,
}
impl<T> SoftQueue<T>
  where T: AsRef<Signal> + Send + Sync,
{
  pub fn doorbell_ref(&self) -> &T {
    &self.doorbell
  }

  pub fn process<F, U, V>(&self, mut f: F) -> V
    where F: for<'a> FnMut(AgentPacket<'a, U>) -> ProcessLoopResult<V>,
          U: Into<u8> + From<u8>,
  {
    use signal::{ConditionOrdering, WaitState, SignalRef};

    let (base_addr, packet_count) = unsafe {
      ((*self.sys.0).base_address as *mut ffi::hsa_agent_dispatch_packet_t,
       (*self.sys.0).size as usize)
    };
    let packets = unsafe {
      from_raw_parts_mut(base_addr, packet_count)
    };

    let doorbell = self.doorbell.as_ref();

    let mut read_index = self.sys.load_read_index_scacquire();
    loop {

      loop {
        let ordering = ConditionOrdering::GreaterEqual;
        let ret = doorbell.wait_scacquire(ordering, read_index as i64,
                                          None, WaitState::Blocked);

        if ret >= read_index as i64 {
          break;
        }
      }

      let packet_index = read_index as usize & (packet_count - 1);
      let packet = &mut packets[packet_index];
      let ret = f(AgentPacket {
        sys: packet,
        _m: PhantomData,
      });
      if packet.completion_signal != Default::default() {
        SignalRef(packet.completion_signal)
          .subtract_screlease(1);
      }

      let invalid_ty = ffi::hsa_packet_type_t_HSA_PACKET_TYPE_INVALID;
      let rest = packet.type_;
      // XXX see about relaxing these scopes.
      packet_store_rel(packet, header(invalid_ty,
                                      &Some(FenceScope::System),
                                      &Some(FenceScope::System),
                                      false), rest);

      read_index += 1;
      self.sys.store_read_index_screlease(read_index);

      match ret {
        ProcessLoopResult::Exit(ret) => {
          return ret;
        },
        ProcessLoopResult::Continue => { },
      }
    }
  }

  pub fn write_dispatch_packet<'a, WGDim, GridDim, Args>(&self, dispatch: DispatchPacket<'a, WGDim, GridDim, Args>)
    -> Result<DispatchCompletion<'a, Args>, Box<Error>>
    where WGDim: nd::IntoDimension,
          GridDim: nd::IntoDimension,
  {
    let (base_addr, packet_count) = unsafe {
      ((*self.sys.0).base_address as *mut ffi::hsa_kernel_dispatch_packet_t,
       (*self.sys.0).size as usize)
    };
    let packets = unsafe {
      from_raw_parts_mut(base_addr, packet_count)
    };
    let write_index = self.sys.add_write_index_relaxed(1);
    let packet_index = write_index as usize & (packet_count - 1);
    let packet = &mut packets[packet_index];
    let invalid_ty = ffi::hsa_packet_type_t_HSA_PACKET_TYPE_INVALID;
    packet_store_rel(packet,
                     header(invalid_ty, &None, &None, false),
                     0);
    let scaquire_fence = dispatch.scaquire_scope.clone();
    let screlease_fence = dispatch.screlease_scope.clone();
    let ordered = dispatch.ordered;
    let (grid_size, kernel_args, completion_signal) = dispatch
      .initialize_packet(packet)?;

    let setup = (grid_size as u16) << ffi::hsa_kernel_dispatch_packet_setup_t_HSA_KERNEL_DISPATCH_PACKET_SETUP_DIMENSIONS;
    let ty = header(ffi::hsa_packet_type_t_HSA_PACKET_TYPE_KERNEL_DISPATCH,
                    &scaquire_fence,
                    &screlease_fence,
                    ordered);
    packet_store_rel(packet, ty, setup);
    self.doorbell_ref().as_ref()
      .store_screlease(write_index as i64);

    Ok(DispatchCompletion {
      _args: kernel_args,
      completion_signal,
    })
  }
}

pub struct Queue {
  sys: RawQueue,
  _callback_data: Option<Box<Any>>,
  _ctxt: ApiContext,
}
impl Agent {
  pub fn new_kernel_queue(&self, size: usize,
                          queue_type: QueueType,
                          private_segment_size: Option<u32>,
                          group_segment_size: Option<u32>)
    -> Result<Queue, Box<Error>>
  {
    let queue_type = match queue_type {
      QueueType::Single => ffi::hsa_queue_type_t_HSA_QUEUE_TYPE_SINGLE,
      QueueType::Multiple => ffi::hsa_queue_type_t_HSA_QUEUE_TYPE_MULTI,
    };
    let private_segment_size = private_segment_size
      .unwrap_or(u32::max_value());
    let group_segment_size = group_segment_size
      .unwrap_or(u32::max_value());
    let callback_data_ptr = 0 as *mut _;

    let mut out: *mut ffi::hsa_queue_t = unsafe { ::std::mem::uninitialized() };
    check_err!(ffi::hsa_queue_create(self.0, 1 << size, queue_type,
                                     None, callback_data_ptr,
                                     private_segment_size, group_segment_size,
                                     &mut out as *mut _))?;

    Ok(Queue {
      sys: RawQueue(out),
      _callback_data: None,
      _ctxt: ApiContext::upref(),
    })
  }
  pub fn new_queue<F>(&self, size: usize,
                      queue_type: QueueType,
                      callback: Option<F>,
                      private_segment_size: Option<u32>,
                      group_segment_size: Option<u32>)
                      -> Result<Queue, Box<Error>>
    where F: FnMut() + 'static,
  {
    extern "C" fn callback_fn(_status: ffi::hsa_status_t,
                              _queue: *mut ffi::hsa_queue_t,
                              _data: *mut c_void) {
      // TODO
      // no unimplemented!(): panics across ffi bounds are undefined.
    }

    let queue_type = match queue_type {
      QueueType::Single => ffi::hsa_queue_type_t_HSA_QUEUE_TYPE_SINGLE,
      QueueType::Multiple => ffi::hsa_queue_type_t_HSA_QUEUE_TYPE_MULTI,
    };
    let callback_ffi_fn = callback
      .as_ref()
      .map(|_| callback_fn as _);
    let private_segment_size = private_segment_size
      .unwrap_or(u32::max_value());
    let group_segment_size = group_segment_size
      .unwrap_or(u32::max_value());
    let mut callback_data = callback
      .map(|cb| Box::new(cb));
    let callback_data_ptr = callback_data
      .as_mut()
      .map(|v| {
        let v: &mut *mut c_void = unsafe {
          transmute(v)
        };
        *v
      })
      .unwrap_or(0 as *mut _);

    let mut out: *mut ffi::hsa_queue_t = unsafe { ::std::mem::uninitialized() };
    check_err!(ffi::hsa_queue_create(self.0, 1 << size, queue_type,
                                     callback_ffi_fn, callback_data_ptr,
                                     private_segment_size, group_segment_size,
                                     &mut out as *mut _))?;

    Ok(Queue {
      sys: RawQueue(out),
      _callback_data: callback_data
        .map(|cb| cb as Box<Any>),
      _ctxt: ApiContext::upref(),
    })
  }
}
impl Queue {
  fn doorbell_ref(&self) -> SignalRef {
    SignalRef(unsafe {
      (*self.sys.0).doorbell_signal
    })
  }
  pub fn write_dispatch_packet<'a, WGDim, GridDim, Args>(&self, dispatch: DispatchPacket<'a, WGDim, GridDim, Args>)
                                                         -> Result<DispatchCompletion<'a, Args>, Box<Error>>
    where WGDim: nd::IntoDimension,
          GridDim: nd::IntoDimension,
  {
    let (base_addr, packet_count) = unsafe {
      ((*self.sys.0).base_address as *mut ffi::hsa_kernel_dispatch_packet_t,
       (*self.sys.0).size as usize)
    };
    let packets = unsafe {
      from_raw_parts_mut(base_addr, packet_count)
    };
    let write_index = self.sys.add_write_index_relaxed(1);
    while write_index - self.sys.load_read_index_scacquire() >= packet_count as u64 { }
    let packet_index = write_index as usize & (packet_count - 1);
    let packet = &mut packets[packet_index];
    let invalid_ty = ffi::hsa_packet_type_t_HSA_PACKET_TYPE_INVALID;
    packet_store_rel(packet,
                     header(invalid_ty, &None, &None, false),
                     0);
    let scaquire_fence = dispatch.scaquire_scope.clone();
    let screlease_fence = dispatch.screlease_scope.clone();
    let ordered = dispatch.ordered;
    let (grid_size, kernel_args, completion_signal) = dispatch
      .initialize_packet(packet)?;

    let setup = (grid_size as u16) << ffi::hsa_kernel_dispatch_packet_setup_t_HSA_KERNEL_DISPATCH_PACKET_SETUP_DIMENSIONS;
    let ty = header(ffi::hsa_packet_type_t_HSA_PACKET_TYPE_KERNEL_DISPATCH,
                    &scaquire_fence,
                    &screlease_fence,
                    ordered);
    packet_store_rel(packet, ty, setup);
    self.doorbell_ref()
      .store_screlease(write_index as i64);

    Ok(DispatchCompletion {
      _args: kernel_args,
      completion_signal,
    })
  }
}

fn scope_to_enum(scope: &Option<FenceScope>) -> u16 {
  match scope {
    &None => ffi::hsa_fence_scope_t_HSA_FENCE_SCOPE_NONE as u16,
    &Some(FenceScope::System) => ffi::hsa_fence_scope_t_HSA_FENCE_SCOPE_SYSTEM as u16,
    &Some(FenceScope::Agent) => ffi::hsa_fence_scope_t_HSA_FENCE_SCOPE_AGENT as u16
  }
}

fn header(ty: ffi::hsa_packet_type_t,
          scaquire: &Option<FenceScope>,
          screlease: &Option<FenceScope>,
          ordered: bool) -> u16 {
  let mut header = (ty as u16) << ffi::hsa_packet_header_t_HSA_PACKET_HEADER_TYPE;

  let v = scope_to_enum(scaquire);
  let shift = ffi::hsa_packet_header_t_HSA_PACKET_HEADER_SCACQUIRE_FENCE_SCOPE;
  header |= v << shift;

  let v = scope_to_enum(screlease);
  let shift = ffi::hsa_packet_header_t_HSA_PACKET_HEADER_SCRELEASE_FENCE_SCOPE;
  header |= v << shift;

  let shift = ffi::hsa_packet_header_t_HSA_PACKET_HEADER_BARRIER;
  let v = if ordered {
    1
  } else {
    0
  };
  header |= v << shift;

  header
}

fn packet_store_rel<T>(packet: &mut T,
                       header: u16,
                       rest: u16) {
  let header = header as u32;
  let rest = rest as u32;
  let new_value = header | (rest << 16);
  unsafe {
    atomic_store_rel(packet as *mut T as *mut u32,
                     new_value);
  }
}

impl ApiContext {
  pub fn new_soft<T>(&self,
                     region: Region,
                     size_log: usize,
                     queue_type: QueueType,
                     kernel_dispatch: bool,
                     agent_dispatch: bool,
                     doorbell_signal: T)
    -> Result<SoftQueue<T>, Box<Error>>
    where T: AsRef<Signal> + Send + Sync,
  {
    let queue_type = match queue_type {
      QueueType::Single => ffi::hsa_queue_type_t_HSA_QUEUE_TYPE_SINGLE,
      QueueType::Multiple => ffi::hsa_queue_type_t_HSA_QUEUE_TYPE_MULTI,
    };
    let mut features = 0;
    if kernel_dispatch {
      features |= ffi::hsa_queue_feature_t_HSA_QUEUE_FEATURE_KERNEL_DISPATCH;
    }
    if agent_dispatch {
      features |= ffi::hsa_queue_feature_t_HSA_QUEUE_FEATURE_AGENT_DISPATCH;
    }
    let mut out: *mut ffi::hsa_queue_t = unsafe { ::std::mem::uninitialized() };
    let out = check_err!(ffi::hsa_soft_queue_create(region.0,
                                                    (1 << size_log) as _,
                                                    queue_type,
                                                    features,
                                                    doorbell_signal.as_ref().0,
                                                    &mut out as *mut _) => out)?;
    Ok(SoftQueue {
      sys: RawQueue(out),

      doorbell: doorbell_signal,

      _ctxt: self.clone(),
    })
  }
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct RawQueue(*mut ffi::hsa_queue_t);
impl fmt::Debug for RawQueue {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:p}", self.0)
  }
}
macro_rules! impl_load {
  ($f:ident, $ffi:ident) => (
    impl RawQueue {
      pub fn $f(&self) -> u64 {
        unsafe {
          ffi::$ffi(self.0)
        }
      }
    }
  )
}
macro_rules! impl_store {
  ($f:ident, $ffi:ident) => (
    impl RawQueue {
      pub fn $f(&self, val: u64) {
        unsafe {
          ffi::$ffi(self.0, val)
        }
      }
    }
  )
}
impl_load!(load_read_index_scacquire, hsa_queue_load_read_index_scacquire);
impl_store!(store_read_index_screlease, hsa_queue_store_read_index_screlease);

macro_rules! impl_add {
  ($f:ident, $ffi:ident) => (
    impl RawQueue {
      pub fn $f(&self, val: u64) -> u64 {
        unsafe {
          ffi::$ffi(self.0, val)
        }
      }
    }
  )
}
impl_add!(add_write_index_scacq_screl, hsa_queue_add_write_index_scacq_screl);
impl_add!(add_write_index_scacquire, hsa_queue_add_write_index_scacquire);
impl_add!(add_write_index_relaxed, hsa_queue_add_write_index_relaxed);
impl_add!(add_write_index_screlease, hsa_queue_add_write_index_screlease);

impl Drop for RawQueue {
  fn drop(&mut self) {
    let _ = unsafe {
      ffi::hsa_queue_destroy(self.0)
    };
    // ignore result.
  }
}

#[derive(Debug)]
pub struct DispatchPacket<'a, WGDim, GridDim, KernArg>
  where WGDim: nd::IntoDimension,
        GridDim: nd::IntoDimension,
{
  pub workgroup_size: WGDim,
  pub grid_size: GridDim,
  pub private_segment_size: u32,
  pub group_segment_size: u32,
  pub ordered: bool,
  pub scaquire_scope: Option<FenceScope>,
  pub screlease_scope: Option<FenceScope>,
  pub kernel_object: u64,
  pub kernel_args: &'a mut KernArg,
  pub completion_signal: &'a Signal,
}

/// keeps the args mut borrow live until the
/// completion signal is waited on.
pub struct DispatchCompletion<'a, KernArg> {
  _args: &'a mut KernArg,
  completion_signal: &'a Signal,
}

impl<'a, KernArg> DispatchCompletion<'a, KernArg> {
  pub fn wait(self) {
    // run our dtor.
  }
}
// The dispatch must complete before buffers are dropped and
// deallocated
impl<'a, KernArg> Drop for DispatchCompletion<'a, KernArg> {
  fn drop(&mut self) {
    loop {
      let val = self.completion_signal
        .wait_scacquire(ConditionOrdering::Equal,
                        0, None, WaitState::Active);
      debug!("completion signal wakeup: {}", val);
      if val == 0 { break; }
    }
  }
}

impl<'a, WGDim, GridDim, KernArg> DispatchPacket<'a, WGDim, GridDim, KernArg>
  where WGDim: nd::IntoDimension,
        GridDim: nd::IntoDimension,
{
  fn initialize_packet(self, p: &mut ffi::hsa_kernel_dispatch_packet_t)
    -> Result<(usize, &'a mut KernArg, &'a Signal), Box<Error>>
  {
    let workgroup_size = self.workgroup_size.into_dimension();
    let grid_size = self.grid_size.into_dimension();

    let workgroup_size = workgroup_size.slice();
    let grid = grid_size.slice();

    if workgroup_size.len() > 3 {
      return Err("Workgroup dim must be <= 3".into());
    } else if workgroup_size.len() == 0 {
      return Err("Workgroup dim must not be zero".into());
    }
    if grid.len() > 3 {
      return Err("Grid dim must be <= 3".into());
    } else if grid.len() == 0 {
      return Err("Grid dim must not be zero".into());
    }

    p.workgroup_size_x = *workgroup_size.get(0).unwrap_or(&1) as u16;
    p.workgroup_size_y = *workgroup_size.get(1).unwrap_or(&1) as u16;
    p.workgroup_size_z = *workgroup_size.get(2).unwrap_or(&1) as u16;
    p.grid_size_x = *grid.get(0).unwrap_or(&1) as u32;
    p.grid_size_y = *grid.get(1).unwrap_or(&1) as u32;
    p.grid_size_z = *grid.get(2).unwrap_or(&1) as u32;

    p.private_segment_size = self.private_segment_size;
    p.group_segment_size   = self.group_segment_size;

    p.kernel_object = self.kernel_object;
    p.kernarg_address = unsafe { transmute_copy(&self.kernel_args) };

    p.completion_signal = self.completion_signal.0;

    Ok((grid.len(), self.kernel_args, self.completion_signal))
  }
}

pub enum ProcessLoopResult<T> {
  Exit(T),
  Continue,
}

pub struct AgentPacket<'a, T>
  where T: Into<u8> + From<u8>,
{
  sys: &'a mut ffi::hsa_agent_dispatch_packet_t,
  _m: PhantomData<T>,
}

impl<'a, T> AgentPacket<'a, T>
  where T: Into<u8> + From<u8>,
{
  pub fn args(&self) -> &[u64] {
    //self.sys.args.as_ref()
    unimplemented!()
  }
  pub fn return_address<U>(&mut self) -> &mut U
    where U: Copy,
  {
    unsafe {
      ::std::mem::transmute(self.sys.return_address)
    }
  }
}
