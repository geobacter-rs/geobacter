
# All targets

First you'll need to create the `Context`:
```rust
extern crate geobacter_runtime_core as grt_core;

use std::error::Error;

use grt_core::context::Context;

fn main() -> Result<(), Box<dyn Error>> {
  let ctx = Context::new()?;
}
```

You'll need to compile all of your programs with
`RUSTFLAGS=-Z always-encode-mir -Z always-emit-metadata`! 
This used to be set automatically, but that was changed when Geobacter's drivers were
integrated into Geobacter's Rust fork.

# AMDGPU

Before we begin, please note that the `geobacter_runtime_amd` crate is unstable, and will likely 
undergo breaking changes as more Things are brought into the fold of safety. Attempts will be made 
to maintain compatibility as much as possible, but no promises.

If your device has PCI-E large BAR, you'll be able to directly access your GPU memory from the CPU.
More on this below. NOTE: Since PCI-E is such a huge nice-to-have feature, especially for Rust's
safety desires, this feature will likely become somewhat required in the future (but probably not a
hard requirement, just that the lack there of won't receive new features).

Some general bad behaviour:
* Calling trait objects (or virtual dispatch), this includes *all* formatting code. For two reasons:
  one, the LLVM AmdGpu target machine doesn't support it, second..
* Moving Trait objects across the kernel arguments boundary will fault, as each device will have
  different virtual function table pointers. Ie passing your kernel `(&() as &dyn Any)` and then 
  calling `Any::type_id` on it will fault (though the lack of virtual dispatch makes this hard to
  trigger)!
* Host stack references. Host stacks are not in accessible memory!
* Allocating on the GPU. You can use a bump allocator, allocated on the host, on the
  GPU to allocate.
* Calling a function which uses `asm!`.
* Anything not Rust code, including anything called via FFI. This includes calling
  anything in libc.
* Panics. These aren't actually disallowed, but panicking will cause the workitem to
  terminate. *No drops run if this happens*, the workitem just stops.

Everything else is fair game.

Some non-issues:
* 32bit system controlling a 64bit device. HSA requires that all agents use the same pointer size.
* Host/GPU endianness mismatch: the HSA specification actually requires that they match. Vulkan 
  is similar.

## The Basics

First we have to create the AMDGPU device object:
```rust
extern crate geobacter_runtime_amd  as grt_amd;

use std::error::Error;

use grt_amd::prelude::*;

fn main() -> Result<(), Box<dyn Error>> {
  let ctx = Context::new()?;
  let dev = HsaAmdGpuAccel::first_device(&ctx)?;
}
```

As far as device setup goes, that's actually it!

To run some code on your GPU, you need to first start with a "arguments" structure, and a few fields:
```rust

// `GeobacterDeps` implements the Deps trait, which is used to ensure things like async memcpy's are
// finished before the GPU command processor actually launches a dispatch's workgroups.
#[derive(GeobacterDeps)] 
struct MyKernel<'a> {
  /// This represents a memory ring, which is written to by the CPU (or GPU if device side enqueue is
  /// performed) with a dispatch's details (grid size, workgroup size, etc).
  /// A `DeviceMultiQueue` is one that implements Sync.
  queue: DeviceMultiQueue,
  /// The signal decremented by the GPU when the dispatch completes. This signal is omitted from the
  /// deps used for dispatching (otherwise, it would block forever!)
  /// `GlobalSignal` is a signal type which can be waited on by all HSA agents on your system.
  /// For GPUs, "waited on" means the signal can be used in barrier submissions.
  completion: GlobalSignal,

  // Now whatever data is required by the kernel:
  /// The input data. NOTE: this absolutely *MUST* reside in GPU accessible memory. Geobacter provides 
  /// utilities for ensuring this, like `LapVec`/`LapBox`. Crucially, stack memory is not accessible
  /// to the GPU! This might be fixed someday, not today.  
  input: &'a [u32],
  /// A *pointer* to the output. This is specifically not a mutable reference; more on why later.
  /// Similarly to `input`, this must point to GPU accessible memory. 
  output: NonNull<[u32]>,
  /// To ensure the mutable borrow on the output slice remains live while the dispatch is running.
  _lt: PhantomData<&'a mut [u32]>,
}

/// NonNull<_> implements !Sync, so we have to force Sync for `Kernel`:
unsafe impl<'a> Sync for MyKernel<'a> { }
```
Programming GPUs is naturally a asynchronous affair, so the `completion` field is used by the 
`Completion` trait so the GPU command processor knows how to signal dispatch completion:
```rust
impl<'a> Completion for MyKernel<'a> {
  type CompletionSignal = GlobalSignal;
  #[inline(always)]
  fn completion(&self) -> &GlobalSignal { &self.completion }
}
```
Now we need to implement the `Kernel` trait:
```rust
impl<'a> Kernel for MyKernel<'a> {
  /// The dispatch grid. This type is used for safely optimizing the indexing math to only that
  /// which is needed. 
  type Grid = Dim1D<RangeTo<u32>>;
  /// The workgroup size. This type will match the Grid dimension.
  const WORKGROUP: Dim1D<RangeTo<u32>> = Dim1D {
    x: ..16,
  };

  type Queue = DeviceMultiQueue;
  #[inline(always)]
  fn queue(&self) -> &Self::Queue { &self.queue }

  /// This is the kernel which is run on the GPU! Note `&self`: each workitem gets a copy of the
  /// kernel arguments, so any drop implementation would be run for each if not for the &.
  /// This is also the reason `&mut` isn't useful in your kernel arguments structure.
  fn kernel(&self, vp: KVectorParams<Self>) {
    // get the Globally Linear id for this workitem:
    let id = vp.gl_id() as usize;
    // Safety: `id` is unique per workitem, so this mutable reference is thus also unique.
    let dst = unsafe {
      &mut *self.output.as_ptr().add(id)
    };
    *dst = self.input[id];
  }
}
```
You'll notice that `MyKernel::kernel` has no attributes. As stated elsewhere, Geobacter hooks into
the compiler to get the information it needs for cross-codegen-ing functions at runtime. This means 
all device functions are *also* compiled for the host; if device specific functions are called on
the host, they will panic instead.

Returning back to our `main`, putting the previous code blocks together:

```rust
extern crate geobacter_runtime_amd  as grt_amd;

use std::error::Error;
use std::ptr::NonNull;
use std::rc::Rc;

use grt_amd::prelude::*;

#[derive(GeobacterDeps)] 
struct MyKernel<'a> {
  queue: DeviceMultiQueue,
  completion: GlobalSignal,

  input: &'a [u32],
  output: NonNull<[u32]>,
  _lt: PhantomData<&'a mut [u32]>,
}
impl<'a> MyKernel<'a> {
  fn new(queue: DeviceMultQueue, completion: GlobalSignal,
         input: &'a [u32], output: &'a mut [u32]) -> Self {
    MyKernel {
      queue,
      completion,
      
      input,
      output: NonNull::from(output),
      _lt: PhantomData,
    }       
  }
}

unsafe impl<'a> Sync for MyKernel<'a> { }

impl<'a> Completion for MyKernel<'a> {
  type CompletionSignal = GlobalSignal;
  #[inline(always)]
  fn completion(&self) -> &GlobalSignal { &self.completion }
}
impl<'a> Kernel for MyKernel<'a> {
  type Grid = Dim1D<RangeTo<u32>>;
  const WORKGROUP: Dim1D<RangeTo<u32>> = Dim1D {
    x: ..16,
  };

  type Queue = DeviceMultiQueue;
  #[inline(always)]
  fn queue(&self) -> &Self::Queue { &self.queue }

  fn kernel(&self, vp: KVectorParams<Self>) {
    let id = vp.gl_id() as usize;
    // Safety: `id` is unique per workitem, so this mutable reference is thus also unique.
    let dst = unsafe {
      &mut *self.output.as_ptr().add(id)
    };
    *dst = self.input[id];
  }
}

fn main() -> Result<(), Box<dyn Error>> {
  let ctx = Context::new()?;
  let dev = HsaAmdGpuAccel::first_device(&ctx)?;

  let module: FuncModule<_> = MyKernel::module(&dev);
  // `FuncModule` is a structure which holds a few details not related to specific
  // dispatches. For example, you can disable the pre/post memory fences (that is unsafe, 
  // however).
  
  module.compile_async(); // launch a job to codegen the kernel to your GPU.
  // We've got other stuff to do while that happens.

  // Create a global signal for the dispatch completion. Perhaps surprisingly, creating
  // global signals is faster than device specific ones.
  let completion = GlobalSignal::new(1)?;

  let grid = Dim1D { x: ..4096 * 4096, };

  // An allocator which allocates on the first NUMA node in your system.
  // This implies the GPU will read from system RAM. For serious work, you'll likely
  // want to move this sort of stuff into GPU RAM, but you'll still have to first
  // allocate it in something like this (unless large BAR is available). 
  let alloc = dev.fine_lap_node_alloc(0); 
  let mut input = LapVec::new_in(alloc.clone());

  // Grant GPU access to this vec (note: it's not actually allocated yet, the grant will
  // happen after allocating)
  input.add_access(&dev)?;

  input.resize(grid.linear_len().unwrap() as _, 0u32);
  for (i, input) in input.iter_mut().enumerate() {
    *input = i as u32;
  }

  let mut output = LapVec::new_in(alloc);
  output.add_access(&dev)?;
  output.resize(grid.linear_len().unwrap() as _, 0u32);

  // Allocate a pool for the kernel arguments. This is occasionally pretty expensive (multiple
  // milliseconds), so for less trivial programs you'll want to avoid creating these often.
  // Here, `1` is number of launches this pool should have room for.
  // You can also create this pool with a byte size instead. This pool isn't shared between
  // threads so we use a Rc here.
  let pool = Rc::new(ArgsPool::new::<MyKernel>(&dev, 1)?);

  // Get these two properties from the codegen results, invoking codegen if we hadn't above:
  let group_size = invoc.group_size()?;
  let private_size = invoc.private_size().unwrap();
  // Those should be zero here.

  let queue = dev.create_multi_queue2(Some(128), group_size, private_size)?;

  let args = MyKernel::new(queue, completion, &input, &mut output);
  
  // prepare for dispatching:
  let mut invoc = module.into_invoc(pool);

  // Dispatching is still unsafe! The compiler can't ensure the `input` reference is in GPU
  // visible (w/ appropriate access grants) memory! Maybe someday this can be tackled.
  let wait = unsafe {
    invoc.unchecked_call_async(&grid, args)?
  };

  // now we wait for the GPU to finish copying. `true` causes this thread to spin wait
  // (because this dispatch should be extremely fast). If `false` if provided, the thread
  // will ask the kernel to park us until the condition is met.
  wait.wait_for_zero(true)?;

  // Now we do whatever is needed with the results:
  for (&input, &output) in input.iter().zip(output.iter()) {
    assert_eq!(input, output);
  }

  Ok(())
}
```

## Device Local Memory

Working with device local memory is unfortunately fairly unsafe, except if you have an
APU (an integrated GPU), which uses system RAM, or your GPU and motherboard supports
resizable PCI-E BAR.

### Determining GPU Large BAR support

You can query GPU memory visibility by calling `device.has_pcie_large_bar()`:

```rust
fn main() -> Result<(), Box<dyn Error>> {
  let ctx = Context::new()?;
  let dev = HsaAmdGpuAccel::first_device(&ctx)?;
  let has_large_bar = dev.has_pcie_large_bar();
  if has_large_bar {
    // GPU memory is CPU visible! yay
  } else {
    // GPU memory is GPU invisible :(
  }
}
```

### With Large BAR

In this case, `device.device_lap_alloc()` will return `Some(LapAlloc)`, which you can 
provide to any of the `Lap*` box types. No explicit host->device copies are required;
you also don't need to manage two pointers for the separate CPU/GPU memories.

Based on my limited testing, access does not need to be granted to host CPU agents to
access GPU memory in this case either (this is despite HSA saying access is default 
disallowed).

Note that CPU reads are especially high latency and slow. Avoid in performance critical
code at all costs.

### Without Large BAR

The safest way to use GPU VRAM currently is via the `H2DMemcpyGroup` trait. Lets modify
the previous section's code to use device local memory with this trait.

First up is modifying `MyKernel`:
```rust
#[derive(GeobacterDeps)] 
struct MyKernel<'a> {
  queue: DeviceMultiQueue,
  completion: GlobalSignal,

  /// H2DGlobalRLapVecMemTransfer is an object which manages an async memcpy to device
  /// memory. GeobacterDeps and the AMDGPU command processor will ensure the copy is
  /// complete before the kernel is run.
  input: H2DGlobalRLapVecMemTransfer<'a, u32, ()>,

  output: NonNull<[u32]>,
  _lt: PhantomData<&'a mut [u32]>,
}
```
Next is a minor modification to the kernel itself:
```rust
impl<'a> Kernel for MyKernel<'a> {
  // ...
  
  fn kernel(&self, vp: KVectorParams<Self>) {
    let id = vp.gl_id() as usize;
    // Safety: `id` is unique per workitem, so this mutable reference is thus also unique.
    let dst = unsafe {
      &mut *self.output.as_ptr().add(id)
    };
    // changed here:
    *dst = self.input.dst()[id];
  }
}
```
And lastly, the host code (including a change to `MyKernel::new`):
```rust
extern crate geobacter_runtime_amd  as grt_amd;

use std::error::Error;
use std::ptr::NonNull;
use std::rc::Rc;

use grt_amd::prelude::*;

#[derive(GeobacterDeps)] 
struct MyKernel<'a> {
  queue: DeviceMultiQueue,
  completion: GlobalSignal,

  input: H2DGlobalRLapVecMemTransfer<'a, u32, ()>,
  output: NonNull<[u32]>,
  _lt: PhantomData<&'a mut [u32]>,
}
impl<'a> MyKernel<'a> {
  fn new(dev: &Arc<HsaAmdGpuAccel>, queue: DeviceMultQueue,
         completion: GlobalSignal,
         input: &'a LapVec<u32>, output: &'a mut [u32]) 
    -> Result<Self, Box<dyn Error>>
  {
    Ok(MyKernel {
      queue,
      completion,

      // This call allocates device memory and tells the GPU to initiate a memcopy to 
      // initialize it. The second parameter can be used to make the memcopy wait on
      // any dependencies. Here we have none.
      input: input.memcopy2(dev, ())?,
 
      output: NonNull::from(output),
      _lt: PhantomData,
    })
  }
}

unsafe impl<'a> Sync for MyKernel<'a> { }

impl<'a> Completion for MyKernel<'a> {
  type CompletionSignal = GlobalSignal;
  #[inline(always)]
  fn completion(&self) -> &GlobalSignal { &self.completion }
}
impl<'a> Kernel for MyKernel<'a> {
  type Grid = Dim1D<RangeTo<u32>>;
  const WORKGROUP: Dim1D<RangeTo<u32>> = Dim1D {
    x: ..16,
  };

  type Queue = DeviceMultiQueue;
  #[inline(always)]
  fn queue(&self) -> &Self::Queue { &self.queue }

  fn kernel(&self, vp: KVectorParams<Self>) {
    let id = vp.gl_id() as usize;
    // Safety: `id` is unique per workitem, so this mutable reference is thus also unique.
    let dst = unsafe {
      &mut *self.output.as_ptr().add(id)
    };
    *dst = self.input.dst()[id];
  }
}
fn main() -> Result<(), Box<dyn Error>> {
  let ctx = Context::new()?;
  let dev = HsaAmdGpuAccel::first_device(&ctx)?;

  let module: FuncModule<_> = MyKernel::module(&dev);
  
  module.compile_async();
  let completion = GlobalSignal::new(1)?;

  let grid = Dim1D { x: ..4096 * 4096, };

  let alloc = dev.fine_lap_node_alloc(0); 
  let mut input = LapVec::new_in(alloc.clone());
  input.add_access(&dev)?;
  input.resize(grid.linear_len().unwrap() as _, 0u32);
  for (i, input) in input.iter_mut().enumerate() {
    *input = i as u32;
  }

  let mut output = LapVec::new_in(alloc);
  output.add_access(&dev)?;
  output.resize(grid.linear_len().unwrap() as _, 0u32);

  let pool = Rc::new(ArgsPool::new::<MyKernel>(&dev, 1)?);

  let group_size = invoc.group_size()?;
  let private_size = invoc.private_size().unwrap();

  let queue = dev.create_multi_queue2(Some(128), group_size, private_size)?;

  // `MyKernel::new` now returns a `Result`.
  let args = MyKernel::new(queue, completion, &input, &mut output)?;
  
  let mut invoc = module.into_invoc(pool);

  let wait = unsafe {
    invoc.unchecked_call_async(&grid, args)?
  };

  wait.wait_for_zero(true)?;

  // Now we do whatever is needed with the results:
  for (&input, &output) in input.iter().zip(output.iter()) {
    assert_eq!(input, output);
  }

  Ok(())
}
```
You can also group memcopies together and use a single signal for all of them, by 
using a tree of 2-ary tuples, or via `MemcpyGroupTuple::chain`.

## Textures

Textures are supported, and always live in device memory (attempts to put textures
in host memory will be met with GPU segfaults). The HSA spec has details on
what's supported. To use:

```rust
type ImageTy = Image<
  ReadOnly,
  Format<f32, channel_order::RGBA>,
  TwoD<u32>,
  layout::Opaque,
>;
fn main() { 
  let grid = Dim2D {
    x: ..4096,
    y: ..4096,
  };
  let mut img: ImageTy = device.create_texture(grid.len().into())
    .expect("failed to create texture");
    
  // The type of contents will depend on the format of the texture
  let mut contents = Vec::new();
  initialize_pixels(&mut contents);
    
  img.import_all_packed(&contents)
    .expect("texture import failed");
}
```

To access the texture's pixels, call `load()` or `store()` (or their masked versions):

```rust
fn kernel(&self, vp: KVectorParams<Self>) {
  let idx = (vp.grid_id().x as _, vp.grid_id().y as _);
  let pixel = self.img.load(idx);

  // If your access detail type is `WriteOnly` or `ReadWrite`, you can also store to
  // the texture:
  self.img.store(idx, [1.0f32; 4]);
  // Note that `self.img` is technically immutable. Textures don't allow complex 
  // types and multiple writes to the same pixel will be serialized by HW (though the
  // order of writes is undefined), so this isn't a problem. Additionally, accessing 
  // the raw texture allocation isn't possible, even for linear (non-tiled) textures.

  // ..
}
```

Hardware channel masking is supported:

```rust
fn kernel(&self, vp: KVectorParams<Self>) {
  let idx = (vp.grid_id().x as _, vp.grid_id().y as _);
  // Adjust Mask<Y, N, Y, N> to suit. If the pixel value is [1, 1, 1, 1], this
  // load will return [1, 0, 1, 0].
  let pixel = self.img.load_masked::<_, Mask<Y, N, Y, N>>(idx);

  // Note for stores masked off channels will get a zero. So this will store
  // [1, 0, 1, 0].
  self.img.store_masked::<_, Mask<Y, N, Y, N>>(idx, [1.0f32; 4]);

  // ..
}
```

The hardware always reads pixels in RGBA order. The channel order in the Image type
specifies the order used for importing and exporting on the host.

You can directly read/write GPU texture memory if you have Large BAR support, but note
that the texture data layout is technically implementation defined unless you use the
`Linear` layout.
