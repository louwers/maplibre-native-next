// Buffers, textures, views, samplers, shader modules, bind groups and
// pipelines.

use core::ops::Range;
use core::ptr::NonNull;
use std::cell::Cell;
use std::pin::Pin;
use std::rc::Rc;

use wgpu::custom::*;

use crate::device::DeviceState;
use crate::event::{self, MessageResult, slot, take, userdata};
use crate::handle::*;
use crate::sys::*;

/// The object behind a handle webgpu.h just returned; null only happens when
/// the browser rejects the call outright (e.g. an impossible allocation).
#[track_caller]
pub(crate) fn created<T>(handle: Option<T>, what: &str) -> T {
    handle.unwrap_or_else(|| panic!("mln_wgpu_webgpu_h: creating a {what} returned null"))
}

macro_rules! downcast {
    ($($fn:ident($api:ty) -> $ty:ty;)*) => {$(
        #[track_caller]
        pub(crate) fn $fn(object: &$api) -> &$ty {
            object
                .as_custom::<$ty>()
                .expect(concat!("mln_wgpu_webgpu_h: ", stringify!($api), " from another wgpu backend"))
        }
    )*};
}

downcast! {
    buffer(wgpu::Buffer) -> Buffer;
    dispatch_buffer(DispatchBuffer) -> Buffer;
    texture(wgpu::Texture) -> Texture;
    texture_view(wgpu::TextureView) -> TextureView;
    sampler(wgpu::Sampler) -> Sampler;
    shader_module(wgpu::ShaderModule) -> ShaderModule;
    bind_group_layout(wgpu::BindGroupLayout) -> BindGroupLayout;
    dispatch_bind_group(DispatchBindGroup) -> BindGroup;
    pipeline_layout(wgpu::PipelineLayout) -> PipelineLayout;
    dispatch_render_pipeline(DispatchRenderPipeline) -> RenderPipeline;
}

/// A `wgpu::Buffer` backend over a WGPUBuffer.
#[derive(Debug)]
pub struct Buffer {
    pub(crate) raw: BufferHandle,
    pub(crate) size: u64,
    device: Rc<DeviceState>,
    /// The mode of the current (or pending) mapping, to pick the read or the
    /// write mapped-range call.
    map_mode: Cell<WGPUMapMode>,
}

impl Buffer {
    pub(crate) fn new(raw: BufferHandle, desc: &wgpu::BufferDescriptor<'_>, device: Rc<DeviceState>) -> Self {
        let map_mode = if desc.mapped_at_creation { WGPUMapMode_Write } else { WGPUMapMode_None };
        Buffer { raw, size: desc.size, device, map_mode: Cell::new(map_mode) }
    }
}

/// Releasing a WGPUBuffer only drops emdawnwebgpu's reference to the
/// JavaScript GPUBuffer, and the browser frees its memory when the garbage
/// collector gets to it. A map that keeps replacing buffers can run the GPU
/// out of memory first. So a dropped buffer is destroyed, which frees it once
/// submitted work no longer needs it. That waits for the next submit: wgpu
/// lets a buffer be dropped after a command encoder recorded it and before
/// that encoder's commands are submitted.
impl Drop for Buffer {
    fn drop(&mut self) {
        self.device.dropped_buffers.borrow_mut().push(self.raw.clone());
    }
}

impl BufferInterface for Buffer {
    fn map_async(&self, mode: wgpu::MapMode, range: Range<wgpu::BufferAddress>, callback: BufferMapCallback) {
        let mode = crate::conv::map_mode(mode);
        self.map_mode.set(mode);
        let result = slot::<MessageResult<WGPUMapAsyncStatus>>();
        let info = WGPUBufferMapCallbackInfo {
            nextInChain: core::ptr::null_mut(),
            mode: WGPUCallbackMode_WaitAnyOnly,
            callback: Some(event::map_callback),
            userdata1: userdata(&result),
            userdata2: core::ptr::null_mut(),
        };
        let offset = usize::try_from(range.start).expect("mln_wgpu_webgpu_h: map offset exceeds usize");
        let size = usize::try_from(range.end - range.start).expect("mln_wgpu_webgpu_h: map size exceeds usize");
        let future = unsafe { wgpuBufferMapAsync(self.raw.raw(), mode, offset, size, info) };
        // wgpu runs map callbacks from Device::poll.
        self.device.defer(
            future,
            Box::new(move || {
                let (status, message) = take(&result, "mapAsync");
                if status != WGPUMapAsyncStatus_Success {
                    eprintln!("mln_wgpu_webgpu_h: mapAsync failed ({status}): {message}");
                }
                callback(if status == WGPUMapAsyncStatus_Success { Ok(()) } else { Err(wgpu::BufferAsyncError) });
            }),
        );
    }

