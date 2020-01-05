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

fn main() {
    redshirt_syscalls_interface::block_on(async_main());
}

async fn async_main() {
    let adapter: redshirt_webgpu_interface::GPUAdapter = unimplemented!(); // TODO: request
                                                                           /*redshirt_webgpu_interface::GPURequestAdapterOptions {
                                                                               power_preference: redshirt_webgpu_interface::GPUPowerPreference::LowPower,
                                                                           },*/

    let device = adapter
        .request_device(&redshirt_webgpu_interface::GPUDeviceDescriptor {
            extensions: Vec::new(),
            limits: redshirt_webgpu_interface::GPULimits::default(),
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

    let render_pipeline =
        device.create_render_pipeline(&redshirt_webgpu_interface::GPURenderPipelineDescriptor {
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
                front_face: redshirt_webgpu_interface::GPUFrontFace::Ccw,
                cull_mode: redshirt_webgpu_interface::GPUCullMode::None,
                depth_bias: 0,
                depth_bias_slope_scale: 0.0,
                depth_bias_clamp: 0.0,
            }),
            color_states: vec![redshirt_webgpu_interface::GPUColorStateDescriptor {
                format: redshirt_webgpu_interface::GPUTextureFormat::Bgra8UnormSrgb,
                color_blend: redshirt_webgpu_interface::GPUBlendDescriptor {
                    src_factor: redshirt_webgpu_interface::GPUBlendFactor::One,
                    dst_factor: redshirt_webgpu_interface::GPUBlendFactor::Zero,
                    operation: redshirt_webgpu_interface::GPUBlendOperation::Add,
                },
                alpha_blend: redshirt_webgpu_interface::GPUBlendDescriptor {
                    src_factor: redshirt_webgpu_interface::GPUBlendFactor::One,
                    dst_factor: redshirt_webgpu_interface::GPUBlendFactor::Zero,
                    operation: redshirt_webgpu_interface::GPUBlendOperation::Add,
                },
                write_mask: 0xf,
            }],
            depth_stencil_state: redshirt_webgpu_interface::GPUDepthStencilStateDescriptor {
                format: redshirt_webgpu_interface::GPUTextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: redshirt_webgpu_interface::GPUCompareFunction::Always,
                stencil_front: redshirt_webgpu_interface::GPUStencilStateFaceDescriptor {
                    compare: redshirt_webgpu_interface::GPUCompareFunction::Always,
                    fail_op: redshirt_webgpu_interface::GPUStencilOperation::Keep,
                    depth_fail_op: redshirt_webgpu_interface::GPUStencilOperation::Keep,
                    pass_op: redshirt_webgpu_interface::GPUStencilOperation::Keep,
                },
                stencil_back: redshirt_webgpu_interface::GPUStencilStateFaceDescriptor {
                    compare: redshirt_webgpu_interface::GPUCompareFunction::Always,
                    fail_op: redshirt_webgpu_interface::GPUStencilOperation::Keep,
                    depth_fail_op: redshirt_webgpu_interface::GPUStencilOperation::Keep,
                    pass_op: redshirt_webgpu_interface::GPUStencilOperation::Keep,
                },
                stencil_read_mask: 0xffffffff,
                stencil_write_mask: 0xffffffff,
            },
            vertex_state: redshirt_webgpu_interface::GPUVertexStateDescriptor {
                index_format: redshirt_webgpu_interface::GPUIndexFormat::Uint16,
                vertex_buffers: Vec::new(),
            },
            sample_count: 1,
            sample_mask: !0,
            alpha_to_coverage_enabled: false,
        });

    let bind_group_layout =
        device.create_bind_group_layout(&redshirt_webgpu_interface::GPUBindGroupLayoutDescriptor {
            bindings: Vec::new(),
        });

    let bind_group = device.create_bind_group(&redshirt_webgpu_interface::GPUBindGroupDescriptor {
        layout: bind_group_layout,
        bindings: Vec::new(),
    });

    let pipeline_layout =
        device.create_pipeline_layout(&redshirt_webgpu_interface::GPUPipelineLayoutDescriptor {
            bind_group_layouts: vec![bind_group_layout],
        });

    let mut swapchain: redshirt_webgpu_interface::GPUSwapChain = unimplemented!(); /* = configure_swap_chain(redshirt_webgpu_interface::GPUSwapChainDescriptor {
                                                                                       device,
                                                                                       format: redshirt_webgpu_interface::GPUTextureFormat::Bgra8UnormSrgb,
                                                                                       usage: redshirt_webgpu_interface::GPUTextureUsage::OUTPUT_ATTACHMENT,
                                                                                   );*/

    loop {
        let texture = swapchain.get_current_texture();
        let view = texture.create_view(None);
        let mut encoder = device
            .create_command_encoder(&redshirt_webgpu_interface::GPUCommandEncoderDescriptor {});

        {
            let mut render_pass =
                encoder.begin_render_pass(&redshirt_webgpu_interface::GPURenderPassDescriptor {
                    color_attachments: vec![
                        redshirt_webgpu_interface::GPURenderPassColorAttachmentDescriptor {
                            attachment: view,
                            resolve_target: None,
                            load_value: redshirt_webgpu_interface::GPUColor::GREEN,
                            store_op: redshirt_webgpu_interface::GPUStoreOp::Store,
                        },
                    ],
                    depth_stencil_attachment: None,
                });
            render_pass.set_pipeline(render_pipeline);
            render_pass.set_bind_group(0, bind_group, vec![]);
            render_pass.draw(3, 1, 0, 0);
        }

        queue.submit(vec![encoder.finish()]);
    }
}
