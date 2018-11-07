
use std::error::Error;
use std::mem::size_of;
use std::sync::{Arc, };

use indexvec::Idx;

use nd;
use nd::Dimension;

use hsa_core::kernel::{kernel_id_for, KernelId, };

use hsa_rt::mem::region::RegionBox;
use hsa_rt::queue::DispatchPacket;
use hsa_rt::signal::{Signal, WaitState, };

pub use hsa_rt::queue::FenceScope;

use {Accelerator, };
use context::{Context, ModuleContextData, ModuleData, };

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

pub struct Invoc<F, Args, WGDim, GridDim>
  where F: Fn<Args, Output = ()>,
        WGDim: nd::IntoDimension,
        GridDim: nd::IntoDimension,
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
        WGDim: nd::IntoDimension,
        GridDim: nd::IntoDimension,
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
    where WGDim2: nd::IntoDimension,
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
    where GridDim2: nd::IntoDimension,
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

  /// Invoke this function on the provided accelerator.
  pub fn call_accel_sync(self, accel: &Arc<Accelerator>,
                         args: Args)
    -> Result<(), Box<Error>>
  {
    let kernel = self.context_data.compile(&self.context, accel)?;
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
      let mut kernargs = RegionBox::new(kernargs_region, args)?;

      let kernel_queue = {
        let queues = accel.queues();
        let queues = if queues.len() == 0 {
          ::std::mem::drop(queues); // for `Arc::make_mut`
          accel.create_queues(1, 12)?;
          accel.queues()
        } else {
          queues
        };

        queues[0].clone()
      };

      let consumer = accel.host_accel()
        .expect("host_accel")
        .agent()
        .clone();
      let completion_signal = Signal::new(1, &[consumer])?;

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

      let wait = kernel_queue.write_dispatch_packet(dispatch)?;
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

fn align_up(size: usize, align: usize) -> usize {
  let padding = (align - (size - 1) % align) - 1;
  size + padding
}
