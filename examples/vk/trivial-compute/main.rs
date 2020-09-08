
#![feature(geobacter)]

use std::geobacter::platform::platform;
use std::geobacter::spirv::{*, workitem::*};
use std::iter;
use std::num::NonZeroU32;
use std::sync::Arc;

use grt_vk::VkAccel;
use grt_core::context::Context;

use vk::buffer::BufferUsage;
use vk::buffer::cpu_access::{CpuAccessibleBuffer, };
use vk::command_buffer::{CommandBuffer, AutoCommandBufferBuilder, };
use vk::descriptor::descriptor_set::{PersistentDescriptorSet, UnsafeDescriptorSetLayout};
use vk::device::{Device, Features, DeviceExtensions};
use vk::instance::{layers_list, Instance, InstanceExtensions, PhysicalDevice,
                   debug::DebugCallback, debug::MessageSeverity,
                   debug::MessageType};
use vk::pipeline::ComputePipeline;
use vk::sync::GpuFuture;


const ELEMENTS: usize = 4096;
type Element = u32;

static mut DATA: Buffer<RuntimeArray32<Element>, 0, 0> = Buffer::new(RuntimeArray32::new());

fn kernel() {
  // we have to use unsafe here because all invocations can access any
  // part of `DATA` (as reflected in the use of a static mut).
  let id = global_invocation_id()[0] as usize;
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
  println!("layers = {:?}", layers);

  let ctxt = Context::new()
    .expect("create context");

  let mut features = Features::none();
  features.robust_buffer_access = true;
  features.buffer_device_address = true;
  features.variable_pointers = true;
  features.shader_int8 = true;
  features.shader_int64 = true;

  let app_info = vk::app_info_from_cargo_toml!();
  let mut exts = InstanceExtensions::supported_by_core()
    .expect("InstanceExtensions::supported_by_core");
  exts.ext_debug_utils = true;
  let instance = Instance::new(Some(app_info).as_ref(),
                               &exts,
                               layers.iter().map(|s| &s[..] ))
    .expect("Instance::new");

  let mut severity = MessageSeverity::none();
  severity.error = true;
  severity.information = true;
  severity.verbose = true;
  severity.warning = true;
  let ty = MessageType::all();
  let _debug_callback =
    DebugCallback::new(&instance, severity,
                       ty, |msg| {
        let ty = if msg.severity.error {
          "error"
        } else if msg.severity.warning {
          "warning"
        } else if msg.severity.verbose {
          "verbose"
        } else if msg.severity.information {
          "information"
        } else {
          unimplemented!()
        };

        println!("{} {}: {}", msg.layer_prefix,
                 ty, msg.description);
      })
      .ok();

  let mut data = vec![0; ELEMENTS];
  for (idx, dest) in data.iter_mut().enumerate() {
    *dest = idx as _;
  }

  for phy in PhysicalDevice::enumerate(&instance) {
    println!("Physical Device Name: `{}`", phy.name());
    println!("Features: {:#?}", phy.supported_features());

    // initialization
    let exts = DeviceExtensions::supported_by_device(phy);
    println!("Extensions: {:#?}", exts);
    let q_fam = phy.queue_families()
      .find(|fam| {
        fam.supports_compute()
      });
    if q_fam.is_none() {
      println!("Couldn't find compute queue family; skipping");
      continue;
    }
    let q_fam = Some((q_fam.unwrap(), 1.0));
    let (device, mut queues) = Device::new(phy, &features,
                                           &exts, q_fam)
      .expect("create vk device");
    let queue = queues.next()
      .expect("no compute queue?");

    let dev = VkAccel::new(&ctxt, device.clone())
      .expect("failed to create Geobacter accelerator");

    let wg_size = (NonZeroU32::new(256).unwrap(), NonZeroU32::new(1).unwrap(),
                   NonZeroU32::new(1).unwrap());
    let compiled = dev.compile_compute_testing(kernel, Some(wg_size))
      .expect("failed to compile kernel");

    // finished core initialization

    println!("initializing data buffer..");
    let data_buffer = unsafe {
      let usage = BufferUsage {
        storage_buffer: true,
        .. BufferUsage::none()
      };
      let buffer_size = <RuntimeArray32<Element>>::layout(data.len() as _)
        .unwrap()
        .size();
      let buf = <CpuAccessibleBuffer<RuntimeArray32<Element>>>::raw(
        device.clone(),
        buffer_size,
        usage, true,
        iter::empty()
      )
        .expect("failed to create buffer");

      {
        let mut write = buf.write().unwrap();
        write.initialize_copy_from_slice(&data);
      }

      buf
    };
    println!("finished initializing data buffer");

    println!("sets = {:#?}", compiled.pipeline_layout());
    let set_0 = compiled.pipeline_layout()
      .set_iter()
      .nth(0)
      .unwrap();

    let layout = UnsafeDescriptorSetLayout::new(device.clone(), set_0)
      .expect("UnsafeDescriptorSetLayout::new");
    let layout = Arc::new(layout);

    let desc_set_builder = PersistentDescriptorSet::start(layout);

    let desc_set_builder = desc_set_builder
      .add_buffer(data_buffer.clone())
      .expect("set/binding mismatch?");

    let desc_set = desc_set_builder.build()
      .expect("failed to build desc set");
    let desc_sets = vec![desc_set];

    let compute_mod = compiled.compute_entry_ref().unwrap();

    let pipeline = ComputePipeline::new(device.clone(),
                                        &compute_mod,
                                        &())
      .expect("create compute pipeline");
    let pipeline = Arc::new(pipeline);

    let mut cmd_buf = AutoCommandBufferBuilder::new(device.clone(), queue.family())
      .expect("failed to create auto cmd buf");

    cmd_buf
      .dispatch([(ELEMENTS / 256) as _, 1, 1], pipeline.clone(),
                desc_sets, ())
      .expect("failed to build cmd buf dispatch");

    let cmd_buf = cmd_buf.build()
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
