// Command encoders, command buffers and render passes.

use core::ops::Range;

use wgpu::custom::*;

use crate::conv;
use crate::handle::*;
use crate::resource::*;
use crate::sys::*;

/// A `wgpu::CommandEncoder` backend over a WGPUCommandEncoder.
#[derive(Debug)]
pub struct CommandEncoder {
    pub(crate) raw: CommandEncoderHandle,
}

/// A `wgpu::CommandBuffer` backend over a WGPUCommandBuffer.
#[derive(Debug)]
pub struct CommandBuffer {
    pub(crate) raw: CommandBufferHandle,
}
impl CommandBufferInterface for CommandBuffer {}

/// A `wgpu::RenderPass` backend over a WGPURenderPassEncoder; ended on drop.
#[derive(Debug)]
pub struct RenderPass {
    raw: RenderPassHandle,
}

native_send_sync!(CommandEncoder, CommandBuffer, RenderPass);

fn depth_load<V: Copy>(ops: &Option<wgpu::Operations<V>>, dont_care: V) -> (WGPULoadOp, WGPUStoreOp, Option<V>) {
    match ops {
        None => (WGPULoadOp_Undefined, WGPUStoreOp_Undefined, None),
        Some(ops) => {
            let (load, value) = load_op(ops.load, dont_care);
            (load, store_op(ops.store), value)
        }
    }
}

/// WebGPU has no "don't care" load: like wgpu's WebGPU backend, clear.
fn load_op<V: Copy>(load: wgpu::LoadOp<V>, dont_care: V) -> (WGPULoadOp, Option<V>) {
    match load {
        wgpu::LoadOp::Clear(value) => (WGPULoadOp_Clear, Some(value)),
        wgpu::LoadOp::Load => (WGPULoadOp_Load, None),
        #[allow(unreachable_patterns)]
        _ => (WGPULoadOp_Clear, Some(dont_care)),
    }
}

fn store_op(store: wgpu::StoreOp) -> WGPUStoreOp {
    match store {
        wgpu::StoreOp::Store => WGPUStoreOp_Store,
        wgpu::StoreOp::Discard => WGPUStoreOp_Discard,
    }
}

impl CommandEncoderInterface for CommandEncoder {
    fn copy_buffer_to_buffer(
        &self,
        source: &DispatchBuffer,
        source_offset: wgpu::BufferAddress,
        destination: &DispatchBuffer,
        destination_offset: wgpu::BufferAddress,
        copy_size: Option<wgpu::BufferAddress>,
    ) {
        let source = dispatch_buffer(source);
        let size = copy_size.unwrap_or_else(|| source.size.saturating_sub(source_offset));
        unsafe {
            wgpuCommandEncoderCopyBufferToBuffer(
                self.raw.raw(),
                source.raw.raw(),
                source_offset,
                dispatch_buffer(destination).raw.raw(),
                destination_offset,
                size,
            );
        }
    }

    fn copy_buffer_to_texture(
        &self,
        source: wgpu::TexelCopyBufferInfo<'_>,
        destination: wgpu::TexelCopyTextureInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        let source = conv::texel_copy_buffer_info(&source);
        let destination = conv::texel_copy_texture_info(&destination);
        let size = conv::extent(copy_size);
        unsafe { wgpuCommandEncoderCopyBufferToTexture(self.raw.raw(), &source, &destination, &size) };
    }

    fn copy_texture_to_buffer(
        &self,
        source: wgpu::TexelCopyTextureInfo<'_>,
        destination: wgpu::TexelCopyBufferInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        let source = conv::texel_copy_texture_info(&source);
        let destination = conv::texel_copy_buffer_info(&destination);
        let size = conv::extent(copy_size);
        unsafe { wgpuCommandEncoderCopyTextureToBuffer(self.raw.raw(), &source, &destination, &size) };
    }

