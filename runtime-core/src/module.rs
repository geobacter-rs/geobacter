
use std::error::Error;
use std::fmt;
use std::marker::{PhantomData, };
use std::mem::{size_of, };
use std::ptr::{NonNull, };
use std::sync::{Arc, };

use indexvec::Idx;

use nd;
use nd::Dimension;

use hsa_core::kernel::{kernel_id_for, KernelId, };

use hsa_rt::mem::region::{RegionBox, Region, };
use hsa_rt::queue::{DispatchPacket, IQueue, QueueKind, };
use hsa_rt::signal::{Signal, WaitState, ConditionOrdering, };

pub use hsa_rt::queue::{FenceScope, Error as QueueError, };

use {Accelerator, AcceleratorId, };
use context::{Context, ModuleContextData, ModuleData, Kernel, };

newtype_index!(FunctionId);

pub struct Function<F, Args, Ret>
  where F: Fn<Args, Output = Ret>,
{
  id: KernelId,
  context_data: ModuleContextData<Args, Ret>,
  f: F,
}

impl<F, Args, Ret> Function<F, Args, Ret>
  where F: Fn<Args, Output = Ret>,
{
  pub fn new(f: F) -> Self {
    Function {
      id: kernel_id_for(&f),
      context_data: From::from(&f),
      f,
    }
  }
}

impl<F, Args, Ret> Clone for Function<F, Args, Ret>
  where F: Fn<Args, Output = Ret> + Clone,
{
  fn clone(&self) -> Self {
    Function {
      id: self.id,
      context_data: self.context_data,
      f: self.f.clone(),
    }
  }
}
impl<F, Args, Ret> Copy for Function<F, Args, Ret>
  where F: Fn<Args, Output = Ret> + Copy,
{ }

#[derive(Debug)]
pub enum CallError {
  Queue(QueueError),
  Compile(Box<Error>),
  Oom(Box<Error>),
  ArgStorageMismatch,
  CompletionSignal(Box<Error>),
}
impl fmt::Display for CallError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:?}", self) // TODO
  }
}
impl From<QueueError> for CallError {
  fn from(v: QueueError) -> Self {
    CallError::Queue(v)
  }
}

pub struct Invoc<F, Args, WGDim, GridDim>
  where F: Fn<Args, Output = ()>,
        WGDim: nd::IntoDimension + Clone,
        GridDim: nd::IntoDimension + Clone,
{
  pub context: Context,
  pub workgroup_dim: WGDim,
  pub grid_dim: GridDim,
  pub dynamic_group_size: u32,
  pub dynamic_private_size: u32,
  pub begin_fence: Option<FenceScope>,
  pub end_fence: Option<FenceScope>,
  f: Function<F, Args, ()>,
  context_data: Arc<ModuleData>,
}

