use std::any::Any;
use std::error::Error as StdError;
use std::ffi::c_void;
use std::fmt;
use std::intrinsics::atomic_store_rel;
use std::marker::PhantomData;
use std::mem::{transmute, transmute_copy, };
use std::ops::{Deref, DerefMut, };
use std::rc::Rc;
use std::slice::from_raw_parts_mut;
use std::sync::Arc;

use nd::{self, Dimension, };

use crate::ApiContext;
use crate::agent::Agent;
use crate::error::Error;
use crate::ffi;
use crate::mem::region::Region;
use crate::signal::{Signal, ConditionOrdering, WaitState, SignalRef, };

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum QueueType {
  Multiple,
  Single,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum FenceScope {
  None,
  Agent,
  System,
}
impl Default for FenceScope {
  fn default() -> Self { FenceScope::System }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum QueueError {
  Full,
  WorkgroupDimSize,
  GridDimSize,
}
impl fmt::Display for QueueError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self) // TODO
  }
}
impl StdError for QueueError { }

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
                                      &FenceScope::System,
                                      &FenceScope::System,
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
}

pub struct KernelQueue<T>
  where T: QueueKind,
{
  sys: T,
  _ctxt: ApiContext,
}

pub struct Queue<T>
  where T: QueueKind,
{
  sys: T,
  _callback_data: Option<Box<dyn Any>>,
  _ctxt: ApiContext,
}

impl Agent {
  pub fn new_kernel_queue(&self, size: u32,
                          private_segment_size: Option<u32>,
                          group_segment_size: Option<u32>)
    -> Result<KernelSingleQueue, Error>
  {
    let queue_type = ffi::hsa_queue_type_t_HSA_QUEUE_TYPE_SINGLE;
    let private_segment_size = private_segment_size
      .unwrap_or(u32::max_value());
    let group_segment_size = group_segment_size
      .unwrap_or(u32::max_value());
    let callback_data_ptr = 0 as *mut _;

    let mut out: *mut ffi::hsa_queue_t = unsafe { ::std::mem::uninitialized() };
    check_err!(ffi::hsa_queue_create(self.0, size as _, queue_type,
                                     None, callback_data_ptr,
                                     private_segment_size, group_segment_size,
                                     &mut out as *mut _))?;

    Ok(KernelQueue {
      sys: SingleQueueType(RawQueue(out)),
      _ctxt: ApiContext::upref(),
    })
  }
  pub fn new_kernel_multi_queue(&self, size: u32,
                                private_segment_size: Option<u32>,
                                group_segment_size: Option<u32>)
    -> Result<KernelMultiQueue, Error>
  {
    let queue_type = ffi::hsa_queue_type_t_HSA_QUEUE_TYPE_MULTI;
    let private_segment_size = private_segment_size
      .unwrap_or(u32::max_value());
    let group_segment_size = group_segment_size
      .unwrap_or(u32::max_value());
    let callback_data_ptr = 0 as *mut _;

    let mut out: *mut ffi::hsa_queue_t = unsafe { ::std::mem::uninitialized() };
    check_err!(ffi::hsa_queue_create(self.0, size as _, queue_type,
                                     None, callback_data_ptr,
                                     private_segment_size, group_segment_size,
                                     &mut out as *mut _))?;
    Ok(KernelQueue {
      sys: MultiQueueType(RawQueue(out)),
      _ctxt: ApiContext::upref(),
    })
  }