    fn copy_texture_to_texture(
        &self,
        source: wgpu::TexelCopyTextureInfo<'_>,
        destination: wgpu::TexelCopyTextureInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        let source = conv::texel_copy_texture_info(&source);
        let destination = conv::texel_copy_texture_info(&destination);
        let size = conv::extent(copy_size);
        unsafe { wgpuCommandEncoderCopyTextureToTexture(self.raw.raw(), &source, &destination, &size) };
    }

    fn begin_compute_pass(&self, _desc: &wgpu::ComputePassDescriptor<'_>) -> DispatchComputePass {
        unimplemented!("mln_wgpu_webgpu_h: compute passes")
    }

    fn begin_render_pass(&self, desc: &wgpu::RenderPassDescriptor<'_>) -> DispatchRenderPass {
        if desc.timestamp_writes.is_some() || desc.occlusion_query_set.is_some() || desc.multiview_mask.is_some() {
            unimplemented!("mln_wgpu_webgpu_h: render pass queries and multiview");
        }
        let color_attachments: Vec<WGPURenderPassColorAttachment> = desc
            .color_attachments
            .iter()
            .map(|attachment| match attachment {
                // A null view is an unused slot.
                None => WGPURenderPassColorAttachment {
                    nextInChain: core::ptr::null_mut(),
                    view: core::ptr::null_mut(),
                    depthSlice: WGPU_DEPTH_SLICE_UNDEFINED,
                    resolveTarget: core::ptr::null_mut(),
                    loadOp: WGPULoadOp_Undefined,
                    storeOp: WGPUStoreOp_Undefined,
                    clearValue: WGPUColor { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
                },
                Some(ca) => {
                    let (load, clear) = load_op(ca.ops.load, wgpu::Color::BLACK);
                    WGPURenderPassColorAttachment {
                        nextInChain: core::ptr::null_mut(),
                        view: texture_view(ca.view).raw.raw(),
                        depthSlice: ca.depth_slice.unwrap_or(WGPU_DEPTH_SLICE_UNDEFINED),
                        resolveTarget: ca.resolve_target.map_or(core::ptr::null_mut(), |t| texture_view(t).raw.raw()),
                        loadOp: load,
                        storeOp: store_op(ca.ops.store),
                        clearValue: conv::color(clear.unwrap_or(wgpu::Color::TRANSPARENT)),
                    }
                }
            })
            .collect();
        let depth_stencil = desc.depth_stencil_attachment.as_ref().map(|ds| {
            let (depth_load, depth_store, depth_clear) = depth_load(&ds.depth_ops, 1.0);
            let (stencil_load, stencil_store, stencil_clear) = depth_load_u32(&ds.stencil_ops);
            WGPURenderPassDepthStencilAttachment {
                nextInChain: core::ptr::null_mut(),
                view: texture_view(ds.view).raw.raw(),
                depthLoadOp: depth_load,
                depthStoreOp: depth_store,
                // Unused unless clearing, but emdawnwebgpu hands it to the
                // browser as is, which rejects the NaN of
                // WGPU_DEPTH_CLEAR_VALUE_UNDEFINED.
                depthClearValue: depth_clear.unwrap_or(0.0),
                // As wgpu's WebGPU backend: no ops means a read-only aspect.
                depthReadOnly: to_bool(ds.depth_ops.is_none()),
                stencilLoadOp: stencil_load,
                stencilStoreOp: stencil_store,
                stencilClearValue: stencil_clear.unwrap_or(0),
                stencilReadOnly: to_bool(ds.stencil_ops.is_none()),
            }
        });
        let raw_desc = WGPURenderPassDescriptor {
            nextInChain: core::ptr::null_mut(),
            label: label(desc.label),
            colorAttachmentCount: color_attachments.len(),
            colorAttachments: color_attachments.as_ptr(),
            depthStencilAttachment: depth_stencil.as_ref().map_or(core::ptr::null(), core::ptr::from_ref),
            occlusionQuerySet: core::ptr::null_mut(),
            timestampWrites: core::ptr::null(),
        };
        let raw = unsafe { RenderPassHandle::from_raw(wgpuCommandEncoderBeginRenderPass(self.raw.raw(), &raw_desc)) };
        DispatchRenderPass::custom(RenderPass { raw: created(raw, "render pass") })
    }

    fn finish(&mut self) -> DispatchCommandBuffer {
        let raw = unsafe { CommandBufferHandle::from_raw(wgpuCommandEncoderFinish(self.raw.raw(), core::ptr::null())) };
        DispatchCommandBuffer::custom(CommandBuffer { raw: created(raw, "command buffer") })
    }

    fn clear_texture(&self, _texture: &DispatchTexture, _subresource_range: &wgpu::ImageSubresourceRange) {
        unimplemented!("mln_wgpu_webgpu_h: clear_texture (not in WebGPU)")
    }

    fn clear_buffer(&self, buffer: &DispatchBuffer, offset: wgpu::BufferAddress, size: Option<wgpu::BufferAddress>) {
        let buffer = dispatch_buffer(buffer);
        let size = size.unwrap_or_else(|| buffer.size.saturating_sub(offset));
        unsafe { wgpuCommandEncoderClearBuffer(self.raw.raw(), buffer.raw.raw(), offset, size) };
    }

    fn insert_debug_marker(&self, label: &str) {
        unsafe { wgpuCommandEncoderInsertDebugMarker(self.raw.raw(), string_view(label)) };
    }

    fn push_debug_group(&self, label: &str) {
        unsafe { wgpuCommandEncoderPushDebugGroup(self.raw.raw(), string_view(label)) };
    }

    fn pop_debug_group(&self) {
        unsafe { wgpuCommandEncoderPopDebugGroup(self.raw.raw()) };
    }

    fn write_timestamp(&self, _query_set: &DispatchQuerySet, _query_index: u32) {
        unimplemented!("mln_wgpu_webgpu_h: write_timestamp")
    }

    fn resolve_query_set(
        &self,
        _query_set: &DispatchQuerySet,
        _first_query: u32,
        _query_count: u32,
        _destination: &DispatchBuffer,
        _destination_offset: wgpu::BufferAddress,
    ) {
        unimplemented!("mln_wgpu_webgpu_h: resolve_query_set")
    }

    fn mark_acceleration_structures_built<'a>(
        &self,
        _blas: &mut dyn Iterator<Item = &'a wgpu::Blas>,
        _tlas: &mut dyn Iterator<Item = &'a wgpu::Tlas>,
    ) {
        unimplemented!("mln_wgpu_webgpu_h: acceleration structures")
    }