impl<F, Args, WGDim, GridDim> Invoc<F, Args, WGDim, GridDim>
  where F: Fn<Args, Output=()>,
        WGDim: nd::IntoDimension + Clone,
        GridDim: nd::IntoDimension + Clone,
{
  pub fn new(context: Context, f: F) -> Result<Self, Box<Error>>
    where WGDim: Default,
          GridDim: Default,
  {
    let f = Function::new(f);
    let context_data = f.context_data
      .get_cache_data(f.id, &context);
    Ok(Invoc {
      context,
      workgroup_dim: Default::default(),
      grid_dim: Default::default(),
      dynamic_group_size: 0,
      dynamic_private_size: 0,
      begin_fence: Some(FenceScope::System),
      end_fence: Some(FenceScope::System),
      f,
      context_data,
    })
  }
  fn set_dims<Dim>(into_dims: &mut Dim, dim: Dim) -> Result<(), Box<Error>>
    where Dim: nd::IntoDimension + Copy,
  {
    let dim_ = dim.into_dimension();

    let slice = dim_.slice();

    if slice.len() > 3 {
      return Err("dim must be <= 3".into());
    } else if slice.len() == 0 {
      return Err("dim must not be zero".into());
    }

    *into_dims = dim;

    Ok(())
  }
  pub fn workgroup_dims(&mut self, dim: WGDim) -> Result<(), Box<Error>>
    where WGDim: Copy,
  {
    Self::set_dims(&mut self.workgroup_dim, dim)
  }
  pub fn grid_dims(&mut self, dim: GridDim) -> Result<(), Box<Error>>
    where GridDim: Copy,
  {
    Self::set_dims(&mut self.grid_dim, dim)
  }
  pub fn with_workgroup_dims<WGDim2>(self, dim: WGDim2)
    -> Result<Invoc<F, Args, WGDim2, GridDim>, Box<Error>>
    where WGDim2: nd::IntoDimension + Clone,
  {
    let Invoc {
      context,
      grid_dim,
      dynamic_group_size,
      dynamic_private_size,
      begin_fence,
      end_fence,
      f,
      context_data,
      ..
    } = self;
    Ok(Invoc {
      workgroup_dim: dim,

      context,
      grid_dim,
      dynamic_group_size,
      dynamic_private_size,
      begin_fence,
      end_fence,
      f,
      context_data,
    })
  }
  pub fn with_grid_dims<GridDim2>(self, dim: GridDim2)
    -> Result<Invoc<F, Args, WGDim, GridDim2>, Box<Error>>
    where GridDim2: nd::IntoDimension + Clone,
  {
    let Invoc {
      context,
      workgroup_dim,
      dynamic_group_size,
      dynamic_private_size,
      begin_fence,
      end_fence,
      f,
      context_data,
      ..
    } = self;
    Ok(Invoc {
      grid_dim: dim,

      context,
      workgroup_dim,
      dynamic_group_size,
      dynamic_private_size,
      begin_fence,
      end_fence,
      f,
      context_data,
    })
  }

  pub fn precompile(&self, accel: &Arc<Accelerator>)
    -> Result<Arc<Kernel>, Box<Error>>
  {
    self.context_data.compile(&self.context, accel)
  }

  pub fn allocate_kernarg_storage(&self, accel: &Arc<Accelerator>)
    -> Result<ArgsStorage, Box<Error>>
  {
    ArgsStorage::new(accel, size_of::<Args>())
  }

  /// `completion` must already have the correct value set, eg set to `1`.
  /// `LocalArgs` should be the same type as the real args; we use a new parameter
  /// here so borrow lifetimes are realistic. Otherwise, Rust thinks the provided args
  /// need to live as long as `self`.
  pub fn call_async<'a, 'b, 'c, T, K, LocalArgs>(&'c self, args: LocalArgs,
                                                 accel: &'c Arc<Accelerator>,
                                                 queue: &'b T,
                                                 completion: &'b Signal,
                                                 args_storage: &'a mut ArgsStorage)
    -> Result<InvocCompletion<'a, 'b, LocalArgs, T>, CallError>
    where T: IQueue<K>,
          K: QueueKind,
          'a: 'b,
  {
    if args_storage.accel_id != accel.id() {
      return Err(CallError::ArgStorageMismatch);
    }
    if args_storage.args_size < size_of::<LocalArgs>() {
      return Err(CallError::ArgStorageMismatch);
    }

    let kernel = match self.context_data.compile(&self.context, accel) {
      Ok(k) => k,
      Err(e) => { return Err(CallError::Compile(e)); },
    };

    assert_eq!(kernel.kernarg_segment_size as usize,
               size_of::<LocalArgs>(),
               "kernarg size mismatch");

    let mut kernargs: NonNull<LocalArgs> = args_storage.ptr.cast();
    unsafe {
      *kernargs.as_mut() = args;
    }

    let kernel_queue = queue;

    let Invoc {
      workgroup_dim,
      grid_dim,
      dynamic_group_size,
      dynamic_private_size,
      begin_fence,
      end_fence,
      ..
    } = self;

    unsafe {
      let dispatch = DispatchPacket {
        workgroup_size: workgroup_dim.clone(),
        grid_size: grid_dim.clone(),
        group_segment_size: dynamic_group_size + kernel.group_segment_size,
        private_segment_size: dynamic_private_size + kernel.private_segment_size,
        scaquire_scope: begin_fence.clone(),
        screlease_scope: end_fence.clone(),
        ordered: true,
        kernel_object: kernel.main_object,
        kernel_args: unsafe { kernargs.as_mut() },
        completion_signal: completion,
      };

      kernel_queue
        .try_enqueue_kernel_dispatch(dispatch)?
        .into_async();
    }

    Ok(InvocCompletion {
      storage: args_storage,
      args: PhantomData,
      waited: false,
      _queue: queue,
      signal: completion,
    })
  }

  /// Invoke this function on the provided accelerator.
  pub fn call_accel_sync<T, K, LocalArgs>(self, accel: &Arc<Accelerator>,
                                          kernel_queue: &T,
                                          args: LocalArgs)
    -> Result<(), CallError>
    where T: IQueue<K>,
          K: QueueKind,
  {
    let kernel = match self.context_data.compile(&self.context, accel) {
      Ok(k) => k,
      Err(e) => { return Err(CallError::Compile(e)); },
    };
    // TODO args type should be `(&mut Ret, Args)`. Need to have
    // codegen create a wrapper.
    //let required_size = align_up(kernel.kernarg_segment_size as usize,
    //                             16);
    //let supplied_size = align_up(size_of::<Args>(), 16);
    assert_eq!(kernel.kernarg_segment_size as usize,
               size_of::<Args>(),
               "kernarg size mismatch");

    //let mut ret_place: Ret = unsafe { ::std::mem::uninitialized() };

    {
      let kernargs_region = accel.kernargs_region();
      let mut kernargs = match RegionBox::new(kernargs_region, args) {
        Ok(k) => k,
        Err(e) => { return Err(CallError::Oom(e)); },
      };

      let consumer = accel.host_accel()
        .expect("host_accel")
        .agent()
        .clone();
      let completion_signal = match Signal::new(1, &[consumer]) {
        Ok(v) => v,
        Err(e) => { return Err(CallError::CompletionSignal(e)); }
      };

      let Invoc {
        context: _context,
        workgroup_dim,
        grid_dim,
        dynamic_group_size,
        dynamic_private_size,
        begin_fence,
        end_fence,
        ..
      } = self;
      let dispatch = DispatchPacket {
        workgroup_size: workgroup_dim,
        grid_size: grid_dim,
        group_segment_size: dynamic_group_size + kernel.group_segment_size,
        private_segment_size: dynamic_private_size + kernel.private_segment_size,
        scaquire_scope: begin_fence,
        screlease_scope: end_fence,
        ordered: true,
        kernel_object: kernel.main_object,
        kernel_args: kernargs.as_mut(),
        completion_signal: &completion_signal,
      };

      let wait = kernel_queue
        .try_enqueue_kernel_dispatch(dispatch)?;
      wait.wait(WaitState::Blocked);
    }

    Ok(())
  }
}