  pub fn new_queue<F>(&self, size: u32,
                      callback: Option<F>,
                      private_segment_size: Option<u32>,
                      group_segment_size: Option<u32>)
    -> Result<Queue<SingleQueueType>, Error>
    where F: FnMut() + 'static,
  {
    extern "C" fn callback_fn(_status: ffi::hsa_status_t,
                              _queue: *mut ffi::hsa_queue_t,
                              _data: *mut c_void) {
      // TODO
      // no unimplemented!(): panics across ffi bounds are undefined.
    }

    let queue_type = ffi::hsa_queue_type_t_HSA_QUEUE_TYPE_SINGLE;
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
    check_err!(ffi::hsa_queue_create(self.0, size as _, queue_type,
                                     callback_ffi_fn, callback_data_ptr,
                                     private_segment_size, group_segment_size,
                                     &mut out as *mut _))?;

    Ok(Queue {
      sys: SingleQueueType(RawQueue(out)),
      _callback_data: callback_data
        .map(|cb| cb as Box<dyn Any>),
      _ctxt: ApiContext::upref(),
    })
  }
  pub fn new_multi_queue<F>(&self, size: u32,
                            callback: Option<F>,
                            private_segment_size: Option<u32>,
                            group_segment_size: Option<u32>)
    -> Result<Queue<MultiQueueType>, Error>
    where F: FnMut() + 'static,
  {
    extern "C" fn callback_fn(_status: ffi::hsa_status_t,
                              _queue: *mut ffi::hsa_queue_t,
                              _data: *mut c_void) {
      // TODO
      // no unimplemented!(): panics across ffi bounds are undefined.
    }

    let queue_type = ffi::hsa_queue_type_t_HSA_QUEUE_TYPE_SINGLE;
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
    check_err!(ffi::hsa_queue_create(self.0, size as _, queue_type,
                                     callback_ffi_fn, callback_data_ptr,
                                     private_segment_size, group_segment_size,
                                     &mut out as *mut _))?;

    Ok(Queue {
      sys: MultiQueueType(RawQueue(out)),
      _callback_data: callback_data
        .map(|cb| cb as Box<dyn Any>),
      _ctxt: ApiContext::upref(),
    })
  }
}

/// XXX This doesn't implement faster operations for single queue types.
pub trait IQueue<T>
  where T: QueueKind,
{
  #[doc(hidden)]
  fn raw_queue(&self) -> &T;

  fn doorbell_ref(&self) -> SignalRef;

  fn try_enqueue_packet<F, P>(&self, f: F)
    -> Result<(), QueueError>
    where P: Copy + Sized,
          F: FnOnce(&mut P),
  {
    let sys = self.raw_queue();

    let packet_count = unsafe { (*(*sys).0).size as usize };
    let write_index = sys.add_write_index_screlease(1);
    let read_index = sys.load_read_index_scacquire();
    if write_index - read_index >= packet_count as u64 {
      return Err(QueueError::Full);
    }

    let base_addr = unsafe {
      (*(*sys).0).base_address as *mut P
    };
    let packets = unsafe {
      from_raw_parts_mut(base_addr, packet_count)
    };

    let packet_index = write_index as usize & (packet_count - 1);
    let packet = &mut packets[packet_index];

    f(packet);

    // Here is why we can ignore possible races:
    /*
     * Signal object used by the application to indicate the ID of a packet that
     * is ready to be processed. The HSA runtime manages the doorbell signal. If
     * the application tries to replace or destroy this signal, the behavior is
     * undefined.
     *
     * If @a type is ::HSA_QUEUE_TYPE_SINGLE, the doorbell signal value must be
     * updated in a monotonically increasing fashion. If @a type is
     * ::HSA_QUEUE_TYPE_MULTI, the doorbell signal value can be updated with any
     * value.
     */
    // `SingleQueueType` specifically impls `!Sync` so such a queue can't race in
    // this function.
    self.doorbell_ref()
      .store_screlease(write_index as i64);

    Ok(())
  }

  /// XXX Need a way to pass the dep signals in by value and still have them
  /// kept alive until this barrier finishes.
  fn try_enqueue_barrier_and<'a, D>(&self, deps: &mut D,
                                    completion: Option<&Signal>)
    -> Result<(), QueueError>
    where D: Iterator<Item = &'a SignalRef>,
  {
    let ty = header(ffi::hsa_packet_type_t_HSA_PACKET_TYPE_BARRIER_AND,
                    &FenceScope::None,
                    &FenceScope::None,
                    completion.is_none()); // XXX: ??
    let invalid_ty = ffi::hsa_packet_type_t_HSA_PACKET_TYPE_INVALID;
    let invalid_ty = header(invalid_ty,
                            &FenceScope::None,
                            &FenceScope::None,
                            completion.is_none()); // XXX: ??

    self.try_enqueue_packet(|packet: &mut ffi::hsa_barrier_and_packet_t| {
      packet_store_rel(packet, invalid_ty, 0);

      if let Some(signal) = completion {
        packet.completion_signal = signal.0;
      }

      for dst_dep in packet.dep_signal.iter_mut() {
        *dst_dep = deps.next()
          .map(|d| d.0 )
          .unwrap_or_default();
      }

      packet_store_rel(packet, ty, 0);
    })?;

    Ok(())
  }
  fn try_enqueue_barrier_or<'a, D>(&self, deps: &mut D,
                               completion: Option<&Signal>)
    -> Result<(), QueueError>
    where D: Iterator<Item = &'a SignalRef>,
  {
    let ty = header(ffi::hsa_packet_type_t_HSA_PACKET_TYPE_BARRIER_OR,
                    &FenceScope::None,
                    &FenceScope::None,
                    completion.is_none()); // XXX: ??
    let invalid_ty = ffi::hsa_packet_type_t_HSA_PACKET_TYPE_INVALID;
    let invalid_ty = header(invalid_ty,
                            &FenceScope::None,
                            &FenceScope::None,
                            completion.is_none()); // XXX: ??

    self.try_enqueue_packet(|packet: &mut ffi::hsa_barrier_or_packet_t| {
      packet_store_rel(packet, invalid_ty, 0);

      if let Some(signal) = completion {
        packet.completion_signal = signal.0;
      }

      for dst_dep in packet.dep_signal.iter_mut() {
        *dst_dep = deps.next()
          .map(|d| d.0 )
          .unwrap_or_default();
      }

      packet_store_rel(packet, ty, 0);
    })?;

    Ok(())
  }
  fn try_enqueue_kernel_dispatch<'a, WGDim, GridDim, Args>(&self, dispatch: DispatchPacket<'a, WGDim, GridDim, Args>)
    -> Result<(), QueueError>
    where WGDim: nd::IntoDimension + Clone,
          GridDim: nd::IntoDimension + Clone,
  {
    // check the packet params before we get a write index.
    dispatch.check()?;

    let ty = header(ffi::hsa_packet_type_t_HSA_PACKET_TYPE_KERNEL_DISPATCH,
                    &dispatch.scaquire_scope,
                    &dispatch.screlease_scope,
                    dispatch.ordered);
    let invalid_ty = ffi::hsa_packet_type_t_HSA_PACKET_TYPE_INVALID;
    let invalid_ty = header(invalid_ty,
                            &FenceScope::None,
                            &FenceScope::None,
                            true);

    self.try_enqueue_packet(|packet: &mut ffi::hsa_kernel_dispatch_packet_t| {
      packet_store_rel(packet, invalid_ty, 0);
      let grid_size = dispatch.initialize_packet(packet);

      let setup = (grid_size as u16) << ffi::hsa_kernel_dispatch_packet_setup_t_HSA_KERNEL_DISPATCH_PACKET_SETUP_DIMENSIONS;
      packet_store_rel(packet, ty, setup);
    })?;

    Ok(())
  }
}

