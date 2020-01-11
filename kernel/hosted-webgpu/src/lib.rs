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

// TODO: make no_std?

use futures::{channel::mpsc, lock::Mutex as AsyncMutex, prelude::*, stream::FuturesUnordered};
use parking_lot::Mutex;
use raw_window_handle::HasRawWindowHandle as _;
use redshirt_core::native::{
    DummyMessageIdWrite, NativeProgramEvent, NativeProgramMessageIdWrite, NativeProgramRef,
};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_webgpu_interface::ffi::{self, WebGPUMessage, INTERFACE};
use send_wrapper::SendWrapper;
use std::{
    collections::HashMap,
    ffi::CString,
    pin::Pin,
    ptr,
    sync::atomic,
};

/// State machine for `webgpu` interface messages handling.
pub struct WebGPUHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    window: winit::window::Window,
    /// We only allow one alive `Adapter` at any given time.
    active: Mutex<Option<ActiveState>>,
    /// Sending side of the queue of messages.
    pending_messages_tx: mpsc::UnboundedSender<(MessageId, Result<EncodedMessage, ()>)>,
    /// Queue of messages to deliver.
    pending_messages: AsyncMutex<mpsc::UnboundedReceiver<(MessageId, Result<EncodedMessage, ()>)>>,
}

struct ActiveState {
    adapter: wgpu_core::id::AdapterId,
    /// Pid of the owner of the objects.
    pid: Pid,
    next_device_id: u64,
    devices: HashMap<u64, (wgpu_core::id::DeviceId, wgpu_core::id::QueueId)>,
    shader_modules: HashMap<u64, wgpu_core::id::ShaderModuleId>,
    bind_group_layouts: HashMap<u64, wgpu_core::id::BindGroupLayoutId>,
    bind_groups: HashMap<u64, wgpu_core::id::BindGroupId>,
    pipeline_layouts: HashMap<u64, wgpu_core::id::PipelineLayoutId>,
    render_pipelines: HashMap<u64, wgpu_core::id::RenderPipelineId>,
    swap_chains: HashMap<u64, (wgpu_core::id::SurfaceId, wgpu_core::id::SwapChainId)>,
    textures: HashMap<u64, wgpu_core::id::TextureId>,
    command_encoders: HashMap<u64, wgpu_core::id::CommandEncoderId>,
    render_passes: HashMap<u64, wgpu_core::id::RenderPassId>,
    command_buffers: HashMap<u64, wgpu_core::id::CommandBufferId>,
    texture_views: HashMap<u64, wgpu_core::id::TextureViewId>,
}

