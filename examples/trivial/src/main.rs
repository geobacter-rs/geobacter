
extern crate runtime_core as rt;
extern crate legionella_std as lstd;
extern crate hsa_core;
#[macro_use]
extern crate log;
#[macro_use]
extern crate vulkano as vk;
extern crate env_logger;

use std::sync::{Arc, };

use vk::buffer::BufferUsage;
use vk::buffer::cpu_access::{CpuAccessibleBuffer, };
use vk::command_buffer::{CommandBuffer, AutoCommandBufferBuilder, };
use vk::descriptor::descriptor_set::{PersistentDescriptorSet, };
use vk::descriptor::pipeline_layout::PipelineLayout;
use vk::device::{Device, Features, };
use vk::instance::{layers_list, Instance, InstanceExtensions, PhysicalDevice,
                   debug::DebugCallback, debug::MessageTypes };
use vk::pipeline::ComputePipeline;
use vk::sync::GpuFuture;

use rt::accelerators::VkAccel;
use rt::context::Context;
use rt::module::KernelDesc;

use lstd::kernel::global_invocation_id;
use lstd::mem::buffer::{BufferBinding, AddBinding, };
use rt::Accelerator;

const ELEMENTS: usize = 4096;
type Element = u32;
type Data = [Element; ELEMENTS];

// TODO fix this ugly duplication of the set and binding number attributes.

#[legionella(set = "0", binding = "0",
             storage_class = "StorageBuffer")]
static mut DATA: Data = [0; ELEMENTS];

#[legionella(set = "0", binding = "0")]
fn data_binding() -> BufferBinding<Data> {
  BufferBinding::new(&data_binding)
}

#[legionella(capabilities(Kernel, Shader))]
#[legionella(local_size(x = 64, y = 1, z = 1))]
fn kernel() {
  // we have to use unsafe here because all invocations can access any
  // part of `DATA` (as reflected in the use of a static mut).
  let id = global_invocation_id().x();
  unsafe {
    if let Some(data) = DATA.get_mut(id) {
      *data *= 12;
    }
  }
}

pub fn main() {
  println!("----- NEW RUN -----");
  let mut layers = vec![];
  for layer in layers_list().unwrap() {
    layers.push(layer.name().to_string());
  }

  env_logger::init();
  let ctxt = Context::new()
    .expect("create context");

  let kernel_desc = KernelDesc::new(ctxt.clone(),
                                    kernel);

  println!("capabilities: {:?}", kernel_desc.capabilities);

  let pipeline = kernel_desc.pipeline_desc;
  info!("pipeline desc: {:#?}", pipeline);
  let raw_exts = kernel_desc.raw_device_extensions();

  let mut features = Features::none();
  features.robust_buffer_access = true;

  let app_info = app_info_from_cargo_toml!();
  let mut exts = InstanceExtensions::supported_by_core()
    .expect("InstanceExtensions::supported_by_core");
  exts.ext_debug_report = true;
  let instance = Instance::new(Some(app_info).as_ref(),
                               &exts,
                               layers.iter().map(|s| &s[..] ))
    .expect("Instance::new");

  let mut debug_message_types = MessageTypes::none();
  debug_message_types.debug = true;
  debug_message_types.error = true;
  debug_message_types.information = true;
  debug_message_types.performance_warning = true;
  debug_message_types.warning = true;
  let _debug_callback =
    DebugCallback::new(&instance,
                       debug_message_types,
                       |msg| {
                         let ty = if msg.ty.error {
                           "error"
                         } else if msg.ty.warning {
                           "warning"
                         } else if msg.ty.performance_warning {
                           "performance_warning"
                         } else if msg.ty.information {
                           "information"
                         } else if msg.ty.debug {
                           "debug"
                         } else {
                           unimplemented!()
                         };

                         println!("{} {}: {}", msg.layer_prefix,
                                  ty, msg.description);
                       });

  let mut data: Data = [0; ELEMENTS];
  for (idx, dest) in data.iter_mut().enumerate() {
    *dest = idx as _;
  }

  for phy in PhysicalDevice::enumerate(&instance) {
    println!("Physical Device Name: `{}`", phy.name());

    // initialization
    let q_fam = phy.queue_families()
      .find(|fam| {
        fam.supports_compute()
      });
    if q_fam.is_none() {
      println!("Couldn't find compute queue family; skipping");
      continue;
    }
    let q_fam = Some((q_fam.unwrap(), 1.0));
    let (device, mut queues) = Device::new(phy,
                                           &features,
                                           raw_exts.clone(),
                                           q_fam)
      .expect("create vk device");
    let queue = queues.next()
      .expect("no compute queue?");

    let vk_accel = VkAccel::new(&ctxt,
                                device.clone(),
                                queue.clone())
      .expect("failed to create Legionella accelerator");
    let vk_accel = Arc::new(vk_accel);
    let accel = vk_accel.clone() as Arc<Accelerator>;

    ctxt.add_accel(vk_accel.clone())
      .expect("failed to add accel to context");

    // finished core initialization
    let usage = BufferUsage {
      storage_buffer: true,
      .. BufferUsage::none()
    };
    let data_buffer = CpuAccessibleBuffer::from_data(device.clone(),
                                                     usage, data)
      .expect("failed to create buffer");

    let pipeline = PipelineLayout::new(device.clone(),
                                       pipeline)
      .expect("PipelineLayout::new");

    let desc_set_builder =
      PersistentDescriptorSet::start(pipeline, 0);

    let desc_set_builder = desc_set_builder
      .add_binding(&data_binding(), data_buffer.clone())
      .expect("set/binding mismatch (this *shouldn't* happen if \
              this code compiled)");

    let desc_set = desc_set_builder.build()
      .expect("failed to build desc set");
    let desc_sets = vec![desc_set];

    let compiled = kernel_desc.compile(&accel)
      .expect("failed to compile kernel");

    let pipeline = ComputePipeline::new(device.clone(),
                                        &compiled,
                                        &())
      .expect("create compute pipeline");
    let pipeline = Arc::new(pipeline);

    let cmd_buf = AutoCommandBufferBuilder::new(device.clone(), queue.family())
      .expect("failed to create auto cmd buf")
      .dispatch([(ELEMENTS / 64) as _, 1, 1], pipeline.clone(),
                desc_sets, ())
      .expect("failed to build cmd buf dispatch")
      .build()
      .expect("build cmd buf");

    cmd_buf.execute(queue.clone())
      .expect("start compute")
      .then_signal_fence_and_flush()
      .expect("then_signal_fence_and_flush")
      .wait(None)
      .expect("GpuFuture::wait");

    println!("Kernel finished; checking results");

    let content = data_buffer.read().unwrap();
    for (n, val) in content.iter().enumerate() {
      assert_eq!(*val, (n * 12) as Element);
    }

    println!("Everything succeeded for device `{}`", phy.name());
  }
}