    fn build_acceleration_structures<'a>(
        &self,
        _blas: &mut dyn Iterator<Item = &'a wgpu::BlasBuildEntry<'a>>,
        _tlas: &mut dyn Iterator<Item = &'a wgpu::Tlas>,
    ) {
        unimplemented!("mln_wgpu_webgpu_h: acceleration structures")
    }

    fn transition_resources<'a>(
        &mut self,
        _buffer_transitions: &mut dyn Iterator<Item = wgpu::wgt::BufferTransition<&'a DispatchBuffer>>,
        _texture_transitions: &mut dyn Iterator<Item = wgpu::wgt::TextureTransition<&'a DispatchTexture>>,
    ) {
        // WebGPU tracks resource states itself.
    }
}

fn depth_load_u32(ops: &Option<wgpu::Operations<u32>>) -> (WGPULoadOp, WGPUStoreOp, Option<u32>) {
    depth_load(ops, 0)
}

impl Drop for RenderPass {
    fn drop(&mut self) {
        unsafe { wgpuRenderPassEncoderEnd(self.raw.raw()) };
    }
}

impl RenderPassInterface for RenderPass {
    fn set_pipeline(&mut self, pipeline: &DispatchRenderPipeline) {
        unsafe { wgpuRenderPassEncoderSetPipeline(self.raw.raw(), dispatch_render_pipeline(pipeline).raw.raw()) };
    }