    fn get_mapped_range(
        &self,
        sub_range: Range<wgpu::BufferAddress>,
    ) -> Result<DispatchBufferMappedRange, wgpu::MapRangeError> {
        let offset = usize::try_from(sub_range.start).expect("mln_wgpu_webgpu_h: mapped offset exceeds usize");
        let len =
            usize::try_from(sub_range.end - sub_range.start).expect("mln_wgpu_webgpu_h: mapped size exceeds usize");
        // emdawnwebgpu copies the mapping into the wasm heap; a write mapping
        // is copied back on unmap.
        let ptr = if self.map_mode.get() == WGPUMapMode_Read {
            unsafe { wgpuBufferGetConstMappedRange(self.raw.raw(), offset, len) }.cast_mut()
        } else {
            unsafe { wgpuBufferGetMappedRange(self.raw.raw(), offset, len) }
        };
        // wgpu::MapRangeError can't be built outside wgpu.
        let ptr = NonNull::new(ptr.cast::<u8>()).unwrap_or_else(|| {
            panic!("mln_wgpu_webgpu_h: getMappedRange({offset}, {len}) failed: the buffer is not mapped")
        });
        Ok(DispatchBufferMappedRange::custom(MappedRange { ptr, len }))
    }

    fn unmap(&self) {
        self.map_mode.set(WGPUMapMode_None);
        unsafe { wgpuBufferUnmap(self.raw.raw()) };
    }

    fn destroy(&self) {
        unsafe { wgpuBufferDestroy(self.raw.raw()) };
    }
}

/// A mapped range: memory emdawnwebgpu allocated in the wasm heap, valid
/// until the buffer is unmapped (wgpu drops every view before unmapping).
#[derive(Debug)]
pub struct MappedRange {
    ptr: NonNull<u8>,
    len: usize,
}

impl BufferMappedRangeInterface for MappedRange {
    fn len(&self) -> usize {
        self.len
    }

    unsafe fn read_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    unsafe fn write_slice(&mut self) -> wgpu::WriteOnly<'_, [u8]> {
        unsafe { wgpu::WriteOnly::new(NonNull::slice_from_raw_parts(self.ptr, self.len)) }
    }
}

/// `Queue::write_buffer_with`'s staging memory, written with
/// wgpuQueueWriteBuffer.
#[derive(Debug)]
pub struct StagingBuffer {
    pub(crate) data: Vec<u8>,
}

impl QueueWriteBufferInterface for StagingBuffer {
    fn len(&self) -> usize {
        self.data.len()
    }

    unsafe fn write_slice(&mut self) -> wgpu::WriteOnly<'_, [u8]> {
        wgpu::WriteOnly::from_mut(&mut self.data[..])
    }
}

/// A `wgpu::Texture` backend over a WGPUTexture.
#[derive(Debug)]
pub struct Texture {
    pub(crate) raw: TextureHandle,
}