impl WebGPUHandler {
    /// Initializes the new state machine for WebGPU.
    pub fn new(window: winit::window::Window) -> Self {
        let (pending_messages_tx, pending_messages) = mpsc::unbounded();
        WebGPUHandler {
            registered: atomic::AtomicBool::new(false),
            window,
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

                unsafe extern "C" fn adapter_callback(
                    id: wgpu_core::id::AdapterId,
                    user_data: *mut std::ffi::c_void,
                ) {
                    *(user_data as *mut wgpu_core::id::AdapterId) = id;
                }

                let mut id = wgpu_core::id::AdapterId::ERROR;
                wgpu_native::wgpu_request_adapter_async(
                    Some(&wgpu_core::instance::RequestAdapterOptions {
                        power_preference: match options.power_preference {
                            None => wgpu_core::instance::PowerPreference::Default,
                            Some(ffi::GPUPowerPreference::LowPower) => wgpu_core::instance::PowerPreference::LowPower,
                            Some(ffi::GPUPowerPreference::HighPerformance) => wgpu_core::instance::PowerPreference::HighPerformance,
                        },
                    }),
                    wgpu_core::instance::BackendBit::all(),
                    adapter_callback,
                    &mut id as *mut _ as *mut std::ffi::c_void,
                );

                // TODO: we only allow one adapter per process at the moment; change this
                *active = Some(ActiveState {
                    adapter: id,
                    pid: emitter_pid,
                    next_device_id: 0,
                    devices: HashMap::new(),
                    shader_modules: HashMap::new(),
                    bind_group_layouts: HashMap::new(),
                    bind_groups: HashMap::new(),
                    pipeline_layouts: HashMap::new(),
                    render_pipelines: HashMap::new(),
                    swap_chains: HashMap::new(),
                    textures: HashMap::new(),
                    command_encoders: HashMap::new(),
                    render_passes: HashMap::new(),
                    command_buffers: HashMap::new(),
                    texture_views: HashMap::new(),
                });

                if let Some(message_id) = message_id {
                    self.pending_messages_tx.unbounded_send((message_id, Ok(0xdeadbeefu64.encode()))).unwrap();
                }
            },

            Ok(WebGPUMessage::GPUAdapterRequestDevice { this, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);

                // TODO: options
                let device = wgpu_native::wgpu_adapter_request_device(state.adapter, Some(&wgpu_core::instance::DeviceDescriptor::default()));
                let queue = wgpu_native::wgpu_device_get_queue(device);
                let device_id = state.next_device_id;
                state.next_device_id += 1;
                state.devices.insert(device_id, (device, queue));
                if let Some(message_id) = message_id {
                    self.pending_messages_tx.unbounded_send((message_id, Ok(device_id.encode()))).unwrap();
                }
            },

            Ok(WebGPUMessage::GPUDeviceGetDefaultQueue { .. }) => {
                // TODO: we don't do anything; this message is badly-designed right now and we should normally return a QueueID
            },

            Ok(WebGPUMessage::GPUDeviceCreateShaderModule { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let device = state.devices.get(&this).unwrap();
                let desc = wgpu_core::pipeline::ShaderModuleDescriptor {
                    code: wgpu_core::U32Array {
                        bytes: descriptor.code.as_ptr(),
                        length: descriptor.code.len(),
                    },
                };
                let shader_module = wgpu_native::wgpu_device_create_shader_module(device.0, &desc);
                state.shader_modules.insert(return_value, shader_module);
            },

            Ok(WebGPUMessage::GPUDeviceCreateBindGroupLayout { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let device = state.devices.get(&this).unwrap();
                let bindings = descriptor.bindings.iter().map(|layout| {
                    wgpu_core::binding_model::BindGroupLayoutBinding {
                        binding: layout.binding,
                        visibility: wgpu_core::binding_model::ShaderStage::from_bits(layout.visibility).unwrap(),       // TODO:
                        ty: match layout.r#type {
                            ffi::GPUBindingType::UniformBuffer => unimplemented!(),//wgpu_core::BindingType::UniformBuffer,
                            ffi::GPUBindingType::StorageBuffer => unimplemented!(),//wgpu_core::BindingType::StorageBuffer,
                            ffi::GPUBindingType::ReadonlyStorageBuffer => unimplemented!(),//wgpu_core::BindingType::ReadonlyStorageBuffer,
                            ffi::GPUBindingType::Sampler => wgpu_core::binding_model::BindingType::Sampler,
                            ffi::GPUBindingType::SampledTexture => unimplemented!(),//wgpu_core::BindingType::SampledTexture,
                            ffi::GPUBindingType::StorageTexture => unimplemented!(),//wgpu_core::BindingType::StorageTexture,
                        },
                        dynamic: false, // TODO:
                        multisampled: false, // TODO:
                        texture_dimension: wgpu_core::resource::TextureViewDimension::D2, // TODO:
                    }
                }).collect::<Vec<_>>();
                let bind_group_layout = wgpu_native::wgpu_device_create_bind_group_layout(
                    device.0,
                    &wgpu_core::binding_model::BindGroupLayoutDescriptor {
                        bindings: bindings.as_ptr(),
                        bindings_length: bindings.len(),
                    },
                );
                state.bind_group_layouts.insert(return_value, bind_group_layout);
            },

            Ok(WebGPUMessage::GPUDeviceCreateBindGroup { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let device = state.devices.get(&this).unwrap();
                let bindings = descriptor.bindings.iter().map(|binding| {
                    /*wgpu_core::binding_model::BindGroupBinding {
                        binding: binding.binding,
                        resource: 
                    }*/
                    unimplemented!()
                }).collect::<Vec<_>>();
                let bind_group = wgpu_native::wgpu_device_create_bind_group(
                    device.0,
                    &wgpu_core::binding_model::BindGroupDescriptor {
                        layout: *state.bind_group_layouts.get(&descriptor.layout).unwrap(), // TODO:
                        bindings: bindings.as_ptr(),
                        bindings_length: bindings.len(),
                    },
                );
                state.bind_groups.insert(return_value, bind_group);
            },

            Ok(WebGPUMessage::GPUDeviceCreatePipelineLayout { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let device = state.devices.get(&this).unwrap();
                let bind_group_layouts = descriptor.bind_group_layouts.iter().map(|gr| {
                    *state.bind_group_layouts.get(&gr).unwrap()  // TODO:
                }).collect::<Vec<_>>();
                let pipeline_layout = wgpu_native::wgpu_device_create_pipeline_layout(
                    device.0,
                    &wgpu_core::binding_model::PipelineLayoutDescriptor {
                        bind_group_layouts: bind_group_layouts.as_ptr(),
                        bind_group_layouts_length: bind_group_layouts.len(),
                    },
                );
                state.pipeline_layouts.insert(return_value, pipeline_layout);
            },

            Ok(WebGPUMessage::GPUDeviceCreateRenderPipeline { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let device = state.devices.get(&this).unwrap();
                let color_states = descriptor.color_states.into_iter().map(|cs| {
                    wgpu_core::pipeline::ColorStateDescriptor {
                        format: convert_texture_format(cs.format),
                        alpha_blend: Default::default(),  // FIXME:
                        color_blend: Default::default(),  // FIXME:
                        write_mask: wgpu_core::pipeline::ColorWrite::from_bits(cs.write_mask.unwrap_or(0xf)).unwrap(), // TODO:
                    }
                }).collect::<Vec<_>>();
                let vertex_entry_point = CString::new(descriptor.vertex_stage.entry_point).unwrap();
                let fragment_entry_point = if let Some(fragment_stage) = &descriptor.fragment_stage {
                    CString::new(fragment_stage.entry_point.clone()).unwrap()
                } else {
                    CString::default()
                };
                let fragment_stage = descriptor.fragment_stage.as_ref().map(|fragment_stage| {
                    wgpu_core::pipeline::ProgrammableStageDescriptor {
                        module: *state.shader_modules.get(&fragment_stage.module).unwrap(),    // TODO:
                        entry_point: fragment_entry_point.as_ptr(),
                    }
                });

                let rasterization_state = descriptor.rasterization_state.as_ref().map(|raster| {
                    wgpu_core::pipeline::RasterizationStateDescriptor {
                        front_face: match raster.front_face {
                            Some(ffi::GPUFrontFace::Ccw) => wgpu_core::pipeline::FrontFace::Ccw,
                            Some(ffi::GPUFrontFace::Cw) => wgpu_core::pipeline::FrontFace::Cw,
                            None => wgpu_core::pipeline::FrontFace::Ccw,
                        },
                        cull_mode: match raster.cull_mode {
                            Some(ffi::GPUCullMode::Front) => wgpu_core::pipeline::CullMode::Front,
                            Some(ffi::GPUCullMode::Back) => wgpu_core::pipeline::CullMode::Back,
                            Some(ffi::GPUCullMode::None) | None => wgpu_core::pipeline::CullMode::None,
                        },
                        depth_bias: raster.depth_bias.unwrap_or(0),
                        depth_bias_slope_scale: raster.depth_bias_slope_scale.map(|f| f32::from(f)).unwrap_or(0.0),
                        depth_bias_clamp: raster.depth_bias_clamp.map(|f| f32::from(f)).unwrap_or(0.0),
                    }
                });

                let vertex_buffers = Vec::new(); // FIXME:

                let render_pipeline = wgpu_native::wgpu_device_create_render_pipeline(device.0, &wgpu_core::pipeline::RenderPipelineDescriptor {
                    layout: *state.pipeline_layouts.get(&descriptor.parent.layout).unwrap(),
                    vertex_stage: wgpu_core::pipeline::ProgrammableStageDescriptor {
                        module: *state.shader_modules.get(&descriptor.vertex_stage.module).unwrap(),    // TODO:
                        entry_point: vertex_entry_point.as_ptr(),
                    },
                    fragment_stage: fragment_stage.as_ref().map_or(ptr::null(), |p| p as *const _),
                    rasterization_state: rasterization_state.as_ref().map_or(ptr::null(), |p| p as *const _),
                    primitive_topology: match descriptor.primitive_topology {
                        ffi::GPUPrimitiveTopology::PointList => wgpu_core::pipeline::PrimitiveTopology::PointList,
                        ffi::GPUPrimitiveTopology::LineList => wgpu_core::pipeline::PrimitiveTopology::LineList,
                        ffi::GPUPrimitiveTopology::LineStrip => wgpu_core::pipeline::PrimitiveTopology::LineStrip,
                        ffi::GPUPrimitiveTopology::TriangleList => wgpu_core::pipeline::PrimitiveTopology::TriangleList,
                        ffi::GPUPrimitiveTopology::TriangleStrip => wgpu_core::pipeline::PrimitiveTopology::TriangleStrip,
                    },
                    color_states: color_states.as_ptr(),
                    color_states_length: color_states.len(),
                    depth_stencil_state: ptr::null(), // FIXME:
                    vertex_input: wgpu_core::pipeline::VertexInputDescriptor {
                        index_format: match descriptor.vertex_state.as_ref().and_then(|vs| vs.index_format.as_ref()) {
                            Some(ffi::GPUIndexFormat::Uint16) => wgpu_core::pipeline::IndexFormat::Uint16,
                            None | Some(ffi::GPUIndexFormat::Uint32) => wgpu_core::pipeline::IndexFormat::Uint32,
                        },
                        vertex_buffers: vertex_buffers.as_ptr(),
                        vertex_buffers_length: vertex_buffers.len(),
                    },
                    sample_count: descriptor.sample_count.unwrap_or(1),
                    sample_mask: descriptor.sample_mask.unwrap_or(0xffffffff),
                    alpha_to_coverage_enabled: descriptor.alpha_to_coverage_enabled.unwrap_or(false),
                });
                state.render_pipelines.insert(return_value, render_pipeline);
            },

            Ok(WebGPUMessage::GPUCanvasContextConfigureSwapChain { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let device = state.devices.get(&descriptor.device).unwrap(); // TODO: don't unwrap
                // TODO: this function destroys all previous swapchains including textures
                let size = self.window.inner_size();
                let surface = wgpu_native::wgpu_create_surface(self.window.raw_window_handle());
                let surface = wgpu_native::wgpu_create_surface(self.window.raw_window_handle());
                let swap_chain = wgpu_native::wgpu_device_create_swap_chain(device.0, surface, &wgpu_core::swap_chain::SwapChainDescriptor {
                    usage: wgpu_core::resource::TextureUsage::from_bits(descriptor.usage.unwrap_or(0x10)).unwrap(), // TODO:
                    format: convert_texture_format(descriptor.format),
                    width: size.width,
                    height: size.height,
                    present_mode: wgpu_core::swap_chain::PresentMode::NoVsync, // TODO:
                });
                state.swap_chains.insert(return_value, (surface, swap_chain));
            },

            Ok(WebGPUMessage::GPUSwapChainGetCurrentTexture { this, return_value }) => {
                // TODO: this message is badly-designed
                /*let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let swap_chain = state.swap_chains.get(&this).unwrap(); // TODO: don't unwrap*/
            },

            Ok(WebGPUMessage::GPUTextureCreateView { this, return_value, descriptor }) => {
                // TODO: we assume that this is the swap chain texture view for now
                /*let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let texture = state.textures.get(&this).unwrap();*/
            },

            Ok(WebGPUMessage::GPUDeviceCreateCommandEncoder { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let device = state.devices.get(&this).unwrap();
                let command_encoder = wgpu_native::wgpu_device_create_command_encoder(device.0, Some(&wgpu_core::command::CommandEncoderDescriptor {
                    todo: 0,
                }));
                state.command_encoders.insert(return_value, command_encoder);
            },

            Ok(WebGPUMessage::GPUCommandEncoderBeginRenderPass { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let command_encoder = *state.command_encoders.get_mut(&this).unwrap();
                // FIXME: hack
                let view = wgpu_native::wgpu_swap_chain_get_next_texture(state.swap_chains.values_mut().next().unwrap().1).view_id;
                assert_ne!(view, wgpu_core::id::Id::ERROR);     // TODO:
                let color_attachments = descriptor.color_attachments.into_iter().map(move |atch| {
                    wgpu_core::command::RenderPassColorAttachmentDescriptor {
                        attachment: view,  // FIXME:
                        resolve_target: ptr::null(),   // FIXME:
                        load_op: wgpu_core::command::LoadOp::Clear, // FIXME:
                        store_op: match atch.store_op {
                            Some(ffi::GPUStoreOp::Clear) => wgpu_core::command::StoreOp::Clear,
                            Some(ffi::GPUStoreOp::Store) | None => wgpu_core::command::StoreOp::Store,
                        },
                        clear_color: wgpu_core::Color { // FIXME:
                            r: 0.0,
                            g: 1.0,
                            b: 0.0,
                            a: 0.0,
                        }
                    }
                }).collect::<Vec<_>>();
                let next_idx = state.texture_views.len() as u64;
                state.texture_views.insert(next_idx, view); // FIXME: hack:
                let render_pass = wgpu_native::wgpu_command_encoder_begin_render_pass(
                    command_encoder,
                    &wgpu_core::command::RenderPassDescriptor {
                        color_attachments: color_attachments.as_ptr(),
                        color_attachments_length: color_attachments.len(),
                        depth_stencil_attachment: ptr::null(),  // FIXME:
                    },
                );
                state.render_passes.insert(return_value, render_pass);
            },

            Ok(WebGPUMessage::GPURenderPassEncoderSetPipeline { this, pipeline }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let render_pass = *state.render_passes.get_mut(&this).unwrap();
                let pipeline = *state.render_pipelines.get(&pipeline).unwrap();
                wgpu_native::wgpu_render_pass_set_pipeline(render_pass, pipeline);
            },

            Ok(WebGPUMessage::GPURenderPassEncoderSetBindGroup { this, index, bind_group, dynamic_offsets }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let render_pass = *state.render_passes.get_mut(&this).unwrap();
                let offsets = dynamic_offsets.into_iter().map(u64::from).collect::<Vec<_>>();
                wgpu_native::wgpu_render_pass_set_bind_group(
                    render_pass,
                    index,
                    *state.bind_groups.get(&bind_group).unwrap(), // TODO:
                    offsets.as_ptr(),
                    offsets.len(),
                );
            },

            Ok(WebGPUMessage::GPURenderPassEncoderDraw { this, vertex_count, instance_count, first_vertex, first_instance }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let render_pass = *state.render_passes.get_mut(&this).unwrap();
                wgpu_native::wgpu_render_pass_draw(
                    render_pass,
                    vertex_count,
                    instance_count,
                    first_vertex,
                    first_instance
                );
            },

            Ok(WebGPUMessage::GPUCommandEncoderFinish { this, return_value, descriptor }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                // FIXME: hack because of other hacks
                for (_, rp) in state.render_passes.drain() {
                    wgpu_native::wgpu_render_pass_end_pass(rp);
                }
                let command_encoder = state.command_encoders.remove(&this).unwrap();
                let command_buffer = wgpu_native::wgpu_command_encoder_finish(command_encoder, None);
                state.command_buffers.insert(return_value, command_buffer);
            },

            Ok(WebGPUMessage::GPUQueueSubmit { this, command_buffers }) => {
                let state = active.as_mut().unwrap();     // TODO:
                assert_eq!(state.pid, emitter_pid);
                let command_buffers = command_buffers
                    .into_iter()
                    .map(|cb| state.command_buffers.remove(&cb).unwrap()) // TODO:
                    .collect::<Vec<_>>();
                let queue = state.devices.values_mut().next().unwrap().1;  // FIXME: hack
                wgpu_native::wgpu_queue_submit(
                    queue,
                    command_buffers.as_ptr(),
                    command_buffers.len(),
                );
            },

            Ok(msg) => unimplemented!("{:?}", msg),  // TODO: there are quite a few unimplemented messages
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

fn convert_texture_format(format: ffi::GPUTextureFormat) -> wgpu_core::resource::TextureFormat {
    match format {
        ffi::GPUTextureFormat::R8unorm => wgpu_core::resource::TextureFormat::R8Unorm,
        ffi::GPUTextureFormat::R8snorm => wgpu_core::resource::TextureFormat::R8Snorm,
        ffi::GPUTextureFormat::R8uint => wgpu_core::resource::TextureFormat::R8Uint,
        ffi::GPUTextureFormat::R8sint => wgpu_core::resource::TextureFormat::R8Sint,
        ffi::GPUTextureFormat::R16uint => wgpu_core::resource::TextureFormat::R16Uint,
        ffi::GPUTextureFormat::R16sint => wgpu_core::resource::TextureFormat::R16Sint,
        ffi::GPUTextureFormat::R16float => wgpu_core::resource::TextureFormat::R16Float,
        ffi::GPUTextureFormat::Rg8unorm => wgpu_core::resource::TextureFormat::Rg8Unorm,
        ffi::GPUTextureFormat::Rg8snorm => wgpu_core::resource::TextureFormat::Rg8Snorm,
        ffi::GPUTextureFormat::Rg8uint => wgpu_core::resource::TextureFormat::Rg8Uint,
        ffi::GPUTextureFormat::Rg8sint => wgpu_core::resource::TextureFormat::Rg8Sint,
        ffi::GPUTextureFormat::R32uint => wgpu_core::resource::TextureFormat::R32Uint,
        ffi::GPUTextureFormat::R32sint => wgpu_core::resource::TextureFormat::R32Sint,
        ffi::GPUTextureFormat::R32float => wgpu_core::resource::TextureFormat::R32Float,
        ffi::GPUTextureFormat::Rg16uint => wgpu_core::resource::TextureFormat::Rg16Uint,
        ffi::GPUTextureFormat::Rg16sint => wgpu_core::resource::TextureFormat::Rg16Sint,
        ffi::GPUTextureFormat::Rg16float => wgpu_core::resource::TextureFormat::Rg16Float,
        ffi::GPUTextureFormat::Rgba8unorm => wgpu_core::resource::TextureFormat::Rgba8Unorm,
        ffi::GPUTextureFormat::Rgba8unormSrgb => wgpu_core::resource::TextureFormat::Rgba8UnormSrgb,
        ffi::GPUTextureFormat::Rgba8snorm => wgpu_core::resource::TextureFormat::Rgba8Snorm,
        ffi::GPUTextureFormat::Rgba8uint => wgpu_core::resource::TextureFormat::Rgba8Uint,
        ffi::GPUTextureFormat::Rgba8sint => wgpu_core::resource::TextureFormat::Rgba8Sint,
        ffi::GPUTextureFormat::Bgra8unorm => wgpu_core::resource::TextureFormat::Bgra8Unorm,
        ffi::GPUTextureFormat::Bgra8unormSrgb => wgpu_core::resource::TextureFormat::Bgra8UnormSrgb,
        ffi::GPUTextureFormat::Rgb10a2unorm => wgpu_core::resource::TextureFormat::Rgb10a2Unorm,
        ffi::GPUTextureFormat::Rg11b10float => wgpu_core::resource::TextureFormat::Rg11b10Float,
        ffi::GPUTextureFormat::Rg32uint => wgpu_core::resource::TextureFormat::Rg32Uint,
        ffi::GPUTextureFormat::Rg32sint => wgpu_core::resource::TextureFormat::Rg32Sint,
        ffi::GPUTextureFormat::Rg32float => wgpu_core::resource::TextureFormat::Rg32Float,
        ffi::GPUTextureFormat::Rgba16uint => wgpu_core::resource::TextureFormat::Rgba16Uint,
        ffi::GPUTextureFormat::Rgba16sint => wgpu_core::resource::TextureFormat::Rgba16Sint,
        ffi::GPUTextureFormat::Rgba16float => wgpu_core::resource::TextureFormat::Rgba16Float,
        ffi::GPUTextureFormat::Rgba32uint => wgpu_core::resource::TextureFormat::Rgba32Uint,
        ffi::GPUTextureFormat::Rgba32sint => wgpu_core::resource::TextureFormat::Rgba32Sint,
        ffi::GPUTextureFormat::Rgba32float => wgpu_core::resource::TextureFormat::Rgba32Float,
        ffi::GPUTextureFormat::Depth32float => wgpu_core::resource::TextureFormat::Depth32Float,
        ffi::GPUTextureFormat::Depth24plus => wgpu_core::resource::TextureFormat::Depth24Plus,
        ffi::GPUTextureFormat::Depth24plusStencil8 => wgpu_core::resource::TextureFormat::Depth24PlusStencil8,
    }
}