    fn set_bind_group(&mut self, index: u32, bind_group: Option<&DispatchBindGroup>, offsets: &[wgpu::DynamicOffset]) {
        let group = bind_group.map_or(core::ptr::null_mut(), |g| dispatch_bind_group(g).raw.raw());
        unsafe { wgpuRenderPassEncoderSetBindGroup(self.raw.raw(), index, group, offsets.len(), offsets.as_ptr()) };
    }

    fn set_index_buffer(
        &mut self,
        buffer: &DispatchBuffer,
        index_format: wgpu::IndexFormat,
        offset: wgpu::BufferAddress,
        size: Option<wgpu::BufferSize>,
    ) {
        unsafe {
            wgpuRenderPassEncoderSetIndexBuffer(
                self.raw.raw(),
                dispatch_buffer(buffer).raw.raw(),
                conv::index_format(index_format),
                offset,
                size.map_or(WGPU_WHOLE_SIZE, |s| s.get()),
            );
        }
    }

    fn set_vertex_buffer(
        &mut self,
        slot: u32,
        buffer: Option<&DispatchBuffer>,
        offset: wgpu::BufferAddress,
        size: Option<wgpu::BufferSize>,
    ) {
        let buffer = buffer.map_or(core::ptr::null_mut(), |b| dispatch_buffer(b).raw.raw());
        let size = size.map_or(WGPU_WHOLE_SIZE, |s| s.get());
        unsafe { wgpuRenderPassEncoderSetVertexBuffer(self.raw.raw(), slot, buffer, offset, size) };
    }

    fn set_immediates(&mut self, _offset: u32, _data: &[u8]) {
        unimplemented!("mln_wgpu_webgpu_h: immediates")
    }

    fn set_blend_constant(&mut self, color: wgpu::Color) {
        let color = conv::color(color);
        unsafe { wgpuRenderPassEncoderSetBlendConstant(self.raw.raw(), &color) };
    }

    fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        unsafe { wgpuRenderPassEncoderSetScissorRect(self.raw.raw(), x, y, width, height) };
    }

    fn set_viewport(&mut self, x: f32, y: f32, width: f32, height: f32, min_depth: f32, max_depth: f32) {
        unsafe { wgpuRenderPassEncoderSetViewport(self.raw.raw(), x, y, width, height, min_depth, max_depth) };
    }

    fn set_stencil_reference(&mut self, reference: u32) {
        unsafe { wgpuRenderPassEncoderSetStencilReference(self.raw.raw(), reference) };
    }

    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        unsafe {
            wgpuRenderPassEncoderDraw(
                self.raw.raw(),
                vertices.end - vertices.start,
                instances.end - instances.start,
                vertices.start,
                instances.start,
            );
        }
    }

    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        unsafe {
            wgpuRenderPassEncoderDrawIndexed(
                self.raw.raw(),
                indices.end - indices.start,
                instances.end - instances.start,
                indices.start,
                base_vertex,
                instances.start,
            );
        }
    }

    fn draw_mesh_tasks(&mut self, _x: u32, _y: u32, _z: u32) {
        unimplemented!("mln_wgpu_webgpu_h: mesh shaders")
    }

    fn draw_indirect(&mut self, indirect_buffer: &DispatchBuffer, indirect_offset: wgpu::BufferAddress) {
        let buffer = dispatch_buffer(indirect_buffer).raw.raw();
        unsafe { wgpuRenderPassEncoderDrawIndirect(self.raw.raw(), buffer, indirect_offset) };
    }

    fn draw_indexed_indirect(&mut self, indirect_buffer: &DispatchBuffer, indirect_offset: wgpu::BufferAddress) {
        let buffer = dispatch_buffer(indirect_buffer).raw.raw();
        unsafe { wgpuRenderPassEncoderDrawIndexedIndirect(self.raw.raw(), buffer, indirect_offset) };
    }

    fn draw_mesh_tasks_indirect(&mut self, _indirect_buffer: &DispatchBuffer, _indirect_offset: wgpu::BufferAddress) {
        unimplemented!("mln_wgpu_webgpu_h: mesh shaders")
    }

    fn multi_draw_indirect(&mut self, _buffer: &DispatchBuffer, _offset: wgpu::BufferAddress, _count: u32) {
        unimplemented!("mln_wgpu_webgpu_h: multi_draw_indirect")
    }

    fn multi_draw_indexed_indirect(&mut self, _buffer: &DispatchBuffer, _offset: wgpu::BufferAddress, _count: u32) {
        unimplemented!("mln_wgpu_webgpu_h: multi_draw_indexed_indirect")
    }

    fn multi_draw_indirect_count(
        &mut self,
        _buffer: &DispatchBuffer,
        _offset: wgpu::BufferAddress,
        _count_buffer: &DispatchBuffer,
        _count_buffer_offset: wgpu::BufferAddress,
        _max_count: u32,
    ) {
        unimplemented!("mln_wgpu_webgpu_h: multi_draw_indirect_count")
    }

    fn multi_draw_mesh_tasks_indirect(&mut self, _buffer: &DispatchBuffer, _offset: wgpu::BufferAddress, _count: u32) {
        unimplemented!("mln_wgpu_webgpu_h: mesh shaders")
    }

    fn multi_draw_indexed_indirect_count(
        &mut self,
        _buffer: &DispatchBuffer,
        _offset: wgpu::BufferAddress,
        _count_buffer: &DispatchBuffer,
        _count_buffer_offset: wgpu::BufferAddress,
        _max_count: u32,
    ) {
        unimplemented!("mln_wgpu_webgpu_h: multi_draw_indexed_indirect_count")
    }

    fn multi_draw_mesh_tasks_indirect_count(
        &mut self,
        _buffer: &DispatchBuffer,
        _offset: wgpu::BufferAddress,
        _count_buffer: &DispatchBuffer,
        _count_buffer_offset: wgpu::BufferAddress,
        _max_count: u32,
    ) {
        unimplemented!("mln_wgpu_webgpu_h: mesh shaders")
    }

    fn insert_debug_marker(&mut self, label: &str) {
        unsafe { wgpuRenderPassEncoderInsertDebugMarker(self.raw.raw(), string_view(label)) };
    }

    fn push_debug_group(&mut self, group_label: &str) {
        unsafe { wgpuRenderPassEncoderPushDebugGroup(self.raw.raw(), string_view(group_label)) };
    }

    fn pop_debug_group(&mut self) {
        unsafe { wgpuRenderPassEncoderPopDebugGroup(self.raw.raw()) };
    }

    fn write_timestamp(&mut self, _query_set: &DispatchQuerySet, _query_index: u32) {
        unimplemented!("mln_wgpu_webgpu_h: write_timestamp")
    }

    fn begin_occlusion_query(&mut self, _query_index: u32) {
        unimplemented!("mln_wgpu_webgpu_h: occlusion queries")
    }

    fn end_occlusion_query(&mut self) {
        unimplemented!("mln_wgpu_webgpu_h: occlusion queries")
    }

    fn begin_pipeline_statistics_query(&mut self, _query_set: &DispatchQuerySet, _query_index: u32) {
        unimplemented!("mln_wgpu_webgpu_h: pipeline statistics queries")
    }

    fn end_pipeline_statistics_query(&mut self) {
        unimplemented!("mln_wgpu_webgpu_h: pipeline statistics queries")
    }

    fn execute_bundles(&mut self, render_bundles: &mut dyn Iterator<Item = &DispatchRenderBundle>) {
        if render_bundles.next().is_some() {
            unimplemented!("mln_wgpu_webgpu_h: render bundles");
        }
    }
}