impl<F, Args, WGDim, GridDim> Clone for Invoc<F, Args, WGDim, GridDim>
  where F: Fn<Args, Output = ()> + Clone,
        WGDim: nd::IntoDimension + Clone,
        GridDim: nd::IntoDimension + Clone,
{
  fn clone(&self) -> Self {
    Invoc {
      context: self.context.clone(),
      workgroup_dim: self.workgroup_dim.clone(),
      grid_dim: self.grid_dim.clone(),
      dynamic_group_size: self.dynamic_group_size,
      dynamic_private_size: self.dynamic_private_size,
      begin_fence: self.begin_fence.clone(),
      end_fence: self.end_fence.clone(),
      f: self.f.clone(),
      context_data: self.context_data.clone(),
    }
  }
}

/// Use this to invoc in a loop without allocating every iteration
/// AND without running amuck of Rust's borrow checker.
pub struct ArgsStorage {
  accel_id: AcceleratorId,
  kernargs_region: Region,
  ptr: NonNull<()>,
  args_size: usize,
}
impl ArgsStorage {
  /// Create storage for args of up to size `n` for use on the provided accelerator.
  pub fn new(accel: &Arc<Accelerator>, n: usize) -> Result<Self, Box<Error>> {
    let kernargs_region = accel.kernargs_region().clone();
    let ptr = unsafe {
      kernargs_region.allocate::<u8>(n)?
    };
    Ok(ArgsStorage {
      accel_id: accel.id(),
      kernargs_region,
      ptr: ptr.cast(),
      args_size: n,
    })
  }
}

impl Drop for ArgsStorage {
  fn drop(&mut self) {
    // we don't need to dtor the argument type, as `InvocCompletion` handles that.
    unsafe {
      if let Err(e) = self.kernargs_region.deallocate(self.ptr.as_ptr()) {
        error!("failed to deallocate kernel arg storage: {:?}", e);
      }
    }
  }
}

#[must_use]
pub struct InvocCompletion<'a, 'b, Args, Queue>
  where 'a: 'b,
{
  storage: &'a mut ArgsStorage,
  args: PhantomData<Args>,
  waited: bool,
  _queue: &'b Queue,
  signal: &'b Signal,
}

impl<'a, 'b, Args, Queue> InvocCompletion<'a, 'b, Args, Queue>
  where 'a: 'b,
{
  pub fn wait(mut self, active: bool) {
    let wait = if active {
      WaitState::Active
    } else {
      WaitState::Blocked
    };

    loop {
      let val = self.signal
        .wait_scacquire(ConditionOrdering::Less, 1, None,
                        wait.clone());
      debug!("completion signal wakeup: {}", val);
      if val == 0 { break; }
      if val < 0 {
        panic!("completion signal unblocked with a negative value: {}",
               val);
      }
    }

    self.waited = true;
  }
}

impl<'a, 'b, Args, Queue> Drop for InvocCompletion<'a, 'b, Args, Queue>
  where 'a: 'b,
{
  fn drop(&mut self) {
    if !self.waited {
      loop {
        let val = self.signal
          .wait_scacquire(ConditionOrdering::Less, 1, None,
                          WaitState::Active);
        debug!("completion signal wakeup: {}", val);
        if val == 0 { break; }
        if val < 0 {
          panic!("completion signal unblocked with a negative value: {}",
                 val);
        }
      }
    }

    unsafe {
      ::std::ptr::drop_in_place(self.storage.ptr.cast::<Args>().as_ptr())
    }
  }
}

fn align_up(size: usize, align: usize) -> usize {
  let padding = (align - (size - 1) % align) - 1;
  size + padding
}