impl TextureInterface for Texture {
    fn create_view(&self, desc: &wgpu::TextureViewDescriptor<'_>) -> DispatchTextureView {
        let raw_desc = WGPUTextureViewDescriptor {
            nextInChain: core::ptr::null_mut(),
            label: label(desc.label),
            format: crate::conv::optional_texture_format(desc.format),
            dimension: crate::conv::optional_texture_view_dimension(desc.dimension),
            baseMipLevel: desc.base_mip_level,
            mipLevelCount: desc.mip_level_count.unwrap_or(WGPU_MIP_LEVEL_COUNT_UNDEFINED),
            baseArrayLayer: desc.base_array_layer,
            arrayLayerCount: desc.array_layer_count.unwrap_or(WGPU_ARRAY_LAYER_COUNT_UNDEFINED),
            aspect: crate::conv::texture_aspect(desc.aspect),
            // None: the texture's usage.
            usage: desc.usage.map_or(WGPUTextureUsage_None, crate::conv::texture_usage),
        };
        let raw = unsafe { TextureViewHandle::from_raw(wgpuTextureCreateView(self.raw.raw(), &raw_desc)) };
        DispatchTextureView::custom(TextureView { raw: created(raw, "texture view") })
    }

    fn destroy(&self) {
        unsafe { wgpuTextureDestroy(self.raw.raw()) };
    }
}

/// A `wgpu::TextureView` backend over a WGPUTextureView.
#[derive(Debug)]
pub struct TextureView {
    pub(crate) raw: TextureViewHandle,
}
impl TextureViewInterface for TextureView {}

/// A `wgpu::Sampler` backend over a WGPUSampler.
#[derive(Debug)]
pub struct Sampler {
    pub(crate) raw: SamplerHandle,
}
impl SamplerInterface for Sampler {}

/// A `wgpu::ShaderModule` backend over a WGPUShaderModule.
#[derive(Debug)]
pub struct ShaderModule {
    pub(crate) raw: ShaderModuleHandle,
    pub(crate) device: Rc<DeviceState>,
}

impl ShaderModuleInterface for ShaderModule {
    fn get_compilation_info(&self) -> Pin<Box<dyn ShaderCompilationInfoFuture>> {
        let result = slot();
        let info = WGPUCompilationInfoCallbackInfo {
            nextInChain: core::ptr::null_mut(),
            mode: WGPUCallbackMode_WaitAnyOnly,
            callback: Some(event::compilation_info_callback),
            userdata1: userdata(&result),
            userdata2: core::ptr::null_mut(),
        };
        let future = unsafe { wgpuShaderModuleGetCompilationInfo(self.raw.raw(), info) };
        event::wait_forever(&self.device.instance.inner.raw, future);
        let messages = take(&result, "getCompilationInfo");
        Box::pin(core::future::ready(wgpu::CompilationInfo { messages }))
    }
}

/// A `wgpu::BindGroupLayout` backend over a WGPUBindGroupLayout.
#[derive(Debug)]
pub struct BindGroupLayout {
    pub(crate) raw: BindGroupLayoutHandle,
}
impl BindGroupLayoutInterface for BindGroupLayout {}

/// A `wgpu::BindGroup` backend over a WGPUBindGroup.
#[derive(Debug)]
pub struct BindGroup {
    pub(crate) raw: BindGroupHandle,
}
impl BindGroupInterface for BindGroup {}

/// A `wgpu::PipelineLayout` backend over a WGPUPipelineLayout.
#[derive(Debug)]
pub struct PipelineLayout {
    pub(crate) raw: PipelineLayoutHandle,
}
impl PipelineLayoutInterface for PipelineLayout {}

/// A `wgpu::RenderPipeline` backend over a WGPURenderPipeline.
#[derive(Debug)]
pub struct RenderPipeline {
    pub(crate) raw: RenderPipelineHandle,
}

impl RenderPipelineInterface for RenderPipeline {
    fn get_bind_group_layout(&self, index: u32) -> DispatchBindGroupLayout {
        let raw =
            unsafe { BindGroupLayoutHandle::from_raw(wgpuRenderPipelineGetBindGroupLayout(self.raw.raw(), index)) };
        DispatchBindGroupLayout::custom(BindGroupLayout { raw: created(raw, "bind group layout") })
    }
}

native_send_sync!(
    Buffer,
    MappedRange,
    StagingBuffer,
    Texture,
    TextureView,
    Sampler,
    ShaderModule,
    BindGroupLayout,
    BindGroup,
    PipelineLayout,
    RenderPipeline,
);
