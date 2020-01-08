// Copyright (C) 2020  Pierre Krieger
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

use std::convert::TryFrom;

fn main() {
    redshirt_syscalls_interface::block_on(async_main());
}

async fn async_main() {
    let adapter: redshirt_webgpu_interface::GPUAdapter = unimplemented!(); // TODO: request
                                                                           /*redshirt_webgpu_interface::GPURequestAdapterOptions {
                                                                               power_preference: redshirt_webgpu_interface::GPUPowerPreference::LowPower,
                                                                           },*/

    let device = adapter
        .request_device(redshirt_webgpu_interface::GPUDeviceDescriptor {
            parent: redshirt_webgpu_interface::GPUObjectDescriptorBase {
                label: None,
            },
            extensions: None,
            limits: None,
        })
        .await;

    let queue = device.get_default_queue();

    let vs_module = {
        let vs = include_bytes!("shader.vert.spv");
        device.create_shader_module(
            &redshirt_webgpu_interface::GPUread_spirv(std::io::Cursor::new(&vs[..])).unwrap(),
        )
    };

    let fs_module = {
        let fs = include_bytes!("shader.frag.spv");
        device.create_shader_module(
            &redshirt_webgpu_interface::GPUread_spirv(std::io::Cursor::new(&fs[..])).unwrap(),
        )
    };

    let bind_group_layout =
        device.create_bind_group_layout(redshirt_webgpu_interface::GPUBindGroupLayoutDescriptor {
            parent: redshirt_webgpu_interface::GPUObjectDescriptorBase {
                label: None,
            },
            bindings: Vec::new(),
        });

    let bind_group = device.create_bind_group(redshirt_webgpu_interface::GPUBindGroupDescriptor {
        parent: redshirt_webgpu_interface::GPUObjectDescriptorBase {
            label: None,
        },
        layout: bind_group_layout,
        bindings: Vec::new(),
    });

    let pipeline_layout =
        device.create_pipeline_layout(redshirt_webgpu_interface::GPUPipelineLayoutDescriptor {
            parent: redshirt_webgpu_interface::GPUObjectDescriptorBase {
                label: None,
            },
            bind_group_layouts: vec![bind_group_layout],
        });

    let render_pipeline =
        device.create_render_pipeline(redshirt_webgpu_interface::GPURenderPipelineDescriptor {
            parent: redshirt_webgpu_interface::GPUPipelineDescriptorBase {
                parent: redshirt_webgpu_interface::GPUObjectDescriptorBase {
                    label: None,
                },
                layout: pipeline_layout,
            },
            vertex_stage: redshirt_webgpu_interface::GPUProgrammableStageDescriptor {
                module: vs_module,
                entry_point: "main".to_owned(),
            },
            fragment_stage: Some(redshirt_webgpu_interface::GPUProgrammableStageDescriptor {
                module: fs_module,
                entry_point: "main".to_owned(),
            }),
            primitive_topology: redshirt_webgpu_interface::GPUPrimitiveTopology::TriangleList,
            rasterization_state: Some(redshirt_webgpu_interface::GPURasterizationStateDescriptor {
                front_face: Some(redshirt_webgpu_interface::GPUFrontFace::Ccw),
                cull_mode: Some(redshirt_webgpu_interface::GPUCullMode::None),
                depth_bias: Some(0),
                depth_bias_slope_scale: Some(TryFrom::try_from(0.0).unwrap()),
                depth_bias_clamp: Some(TryFrom::try_from(0.0).unwrap()),
            }),
            color_states: vec![redshirt_webgpu_interface::GPUColorStateDescriptor {
                format: redshirt_webgpu_interface::GPUTextureFormat::Bgra8unormSrgb,
                color_blend: Some(redshirt_webgpu_interface::GPUBlendDescriptor {
                    src_factor: Some(redshirt_webgpu_interface::GPUBlendFactor::One),
                    dst_factor: Some(redshirt_webgpu_interface::GPUBlendFactor::Zero),
                    operation: Some(redshirt_webgpu_interface::GPUBlendOperation::Add),
                }),
                alpha_blend: Some(redshirt_webgpu_interface::GPUBlendDescriptor {
                    src_factor: Some(redshirt_webgpu_interface::GPUBlendFactor::One),
                    dst_factor: Some(redshirt_webgpu_interface::GPUBlendFactor::Zero),
                    operation: Some(redshirt_webgpu_interface::GPUBlendOperation::Add),
                }),
                write_mask: Some(0xf),
            }],
            depth_stencil_state: None,
            vertex_state: Some(redshirt_webgpu_interface::GPUVertexStateDescriptor {
                index_format: None,
                vertex_buffers: Some(Vec::new()),
            }),
            sample_count: Some(1),
            sample_mask: Some(!0),
            alpha_to_coverage_enabled: Some(false),
        });

    let mut swapchain: redshirt_webgpu_interface::GPUSwapChain = unimplemented!(); /* = configure_swap_chain(redshirt_webgpu_interface::GPUSwapChainDescriptor {
                                                                                       device,
                                                                                       format: redshirt_webgpu_interface::GPUTextureFormat::Bgra8unormSrgb,
                                                                                       usage: redshirt_webgpu_interface::GPUTextureUsage::OUTPUT_ATTACHMENT,
                                                                                   );*/

    loop {
        let texture = swapchain.get_current_texture();
        let view = texture.create_view(redshirt_webgpu_interface::GPUTextureViewDescriptor {
            parent: redshirt_webgpu_interface::GPUObjectDescriptorBase {
                label: None,
            },
            format: None,
            dimension: None,
            aspect: Some(redshirt_webgpu_interface::GPUTextureAspect::All),
            base_mip_level: None,
            mip_level_count: None,
            base_array_layer: None,
            array_layer_count: None,
        });

        let mut encoder = device
            .create_command_encoder(redshirt_webgpu_interface::GPUCommandEncoderDescriptor {
                parent: redshirt_webgpu_interface::GPUObjectDescriptorBase {
                    label: None,
                }
            });

        {
            let mut render_pass =
                encoder.begin_render_pass(redshirt_webgpu_interface::GPURenderPassDescriptor {
                    parent: redshirt_webgpu_interface::GPUObjectDescriptorBase {
                        label: None,
                    },
                    color_attachments: vec![
                        redshirt_webgpu_interface::GPURenderPassColorAttachmentDescriptor {
                            attachment: view,
                            resolve_target: None,
                            load_value: redshirt_webgpu_interface::GPUColor::GREEN,
                            store_op: Some(redshirt_webgpu_interface::GPUStoreOp::Store),
                        },
                    ],
                    depth_stencil_attachment: None,
                });
            render_pass.set_pipeline(render_pipeline);
            render_pass.set_bind_group(0, bind_group, vec![]);
            render_pass.draw(3, 1, 0, 0);
        }

        queue.submit(vec![encoder.finish(redshirt_webgpu_interface::GPUCommandBufferDescriptor {
            parent: redshirt_webgpu_interface::GPUObjectDescriptorBase {
                label: None,
            }
        })]);
    }
}