impl<T> IQueue<T> for Queue<T>
  where T: QueueKind,
{
  fn raw_queue(&self) -> &T { &self.sys }
  fn doorbell_ref(&self) -> SignalRef {
    SignalRef(unsafe {
      (*self.sys.0).doorbell_signal
    })
  }
}
impl<T> IQueue<T> for KernelQueue<T>
  where T: QueueKind,
{
  fn raw_queue(&self) -> &T { &self.sys }
  fn doorbell_ref(&self) -> SignalRef {
    SignalRef(unsafe {
      (*self.sys.0).doorbell_signal
    })
  }
}
impl<T, U> IQueue<U> for Rc<T>
  where T: IQueue<U> + ?Sized,
        U: QueueKind,
{
  fn raw_queue(&self) -> &U { (&**self).raw_queue() }
  fn doorbell_ref(&self) -> SignalRef {
    (&**self).doorbell_ref()
  }
}
impl<T, U> IQueue<U> for Arc<T>
  where T: IQueue<U> + ?Sized,
        U: QueueKind,
{
  fn raw_queue(&self) -> &U { (&**self).raw_queue() }
  fn doorbell_ref(&self) -> SignalRef {
    (&**self).doorbell_ref()
  }
}
impl<T, U> IQueue<U> for Box<T>
  where T: IQueue<U> + ?Sized,
        U: QueueKind,
{
  fn raw_queue(&self) -> &U { (&**self).raw_queue() }
  fn doorbell_ref(&self) -> SignalRef {
    (&**self).doorbell_ref()
  }
}

fn scope_to_enum(scope: &FenceScope) -> u16 {
  match scope {
    FenceScope::None => ffi::hsa_fence_scope_t_HSA_FENCE_SCOPE_NONE as u16,
    FenceScope::System => ffi::hsa_fence_scope_t_HSA_FENCE_SCOPE_SYSTEM as u16,
    FenceScope::Agent => ffi::hsa_fence_scope_t_HSA_FENCE_SCOPE_AGENT as u16
  }
}

