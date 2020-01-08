// Copyright (C) 2019  Pierre Krieger
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Implements the WebGPU interface.

use futures::{channel::mpsc, lock::Mutex as AsyncMutex, prelude::*, stream::FuturesUnordered};
use parking_lot::Mutex;
use redshirt_core::native::{
    DummyMessageIdWrite, NativeProgramEvent, NativeProgramMessageIdWrite, NativeProgramRef,
};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_webgpu_interface::ffi::{self, WebGPUMessage, INTERFACE};
use std::{
    collections::HashMap,
    convert::TryFrom,
    pin::Pin,
    sync::atomic,
    time::{Duration, Instant, SystemTime},
};

/// State machine for `webgpu` interface messages handling.
pub struct WebGPUHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    /// We only allow one alive `Adapter` at any given time.
    active: Mutex<Option<ActiveState>>,
    /// Sending side of the queue of messages.
    pending_messages_tx: mpsc::UnboundedSender<(MessageId, Result<EncodedMessage, ()>)>,
    /// Queue of messages to deliver.
    pending_messages: AsyncMutex<mpsc::UnboundedReceiver<(MessageId, Result<EncodedMessage, ()>)>>,
}

struct ActiveState {
    adapter: wgpu::Adapter,
    /// Pid of the owner of the objects.
    pid: Pid,
    next_device_id: u64,
    devices: HashMap<u64, (wgpu::Device, wgpu::Queue)>,
    shader_modules: HashMap<u64, wgpu::ShaderModule>,
    bind_group_layouts: HashMap<u64, wgpu::BindGroupLayout>,
    bind_groups: HashMap<u64, wgpu::BindGroup>,
    pipeline_layouts: HashMap<u64, wgpu::PipelineLayout>,
    render_pipelines: HashMap<u64, wgpu::PipelineLayout>,
}

impl WebGPUHandler {
    /// Initializes the new state machine for WebGPU.
    pub fn new() -> Self {
        let (pending_messages_tx, pending_messages) = mpsc::unbounded();
        WebGPUHandler {
            registered: atomic::AtomicBool::new(false),
            active: Mutex::new(None),
            pending_messages_tx,
            pending_messages: AsyncMutex::new(pending_messages),
        }
    }
}

