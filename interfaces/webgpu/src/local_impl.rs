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

//! Local implementation of WebGPU functions that dispatch actual calls to a handler.

use crate::bindings;

use alloc::{string::String, vec::Vec};
use core::{convert::TryFrom, fmt, mem, ptr, slice, sync::atomic};
use futures::prelude::*;

/// Whenever we create a new object (e.g. a `GPUBuffer`), we decide locally of the ID of the
/// object and pass it to the interface implementer.
static NEXT_OBJECT_ID: atomic::AtomicU64 = atomic::AtomicU64::new(1);

#[no_mangle]
pub extern "C" fn wgpuCreateInstance(
    _: *const bindings::WGPUInstanceDescriptor,
) -> bindings::WGPUInstance {
    ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn wgpuGetProcAddress(
    device: bindings::WGPUDevice,
    proc_name: *const ::libc::c_char,
) -> bindings::WGPUProc {
    unsafe {
        // Note: we would like to use `CStr`, but it is not available in no_std contexts.
        let proc_name = {
            let len = (0..).find(|n| *proc_name.add(*n) == 0).unwrap();
            slice::from_raw_parts(proc_name as *const u8, len)
        };

        match proc_name {
            b"wgpuCreateInstance" => Some(mem::transmute(wgpuCreateInstance as *const ())),
            b"wgpuGetProcAddress" => Some(mem::transmute(wgpuGetProcAddress as *const ())),
            b"wgpuAdapterGetProperties" => {
                Some(mem::transmute(wgpuAdapterGetProperties as *const ()))
            }
            b"wgpuAdapterRequestDevice" => {
                Some(mem::transmute(wgpuAdapterRequestDevice as *const ()))
            }
            b"wgpuInstanceCreateSurface" => {
                Some(mem::transmute(wgpuInstanceCreateSurface as *const ()))
            }
            b"wgpuInstanceProcessEvents" => {
                Some(mem::transmute(wgpuInstanceProcessEvents as *const ()))
            }
            b"wgpuInstanceRequestAdapter" => {
                Some(mem::transmute(wgpuInstanceRequestAdapter as *const ()))
            }
            // TODO: do the rest
            _ => unimplemented!(),
        }
    }
}

pub extern "C" fn wgpuAdapterGetProperties(
    adapter: bindings::WGPUAdapter,
    properties: *mut bindings::WGPUAdapterProperties,
) {
    unimplemented!()
}

pub extern "C" fn wgpuAdapterRequestDevice(
    adapter: bindings::WGPUAdapter,
    descriptor: *const bindings::WGPUDeviceDescriptor,
    callback: bindings::WGPURequestDeviceCallback,
    userdata: *mut ::libc::c_void,
) {
    unimplemented!()
}

pub extern "C" fn wgpuBufferDestroy(buffer: bindings::WGPUBuffer) {
    unimplemented!()
}

pub extern "C" fn wgpuBufferMapReadAsync(
    buffer: bindings::WGPUBuffer,
    callback: bindings::WGPUBufferMapReadCallback,
    userdata: *mut ::libc::c_void,
) {
    unimplemented!()
}

pub extern "C" fn wgpuBufferMapWriteAsync(
    buffer: bindings::WGPUBuffer,
    callback: bindings::WGPUBufferMapWriteCallback,
    userdata: *mut ::libc::c_void,
) {
    unimplemented!()
}

pub extern "C" fn wgpuBufferUnmap(buffer: bindings::WGPUBuffer) {
    unimplemented!()
}

pub extern "C" fn wgpuCommandEncoderBeginComputePass(
    commandEncoder: bindings::WGPUCommandEncoder,
    descriptor: *const bindings::WGPUComputePassDescriptor,
) -> bindings::WGPUComputePassEncoder {
    unimplemented!()
}

pub extern "C" fn wgpuCommandEncoderBeginRenderPass(
    commandEncoder: bindings::WGPUCommandEncoder,
    descriptor: *const bindings::WGPURenderPassDescriptor,
) -> bindings::WGPURenderPassEncoder {
    unimplemented!()
}

pub extern "C" fn wgpuCommandEncoderCopyBufferToBuffer(
    commandEncoder: bindings::WGPUCommandEncoder,
    source: bindings::WGPUBuffer,
    sourceOffset: u64,
    destination: bindings::WGPUBuffer,
    destinationOffset: u64,
    size: u64,
) {
    unimplemented!()
}

pub extern "C" fn wgpuCommandEncoderCopyBufferToTexture(
    commandEncoder: bindings::WGPUCommandEncoder,
    source: *const bindings::WGPUBufferCopyView,
    destination: *const bindings::WGPUTextureCopyView,
    copySize: *const bindings::WGPUExtent3D,
) {
    unimplemented!()
}

pub extern "C" fn wgpuCommandEncoderCopyTextureToBuffer(
    commandEncoder: bindings::WGPUCommandEncoder,
    source: *const bindings::WGPUTextureCopyView,
    destination: *const bindings::WGPUBufferCopyView,
    copySize: *const bindings::WGPUExtent3D,
) {
    unimplemented!()
}

pub extern "C" fn wgpuCommandEncoderCopyTextureToTexture(
    commandEncoder: bindings::WGPUCommandEncoder,
    source: *const bindings::WGPUTextureCopyView,
    destination: *const bindings::WGPUTextureCopyView,
    copySize: *const bindings::WGPUExtent3D,
) {
    unimplemented!()
}

pub extern "C" fn wgpuCommandEncoderFinish(
    commandEncoder: bindings::WGPUCommandEncoder,
    descriptor: *const bindings::WGPUCommandBufferDescriptor,
) -> bindings::WGPUCommandBuffer {
    unimplemented!()
}

pub extern "C" fn wgpuCommandEncoderInsertDebugMarker(
    commandEncoder: bindings::WGPUCommandEncoder,
    groupLabel: *const ::libc::c_char,
) {
    unimplemented!()
}

pub extern "C" fn wgpuCommandEncoderPopDebugGroup(commandEncoder: bindings::WGPUCommandEncoder) {
    unimplemented!()
}

pub extern "C" fn wgpuCommandEncoderPushDebugGroup(
    commandEncoder: bindings::WGPUCommandEncoder,
    groupLabel: *const ::libc::c_char,
) {
    unimplemented!()
}

pub extern "C" fn wgpuComputePassEncoderDispatch(
    computePassEncoder: bindings::WGPUComputePassEncoder,
    x: u32,
    y: u32,
    z: u32,
) {
    unimplemented!()
}

pub extern "C" fn wgpuComputePassEncoderDispatchIndirect(
    computePassEncoder: bindings::WGPUComputePassEncoder,
    indirectBuffer: bindings::WGPUBuffer,
    indirectOffset: u64,
) {
    unimplemented!()
}

pub extern "C" fn wgpuComputePassEncoderEndPass(
    computePassEncoder: bindings::WGPUComputePassEncoder,
) {
    unimplemented!()
}

pub extern "C" fn wgpuComputePassEncoderInsertDebugMarker(
    computePassEncoder: bindings::WGPUComputePassEncoder,
    groupLabel: *const ::libc::c_char,
) {
    unimplemented!()
}

pub extern "C" fn wgpuComputePassEncoderPopDebugGroup(
    computePassEncoder: bindings::WGPUComputePassEncoder,
) {
    unimplemented!()
}

pub extern "C" fn wgpuComputePassEncoderPushDebugGroup(
    computePassEncoder: bindings::WGPUComputePassEncoder,
    groupLabel: *const ::libc::c_char,
) {
    unimplemented!()
}

pub extern "C" fn wgpuComputePassEncoderSetBindGroup(
    computePassEncoder: bindings::WGPUComputePassEncoder,
    groupIndex: u32,
    group: bindings::WGPUBindGroup,
    dynamicOffsetCount: u32,
    dynamicOffsets: *const u32,
) {
    unimplemented!()
}

pub extern "C" fn wgpuComputePassEncoderSetPipeline(
    computePassEncoder: bindings::WGPUComputePassEncoder,
    pipeline: bindings::WGPUComputePipeline,
) {
    unimplemented!()
}

pub extern "C" fn wgpuComputePipelineGetBindGroupLayout(
    computePipeline: bindings::WGPUComputePipeline,
    groupIndex: u32,
) -> bindings::WGPUBindGroupLayout {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateBindGroup(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPUBindGroupDescriptor,
) -> bindings::WGPUBindGroup {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateBindGroupLayout(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPUBindGroupLayoutDescriptor,
) -> bindings::WGPUBindGroupLayout {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateBuffer(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPUBufferDescriptor,
) -> bindings::WGPUBuffer {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateBufferMapped(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPUBufferDescriptor,
) -> bindings::WGPUCreateBufferMappedResult {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateBufferMappedAsync(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPUBufferDescriptor,
    callback: bindings::WGPUBufferCreateMappedCallback,
    userdata: *mut ::libc::c_void,
) {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateCommandEncoder(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPUCommandEncoderDescriptor,
) -> bindings::WGPUCommandEncoder {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateComputePipeline(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPUComputePipelineDescriptor,
) -> bindings::WGPUComputePipeline {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreatePipelineLayout(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPUPipelineLayoutDescriptor,
) -> bindings::WGPUPipelineLayout {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateQueue(device: bindings::WGPUDevice) -> bindings::WGPUQueue {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateRenderBundleEncoder(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPURenderBundleEncoderDescriptor,
) -> bindings::WGPURenderBundleEncoder {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateRenderPipeline(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPURenderPipelineDescriptor,
) -> bindings::WGPURenderPipeline {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateSampler(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPUSamplerDescriptor,
) -> bindings::WGPUSampler {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateShaderModule(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPUShaderModuleDescriptor,
) -> bindings::WGPUShaderModule {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateSwapChain(
    device: bindings::WGPUDevice,
    surface: bindings::WGPUSurface,
    descriptor: *const bindings::WGPUSwapChainDescriptor,
) -> bindings::WGPUSwapChain {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceCreateTexture(
    device: bindings::WGPUDevice,
    descriptor: *const bindings::WGPUTextureDescriptor,
) -> bindings::WGPUTexture {
    unimplemented!()
}

pub extern "C" fn wgpuDevicePopErrorScope(
    device: bindings::WGPUDevice,
    callback: bindings::WGPUErrorCallback,
    userdata: *mut ::libc::c_void,
) -> bool {
    unimplemented!()
}

pub extern "C" fn wgpuDevicePushErrorScope(
    device: bindings::WGPUDevice,
    filter: bindings::WGPUErrorFilter,
) {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceSetDeviceLostCallback(
    device: bindings::WGPUDevice,
    callback: bindings::WGPUDeviceLostCallback,
    userdata: *mut ::libc::c_void,
) {
    unimplemented!()
}

pub extern "C" fn wgpuDeviceSetUncapturedErrorCallback(
    device: bindings::WGPUDevice,
    callback: bindings::WGPUErrorCallback,
    userdata: *mut ::libc::c_void,
) {
    unimplemented!()
}

pub extern "C" fn wgpuFenceGetCompletedValue(fence: bindings::WGPUFence) -> u64 {
    unimplemented!()
}

pub extern "C" fn wgpuFenceOnCompletion(
    fence: bindings::WGPUFence,
    value: u64,
    callback: bindings::WGPUFenceOnCompletionCallback,
    userdata: *mut ::libc::c_void,
) {
    unimplemented!()
}

pub extern "C" fn wgpuInstanceCreateSurface(
    _: bindings::WGPUInstance,
    _: *const bindings::WGPUSurfaceDescriptor,
) -> bindings::WGPUSurface {
    unimplemented!()
}

pub extern "C" fn wgpuInstanceProcessEvents(_: bindings::WGPUInstance) {}

pub extern "C" fn wgpuInstanceRequestAdapter(
    _: bindings::WGPUInstance,
    _: *const bindings::WGPUAdapterDescriptor,
    callback: bindings::WGPURequestAdapterCallback,
    userdata: *mut ::libc::c_void,
) {
    unimplemented!()
    // TODO: callback(adapter, userdata)
}

pub extern "C" fn wgpuQueueCreateFence(
    queue: bindings::WGPUQueue,
    descriptor: *const bindings::WGPUFenceDescriptor,
) -> bindings::WGPUFence {
    unimplemented!()
}

pub extern "C" fn wgpuQueueSignal(
    queue: bindings::WGPUQueue,
    fence: bindings::WGPUFence,
    signalValue: u64,
) {
    unimplemented!()
}

pub extern "C" fn wgpuQueueSubmit(
    queue: bindings::WGPUQueue,
    commandCount: u32,
    commands: *const bindings::WGPUCommandBuffer,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderBundleEncoderDraw(
    renderBundleEncoder: bindings::WGPURenderBundleEncoder,
    vertexCount: u32,
    instanceCount: u32,
    firstVertex: u32,
    firstInstance: u32,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderBundleEncoderDrawIndexed(
    renderBundleEncoder: bindings::WGPURenderBundleEncoder,
    indexCount: u32,
    instanceCount: u32,
    firstIndex: u32,
    baseVertex: i32,
    firstInstance: u32,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderBundleEncoderDrawIndexedIndirect(
    renderBundleEncoder: bindings::WGPURenderBundleEncoder,
    indirectBuffer: bindings::WGPUBuffer,
    indirectOffset: u64,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderBundleEncoderDrawIndirect(
    renderBundleEncoder: bindings::WGPURenderBundleEncoder,
    indirectBuffer: bindings::WGPUBuffer,
    indirectOffset: u64,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderBundleEncoderFinish(
    renderBundleEncoder: bindings::WGPURenderBundleEncoder,
    descriptor: *const bindings::WGPURenderBundleDescriptor,
) -> bindings::WGPURenderBundle {
    unimplemented!()
}

pub extern "C" fn wgpuRenderBundleEncoderInsertDebugMarker(
    renderBundleEncoder: bindings::WGPURenderBundleEncoder,
    groupLabel: *const ::libc::c_char,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderBundleEncoderPopDebugGroup(
    renderBundleEncoder: bindings::WGPURenderBundleEncoder,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderBundleEncoderPushDebugGroup(
    renderBundleEncoder: bindings::WGPURenderBundleEncoder,
    groupLabel: *const ::libc::c_char,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderBundleEncoderSetBindGroup(
    renderBundleEncoder: bindings::WGPURenderBundleEncoder,
    groupIndex: u32,
    group: bindings::WGPUBindGroup,
    dynamicOffsetCount: u32,
    dynamicOffsets: *const u32,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderBundleEncoderSetIndexBuffer(
    renderBundleEncoder: bindings::WGPURenderBundleEncoder,
    buffer: bindings::WGPUBuffer,
    offset: u64,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderBundleEncoderSetPipeline(
    renderBundleEncoder: bindings::WGPURenderBundleEncoder,
    pipeline: bindings::WGPURenderPipeline,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderBundleEncoderSetVertexBuffer(
    renderBundleEncoder: bindings::WGPURenderBundleEncoder,
    slot: u32,
    buffer: bindings::WGPUBuffer,
    offset: u64,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderDraw(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    vertexCount: u32,
    instanceCount: u32,
    firstVertex: u32,
    firstInstance: u32,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderDrawIndexed(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    indexCount: u32,
    instanceCount: u32,
    firstIndex: u32,
    baseVertex: i32,
    firstInstance: u32,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderDrawIndexedIndirect(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    indirectBuffer: bindings::WGPUBuffer,
    indirectOffset: u64,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderDrawIndirect(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    indirectBuffer: bindings::WGPUBuffer,
    indirectOffset: u64,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderEndPass(renderPassEncoder: bindings::WGPURenderPassEncoder) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderExecuteBundles(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    bundlesCount: u32,
    bundles: *const bindings::WGPURenderBundle,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderInsertDebugMarker(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    groupLabel: *const ::libc::c_char,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderPopDebugGroup(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderPushDebugGroup(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    groupLabel: *const ::libc::c_char,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderSetBindGroup(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    groupIndex: u32,
    group: bindings::WGPUBindGroup,
    dynamicOffsetCount: u32,
    dynamicOffsets: *const u32,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderSetBlendColor(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    color: *const bindings::WGPUColor,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderSetIndexBuffer(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    buffer: bindings::WGPUBuffer,
    offset: u64,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderSetPipeline(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    pipeline: bindings::WGPURenderPipeline,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderSetScissorRect(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderSetStencilReference(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    reference: u32,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderSetVertexBuffer(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    slot: u32,
    buffer: bindings::WGPUBuffer,
    offset: u64,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPassEncoderSetViewport(
    renderPassEncoder: bindings::WGPURenderPassEncoder,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    minDepth: f32,
    maxDepth: f32,
) {
    unimplemented!()
}

pub extern "C" fn wgpuRenderPipelineGetBindGroupLayout(
    renderPipeline: bindings::WGPURenderPipeline,
    groupIndex: u32,
) -> bindings::WGPUBindGroupLayout {
    unimplemented!()
}

pub extern "C" fn wgpuSurfaceGetPreferredFormat(
    surface: bindings::WGPUSurface,
    adapter: bindings::WGPUAdapter,
    callback: bindings::WGPUSurfaceGetPreferredFormatCallback,
    userdata: *mut ::libc::c_void,
) {
    unimplemented!()
}

pub extern "C" fn wgpuSwapChainGetCurrentTextureView(
    swapChain: bindings::WGPUSwapChain,
) -> bindings::WGPUTextureView {
    unimplemented!()
}

pub extern "C" fn wgpuSwapChainPresent(swapChain: bindings::WGPUSwapChain) {
    unimplemented!()
}

pub extern "C" fn wgpuTextureCreateView(
    texture: bindings::WGPUTexture,
    descriptor: *const bindings::WGPUTextureViewDescriptor,
) -> bindings::WGPUTextureView {
    unimplemented!()
}

pub extern "C" fn wgpuTextureDestroy(texture: bindings::WGPUTexture) {
    unimplemented!()
}