fn header(ty: ffi::hsa_packet_type_t,
          scaquire: &FenceScope,
          screlease: &FenceScope,
          ordered: bool) -> u16 {
  let mut header = (ty as u16) << ffi::hsa_packet_header_t_HSA_PACKET_HEADER_TYPE;

  let v = scope_to_enum(scaquire);
  let shift = ffi::hsa_packet_header_t_HSA_PACKET_HEADER_SCACQUIRE_FENCE_SCOPE;
  header |= v << shift;

  let v = scope_to_enum(screlease);
  let shift = ffi::hsa_packet_header_t_HSA_PACKET_HEADER_SCRELEASE_FENCE_SCOPE;
  header |= v << shift;

  if ordered {
    let shift = ffi::hsa_packet_header_t_HSA_PACKET_HEADER_BARRIER;
    header |= 1 << shift;
  }

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
                     size: usize,
                     queue_type: QueueType,
                     kernel_dispatch: bool,
                     agent_dispatch: bool,
                     doorbell_signal: T)
    -> Result<SoftQueue<T>, Error>
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
                                                    size as _,
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

#[doc(hidden)]
#[derive(Eq, PartialEq, Ord, PartialOrd)]
pub struct RawQueue(*mut ffi::hsa_queue_t);
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

pub trait QueueKind: Deref<Target = RawQueue> { }
#[derive(Eq, PartialEq, Ord, PartialOrd)]
pub struct SingleQueueType(RawQueue);
unsafe impl Send for SingleQueueType { }
impl !Sync for SingleQueueType { }
impl Deref for SingleQueueType {
  type Target = RawQueue;
  fn deref(&self) -> &Self::Target { &self.0 }
}
impl DerefMut for SingleQueueType {
  fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0 }
}
impl QueueKind for SingleQueueType { }

#[derive(Eq, PartialEq, Ord, PartialOrd)]
pub struct MultiQueueType(RawQueue);
unsafe impl Send for MultiQueueType { }
unsafe impl Sync for MultiQueueType { }
impl Deref for MultiQueueType {
  type Target = RawQueue;
  fn deref(&self) -> &Self::Target { &self.0 }
}
impl DerefMut for MultiQueueType {
  fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0 }
}
impl QueueKind for MultiQueueType { }

pub type SingleQueue = Queue<SingleQueueType>;
pub type MultiQueue = Queue<MultiQueueType>;
pub type KernelSingleQueue = KernelQueue<SingleQueueType>;
pub type KernelMultiQueue = KernelQueue<MultiQueueType>;

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
  pub scaquire_scope: FenceScope,
  pub screlease_scope: FenceScope,
  pub kernel_object: u64,
  pub kernel_args: &'a KernArg,
  pub completion_signal: Option<&'a SignalRef>,
}

impl<'a, WGDim, GridDim, KernArg> DispatchPacket<'a, WGDim, GridDim, KernArg>
  where WGDim: nd::IntoDimension + Clone,
        GridDim: nd::IntoDimension + Clone,
{
  fn check(&self) -> Result<(), QueueError> {
    let workgroup_size =
      self.workgroup_size.clone().into_dimension();
    let grid_size =
      self.grid_size.clone().into_dimension();

    let workgroup_size = workgroup_size.slice();
    let grid = grid_size.slice();

    if workgroup_size.len() > 3 || workgroup_size.len() == 0 {
      return Err(QueueError::WorkgroupDimSize);
    }
    if grid.len() > 3 || grid.len() == 0 {
      return Err(QueueError::GridDimSize);
    }

    Ok(())
  }
  fn initialize_packet(&self, p: &mut ffi::hsa_kernel_dispatch_packet_t)
    -> usize
  {
    let workgroup_size =
      self.workgroup_size.clone()
        .into_dimension();
    let grid_size =
      self.grid_size.clone()
        .into_dimension();

    let workgroup_size = workgroup_size.slice();
    let grid = grid_size.slice();

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

    p.completion_signal = self.completion_signal
      .map(|cs| cs.0 )
      .unwrap_or_default();

    grid.len()
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

unsafe impl<T> Send for Queue<T>
  where T: QueueKind + Send,
{ }
unsafe impl<T> Sync for Queue<T>
  where T: QueueKind + Sync,
{ }