impl<'a> NativeProgramRef<'a> for &'a WebGPUHandler {
    type Future =
        Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(self) -> Self::Future {
        Box::pin(async move {
            if !self.registered.swap(true, atomic::Ordering::Relaxed) {
                return NativeProgramEvent::Emit {
                    interface: redshirt_interface_interface::ffi::INTERFACE,
                    message_id_write: None,
                    message: redshirt_interface_interface::ffi::InterfaceMessage::Register(
                        INTERFACE,
                    )
                    .encode(),
                };
            }

            let mut pending_messages = self.pending_messages.lock().await;
            let (message_id, answer) = pending_messages.next().await.unwrap();
            NativeProgramEvent::Answer {
                message_id,
                answer,
            }
        })
    }

    fn interface_message(
        self,
        interface: InterfaceHash,
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        message: EncodedMessage,
    ) {
        debug_assert_eq!(interface, INTERFACE);

        let mut active = self.active.lock();

        match WebGPUMessage::decode(message) {
            Ok(WebGPUMessage::GPURequestAdapter { options, .. }) => {
                assert!(active.is_none());      // TODO: return error instead

                let adapter = wgpu::Adapter::request(&wgpu::RequestAdapterOptions {
                    power_preference: match options.power_preference {
                        None => wgpu::PowerPreference::Default,
                        Some(ffi::GPUPowerPreference::LowPower) => wgpu::PowerPreference::LowPower,
                        Some(ffi::GPUPowerPreference::HighPerformance) => wgpu::PowerPreference::HighPerformance,
                    },
                }, wgpu::BackendBit::all()).unwrap();

                // TODO: we only allow one adapter per process at the moment; change this
                *active = Some(ActiveState {
                    adapter,
                    pid: emitter_pid,
                    next_device_id: 0,
                    devices: HashMap::new(),
                    shader_modules: HashMap::new(),
                    bind_group_layouts: HashMap::new(),
                    bind_groups: HashMap::new(),
                    pipeline_layouts: HashMap::new(),
                    render_pipelines: HashMap::new(),
                });

                if let Some(message_id) = message_id {
                    self.pending_messages_tx.unbounded_send((message_id, Ok(0xdeadbeefu64.encode()))).unwrap();
                }
            },

            Ok(WebGPUMessage::GPUAdapterRequestDevice { this, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);

                let (device, queue) = state.adapter.request_device(&wgpu::DeviceDescriptor::default());   // TODO: options
                let device_id = state.next_device_id;
                state.next_device_id += 1;
                state.devices.insert(device_id, (device, queue));
                if let Some(message_id) = message_id {
                    self.pending_messages_tx.unbounded_send((message_id, Ok(device_id.encode()))).unwrap();
                }
            },

            Ok(WebGPUMessage::GPUDeviceGetDefaultQueue { .. }) => {
                // TODO: we don't do anything; this message is hacky and we should return a QueueID ideally
            },

            Ok(WebGPUMessage::GPUDeviceCreateShaderModule { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let device = state.devices.get(&this).unwrap();
                let shader_module = device.0.create_shader_module(&descriptor.code);
                state.shader_modules.insert(return_value, shader_module);
            },

            Ok(WebGPUMessage::GPUDeviceCreateBindGroupLayout { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let device = state.devices.get(&this).unwrap();
                let bindings = descriptor.bindings.iter().map(|layout| {
                    wgpu::BindGroupLayoutBinding {
                        binding: layout.binding,
                        visibility: wgpu::ShaderStage::from_bits(layout.visibility).unwrap(),       // TODO:
                        ty: match layout.r#type {
                            ffi::GPUBindingType::UniformBuffer => unimplemented!(),//wgpu::BindingType::UniformBuffer,
                            ffi::GPUBindingType::StorageBuffer => unimplemented!(),//wgpu::BindingType::StorageBuffer,
                            ffi::GPUBindingType::ReadonlyStorageBuffer => unimplemented!(),//wgpu::BindingType::ReadonlyStorageBuffer,
                            ffi::GPUBindingType::Sampler => wgpu::BindingType::Sampler,
                            ffi::GPUBindingType::SampledTexture => unimplemented!(),//wgpu::BindingType::SampledTexture,
                            ffi::GPUBindingType::StorageTexture => unimplemented!(),//wgpu::BindingType::StorageTexture,
                        },
                    }
                }).collect::<Vec<_>>();
                let bind_group_layout = device.0.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    bindings: &bindings,
                });
                state.bind_group_layouts.insert(return_value, bind_group_layout);
            },

            Ok(WebGPUMessage::GPUDeviceCreateBindGroup { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let device = state.devices.get(&this).unwrap();
                let bindings = descriptor.bindings.iter().map(|binding| {
                    /*wgpu::Binding {
                        binding: binding.binding,
                        resource: 
                    }*/
                    unimplemented!()
                }).collect::<Vec<_>>();
                let bind_group = device.0.create_bind_group(&wgpu::BindGroupDescriptor {
                    layout: state.bind_group_layouts.get(&descriptor.layout).unwrap(), // TODO:
                    bindings: &bindings,
                });
                state.bind_groups.insert(return_value, bind_group);
            },

            Ok(WebGPUMessage::GPUDeviceCreatePipelineLayout { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let device = state.devices.get(&this).unwrap();
                let bind_group_layouts = descriptor.bind_group_layouts.iter().map(|gr| {
                    state.bind_group_layouts.get(&gr).unwrap()  // TODO:
                }).collect::<Vec<_>>();
                let pipeline_layout = device.0.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    bind_group_layouts: &bind_group_layouts,
                });
                state.pipeline_layouts.insert(return_value, pipeline_layout);
            },

            Ok(WebGPUMessage::GPUDeviceCreateRenderPipeline { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let device = state.devices.get(&this).unwrap();
                let render_pipeline = device.0.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    layout: state.layouts.get(&descriptor.parent.layout).unwrap(),
                    vertex_stage: wgpu::ProgrammableStageDescriptor {
                        module: state.shader_modules.get(&descriptor.vertex_stage.module).unwrap(),    // TODO:
                        entry_point: descriptor.vertex_stage.entry_point,
                    },
                    pub fragment_stage: Option<ProgrammableStageDescriptor<'a>>,
                    pub rasterization_state: Option<RasterizationStateDescriptor>,
                    primitive_topology: match descriptor.primitive_topology {
                        ffi::GPUPrimitiveTopology::PointList => wgpu::PrimitiveTopology::PointList,
                        ffi::GPUPrimitiveTopology::LineList => wgpu::PrimitiveTopology::LineList,
                        ffi::GPUPrimitiveTopology::LineStrip => wgpu::PrimitiveTopology::LineStrip,
                        ffi::GPUPrimitiveTopology::TriangleList => wgpu::PrimitiveTopology::TriangleList,
                        ffi::GPUPrimitiveTopology::TriangleStrip => wgpu::PrimitiveTopology::TriangleStrip,
                    },
                    pub color_states: &'a [ColorStateDescriptor],
                    pub depth_stencil_state: Option<DepthStencilStateDescriptor>,
                    index_format: match self.vertex_state.as_ref().and_then(|vs| vs.index_format) {
                        Some(ffi::GPUIndexFormat::Uint16) => wgpu::IndexFormat::Uint16,
                        None | Some(ffi::GPUIndexFormat::Uint32) => wgpu::IndexFormat::Uint32,
                    },
                    pub vertex_buffers: &'a [VertexBufferDescriptor<'a>],
                    sample_count: descriptor.sample_count.unwrap_or(1),
                    sample_mask: descriptor.sample_mask.unwrap_or(0xffffffff),
                    alpha_to_coverage_enabled: descriptor.alpha_to_coverage_enabled.unwrap_or(false),
                });
                state.render_pipelines.insert(return_value, render_pipeline);
            },

            Ok(msg) => {
                panic!("{:?}", msg);
                /*self.messages_tx
                    .unbounded_send((msg, message_id.unwrap()))
                    .unwrap();*/
            },

            Err(_) => {}
        }
    }

    fn process_destroyed(self, pid: Pid) {
        let mut active = self.active.lock();
        if let Some(a) = active.as_ref() {
            if a.pid == pid {
                *active = None;
            }
        }
    }

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}
