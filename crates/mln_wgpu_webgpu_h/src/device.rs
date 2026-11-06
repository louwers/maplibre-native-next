// wgpu::Device and wgpu::Queue over WGPUDevice and WGPUQueue: resource
// creation, error scopes, uncaptured errors and polling.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use wgpu::custom::*;

use crate::conv;
use crate::event::{self, MessageResult, PopErrorScopeResult, slot, take, userdata};
use crate::handle::*;
use crate::instance::Instance;
use crate::resource::*;
use crate::sys::*;

/// A callback to run once its future completed, inside `poll`.
struct Pending {
    future: WGPUFuture,
    complete: Box<dyn FnOnce()>,
}

/// What the device-lost and uncaptured-error callbacks reach.
#[derive(Default)]
struct DeviceCallbacks {
    uncaptured: RefCell<Option<Arc<dyn wgpu::UncapturedErrorHandler>>>,
    lost: RefCell<Option<BoxDeviceLostCallback>>,
}

thread_local! {
    /// Device id -> callbacks. webgpu.h may call the device-lost callback
    /// after the last WGPUDevice reference is gone, so the callbacks look
    /// the device up here instead of holding a pointer to it.
    static CALLBACKS: RefCell<HashMap<usize, Rc<DeviceCallbacks>>> = RefCell::default();
}

pub(crate) fn next_device_id() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn callbacks(id: usize) -> Option<Rc<DeviceCallbacks>> {
    CALLBACKS.with(|map| map.borrow().get(&id).cloned())
}

pub(crate) unsafe extern "C" fn device_lost_callback(
    _device: *const WGPUDevice,
    reason: WGPUDeviceLostReason,
    message: WGPUStringView,
    userdata1: *mut core::ffi::c_void,
    _: *mut core::ffi::c_void,
) {
    let reason = match reason {
        WGPUDeviceLostReason_Destroyed => wgpu::DeviceLostReason::Destroyed,
        WGPUDeviceLostReason_Unknown => wgpu::DeviceLostReason::Unknown,
        // CallbackCancelled (instance dropped) and FailedCreation: wgpu has
        // no device to report them for.
        _ => return,
    };
    let callback = callbacks(userdata1 as usize).and_then(|c| c.lost.borrow_mut().take());
    if let Some(callback) = callback {
        callback(reason, unsafe { to_string(message) });
    }
}

pub(crate) unsafe extern "C" fn uncaptured_error_callback(
    _device: *const WGPUDevice,
    type_: WGPUErrorType,
    message: WGPUStringView,
    userdata1: *mut core::ffi::c_void,
    _: *mut core::ffi::c_void,
) {
    let Some(error) = event::error(type_, unsafe { to_string(message) }) else { return };
    let handler = callbacks(userdata1 as usize).and_then(|c| c.uncaptured.borrow().clone());
    match handler {
        Some(handler) => handler(error),
        // Like wgpu's WebGPU backend (the browser logs it); wgpu-core panics.
        None => eprintln!("mln_wgpu_webgpu_h: uncaptured WebGPU error: {error}"),
    }
}

/// State shared by a device, its queue and the objects that need to reach
/// them (buffers register map callbacks here).
pub(crate) struct DeviceState {
    id: usize,
    pub(crate) raw: DeviceHandle,
    pub(crate) queue: QueueHandle,
    pub(crate) instance: Instance,
    features: wgpu::Features,
    limits: wgpu::Limits,
    info: wgpu::AdapterInfo,
    callbacks: Rc<DeviceCallbacks>,
    pending: RefCell<Vec<Pending>>,
    error_scopes: Cell<u32>,
    submissions: Cell<u64>,
    /// The last submission `poll(Wait)` saw complete.
    completed_submission: Rc<Cell<u64>>,
    /// Buffers wgpu dropped since the last submit, destroyed after the next
    /// one (see `Buffer`'s `Drop`).
    pub(crate) dropped_buffers: RefCell<Vec<BufferHandle>>,
}

impl core::fmt::Debug for DeviceState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DeviceState").field("raw", &self.raw).finish_non_exhaustive()
    }
}

impl Drop for DeviceState {
    fn drop(&mut self) {
        // Before `raw` is released: releasing the last reference destroys the
        // device and runs the device-lost callback, which wgpu doesn't expect.
        CALLBACKS.with(|map| map.borrow_mut().remove(&self.id));
    }
}

impl DeviceState {
    pub(crate) fn create(id: usize, raw: DeviceHandle, instance: Instance) -> (Device, Queue) {
        let queue = unsafe { QueueHandle::from_raw(wgpuDeviceGetQueue(raw.raw())) }
            .expect("mln_wgpu_webgpu_h: wgpuDeviceGetQueue returned null");

        let mut supported = WGPUSupportedFeatures { featureCount: 0, features: core::ptr::null() };
        unsafe { wgpuDeviceGetFeatures(raw.raw(), &mut supported) };
        let features = unsafe { conv::features(&supported) };
        unsafe { wgpuSupportedFeaturesFreeMembers(supported) };

        let mut raw_limits = conv::undefined_limits();
        unsafe { wgpuDeviceGetLimits(raw.raw(), &mut raw_limits) };
        let limits = conv::limits_from(&raw_limits);

        let mut raw_info: WGPUAdapterInfo = unsafe { core::mem::zeroed() };
        unsafe { wgpuDeviceGetAdapterInfo(raw.raw(), &mut raw_info) };
        let info = unsafe { conv::adapter_info(&raw_info) };
        unsafe { wgpuAdapterInfoFreeMembers(raw_info) };

        let callbacks = Rc::new(DeviceCallbacks::default());
        CALLBACKS.with(|map| map.borrow_mut().insert(id, callbacks.clone()));
        let state = Rc::new(DeviceState {
            id,
            raw,
            queue,
            instance: instance.clone(),
            features,
            limits,
            info,
            callbacks,
            pending: RefCell::default(),
            error_scopes: Cell::new(0),
            submissions: Cell::new(0),
            completed_submission: Rc::new(Cell::new(0)),
            dropped_buffers: RefCell::default(),
        });
        let mut devices = instance.inner.devices.borrow_mut();
        devices.retain(|d| d.strong_count() > 0);
        devices.push(Rc::downgrade(&state));
        drop(devices);
        (Device { state: state.clone() }, Queue { state })
    }

    /// Runs `complete` from `poll` once `future` completed.
    pub(crate) fn defer(&self, future: WGPUFuture, complete: Box<dyn FnOnce()>) {
        self.pending.borrow_mut().push(Pending { future, complete });
    }

    fn on_submitted_work_done(&self, complete: Box<dyn FnOnce(MessageResult<WGPUQueueWorkDoneStatus>)>) {
        let result = slot();
        let info = WGPUQueueWorkDoneCallbackInfo {
            nextInChain: core::ptr::null_mut(),
            mode: WGPUCallbackMode_WaitAnyOnly,
            callback: Some(event::work_done_callback),
            userdata1: userdata(&result),
            userdata2: core::ptr::null_mut(),
        };
        let future = unsafe { wgpuQueueOnSubmittedWorkDone(self.queue.raw(), info) };
        self.defer(future, Box::new(move || complete(take(&result, "onSubmittedWorkDone"))));
    }

    /// wgpu-core semantics on webgpu.h: `Wait` blocks until the submission
    /// is done (an onSubmittedWorkDone future) and runs the callbacks of
    /// every future registered before the call; `Poll` lets the worker's
    /// event loop run once per pending future and runs the callbacks of
    /// those that completed.
    pub(crate) fn poll(&self, poll_type: wgpu::wgt::PollType<u64>) -> Result<wgpu::PollStatus, wgpu::PollError> {
        let (timeout_ns, blocking) = match poll_type {
            wgpu::wgt::PollType::Wait { submission_index, timeout } => {
                let last = self.submissions.get();
                let target = submission_index.unwrap_or(last);
                if target > last {
                    return Err(wgpu::PollError::WrongSubmissionIndex(target, last));
                }
                if self.completed_submission.get() < target {
                    let completed = self.completed_submission.clone();
                    self.on_submitted_work_done(Box::new(move |(status, _)| {
                        if status == WGPUQueueWorkDoneStatus_Success && completed.get() < target {
                            completed.set(target);
                        }
                    }));
                }
                let timeout_ns = timeout.map_or(u64::MAX, |t| u64::try_from(t.as_nanos()).unwrap_or(u64::MAX));
                (timeout_ns, true)
            }
            wgpu::wgt::PollType::Poll => (1, false),
        };

        let pending = core::mem::take(&mut *self.pending.borrow_mut());
        if pending.is_empty() {
            return Ok(if blocking { wgpu::PollStatus::WaitSucceeded } else { wgpu::PollStatus::QueueEmpty });
        }
        let mut done = Vec::new();
        let mut not_done = Vec::new();
        for p in pending {
            // A blocking poll stops at the first timeout, so callbacks still
            // run in order.
            if (blocking && !not_done.is_empty()) || !event::wait(&self.instance.inner.raw, p.future, timeout_ns) {
                not_done.push(p);
            } else {
                done.push(p.complete);
            }
        }
        let all_done = not_done.is_empty();
        if !all_done {
            let mut pending = self.pending.borrow_mut();
            not_done.append(&mut pending);
            *pending = not_done;
        }
        for complete in done {
            complete();
        }
        match (blocking, all_done) {
            (true, true) => Ok(wgpu::PollStatus::WaitSucceeded),
            (true, false) => Err(wgpu::PollError::Timeout),
            (false, true) => Ok(wgpu::PollStatus::QueueEmpty),
            (false, false) => Ok(wgpu::PollStatus::Poll),
        }
    }
}

/// A `wgpu::Device` backend over a WGPUDevice.
#[derive(Debug)]
pub struct Device {
    pub(crate) state: Rc<DeviceState>,
}

/// A `wgpu::Queue` backend over a WGPUQueue.
#[derive(Debug)]
pub struct Queue {
    state: Rc<DeviceState>,
}

native_send_sync!(Device, Queue);

impl Device {
    fn raw(&self) -> WGPUDevice {
        self.state.raw.raw()
    }
}

fn constants(constants: &[(&str, f64)]) -> Vec<WGPUConstantEntry> {
    constants
        .iter()
        .map(|(key, value)| WGPUConstantEntry {
            nextInChain: core::ptr::null_mut(),
            key: string_view(key),
            value: *value,
        })
        .collect()
}

fn stencil_face(face: &wgpu::StencilFaceState) -> WGPUStencilFaceState {
    WGPUStencilFaceState {
        compare: conv::compare_function(face.compare),
        failOp: conv::stencil_operation(face.fail_op),
        depthFailOp: conv::stencil_operation(face.depth_fail_op),
        passOp: conv::stencil_operation(face.pass_op),
    }
}

impl DeviceInterface for Device {
    fn features(&self) -> wgpu::Features {
        self.state.features
    }

    fn limits(&self) -> wgpu::Limits {
        self.state.limits.clone()
    }

    fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.state.info.clone()
    }

    fn create_shader_module(
        &self,
        desc: wgpu::ShaderModuleDescriptor<'_>,
        _shader_bound_checks: wgpu::ShaderRuntimeChecks,
    ) -> DispatchShaderModule {
        // WGSL goes to the browser unchanged (the port's WGSL is the C++
        // core's, which runs on Dawn too).
        let code = match &desc.source {
            wgpu::ShaderSource::Wgsl(code) => code,
            #[allow(unreachable_patterns)]
            _ => unimplemented!("mln_wgpu_webgpu_h: only WGSL shader modules are supported"),
        };
        let wgsl = WGPUShaderSourceWGSL {
            chain: WGPUChainedStruct { next: core::ptr::null_mut(), sType: WGPUSType_ShaderSourceWGSL },
            code: string_view(code),
        };
        let raw_desc =
            WGPUShaderModuleDescriptor { nextInChain: (&raw const wgsl.chain).cast_mut(), label: label(desc.label) };
        let raw = unsafe { ShaderModuleHandle::from_raw(wgpuDeviceCreateShaderModule(self.raw(), &raw_desc)) };
        DispatchShaderModule::custom(ShaderModule { raw: created(raw, "shader module"), device: self.state.clone() })
    }

    unsafe fn create_shader_module_passthrough(
        &self,
        _desc: &wgpu::ShaderModuleDescriptorPassthrough<'_>,
    ) -> DispatchShaderModule {
        unimplemented!("mln_wgpu_webgpu_h: create_shader_module_passthrough")
    }

    fn create_bind_group_layout(&self, desc: &wgpu::BindGroupLayoutDescriptor<'_>) -> DispatchBindGroupLayout {
        let entries: Vec<WGPUBindGroupLayoutEntry> = desc
            .entries
            .iter()
            .map(|entry| {
                let mut raw: WGPUBindGroupLayoutEntry = unsafe { core::mem::zeroed() };
                raw.binding = entry.binding;
                raw.visibility = conv::shader_stage(entry.visibility);
                raw.bindingArraySize = entry.count.map_or(0, |c| c.get());
                match entry.ty {
                    wgpu::BindingType::Buffer { ty, has_dynamic_offset, min_binding_size } => {
                        raw.buffer.type_ = match ty {
                            wgpu::BufferBindingType::Uniform => WGPUBufferBindingType_Uniform,
                            wgpu::BufferBindingType::Storage { read_only: false } => WGPUBufferBindingType_Storage,
                            wgpu::BufferBindingType::Storage { read_only: true } => {
                                WGPUBufferBindingType_ReadOnlyStorage
                            }
                        };
                        raw.buffer.hasDynamicOffset = to_bool(has_dynamic_offset);
                        raw.buffer.minBindingSize = min_binding_size.map_or(0, |s| s.get());
                    }
                    wgpu::BindingType::Sampler(ty) => {
                        raw.sampler.type_ = match ty {
                            wgpu::SamplerBindingType::Filtering => WGPUSamplerBindingType_Filtering,
                            wgpu::SamplerBindingType::NonFiltering => WGPUSamplerBindingType_NonFiltering,
                            wgpu::SamplerBindingType::Comparison => WGPUSamplerBindingType_Comparison,
                        };
                    }
                    wgpu::BindingType::Texture { sample_type, view_dimension, multisampled } => {
                        raw.texture.sampleType = conv::texture_sample_type(sample_type);
                        raw.texture.viewDimension = conv::texture_view_dimension(view_dimension);
                        raw.texture.multisampled = to_bool(multisampled);
                    }
                    wgpu::BindingType::StorageTexture { access, format, view_dimension } => {
                        raw.storageTexture.access = conv::storage_texture_access(access);
                        raw.storageTexture.format = conv::texture_format(format);
                        raw.storageTexture.viewDimension = conv::texture_view_dimension(view_dimension);
                    }
                    ty => unimplemented!("mln_wgpu_webgpu_h: binding type {ty:?}"),
                }
                raw
            })
            .collect();
        let raw_desc = WGPUBindGroupLayoutDescriptor {
            nextInChain: core::ptr::null_mut(),
            label: label(desc.label),
            entryCount: entries.len(),
            entries: entries.as_ptr(),
        };
        let raw = unsafe { BindGroupLayoutHandle::from_raw(wgpuDeviceCreateBindGroupLayout(self.raw(), &raw_desc)) };
        DispatchBindGroupLayout::custom(BindGroupLayout { raw: created(raw, "bind group layout") })
    }

    fn create_bind_group(&self, desc: &wgpu::BindGroupDescriptor<'_>) -> DispatchBindGroup {
        let entries: Vec<WGPUBindGroupEntry> = desc
            .entries
            .iter()
            .map(|entry| {
                let mut raw: WGPUBindGroupEntry = unsafe { core::mem::zeroed() };
                raw.binding = entry.binding;
                match &entry.resource {
                    wgpu::BindingResource::Buffer(binding) => {
                        raw.buffer = buffer(binding.buffer).raw.raw();
                        raw.offset = binding.offset;
                        raw.size = binding.size.map_or(WGPU_WHOLE_SIZE, |s| s.get());
                    }
                    wgpu::BindingResource::Sampler(s) => raw.sampler = sampler(s).raw.raw(),
                    wgpu::BindingResource::TextureView(v) => raw.textureView = texture_view(v).raw.raw(),
                    _ => unimplemented!("mln_wgpu_webgpu_h: binding resource arrays and external textures"),
                }
                raw
            })
            .collect();
        let raw_desc = WGPUBindGroupDescriptor {
            nextInChain: core::ptr::null_mut(),
            label: label(desc.label),
            layout: bind_group_layout(desc.layout).raw.raw(),
            entryCount: entries.len(),
            entries: entries.as_ptr(),
        };
        let raw = unsafe { BindGroupHandle::from_raw(wgpuDeviceCreateBindGroup(self.raw(), &raw_desc)) };
        DispatchBindGroup::custom(BindGroup { raw: created(raw, "bind group") })
    }

    fn create_pipeline_layout(&self, desc: &wgpu::PipelineLayoutDescriptor<'_>) -> DispatchPipelineLayout {
        let layouts: Vec<WGPUBindGroupLayout> = desc
            .bind_group_layouts
            .iter()
            .map(|l| l.map_or(core::ptr::null_mut(), |l| bind_group_layout(l).raw.raw()))
            .collect();
        let raw_desc = WGPUPipelineLayoutDescriptor {
            nextInChain: core::ptr::null_mut(),
            label: label(desc.label),
            bindGroupLayoutCount: layouts.len(),
            bindGroupLayouts: layouts.as_ptr(),
            immediateSize: desc.immediate_size,
        };
        let raw = unsafe { PipelineLayoutHandle::from_raw(wgpuDeviceCreatePipelineLayout(self.raw(), &raw_desc)) };
        DispatchPipelineLayout::custom(PipelineLayout { raw: created(raw, "pipeline layout") })
    }

    fn create_render_pipeline(&self, desc: &wgpu::RenderPipelineDescriptor<'_>) -> DispatchRenderPipeline {
        if desc.multiview_mask.is_some() {
            unimplemented!("mln_wgpu_webgpu_h: multiview render pipelines");
        }
        let primitive = &desc.primitive;
        if primitive.polygon_mode != wgpu::PolygonMode::Fill || primitive.conservative {
            unimplemented!("mln_wgpu_webgpu_h: polygon modes and conservative rasterization");
        }

        // Everything the descriptor points at lives in these locals until
        // wgpuDeviceCreateRenderPipeline returns.
        let attributes: Vec<Vec<WGPUVertexAttribute>> = desc
            .vertex
            .buffers
            .iter()
            .map(|layout| {
                layout.as_ref().map_or_else(Vec::new, |layout| {
                    layout
                        .attributes
                        .iter()
                        .map(|a| WGPUVertexAttribute {
                            nextInChain: core::ptr::null_mut(),
                            format: conv::vertex_format(a.format),
                            offset: a.offset,
                            shaderLocation: a.shader_location,
                        })
                        .collect()
                })
            })
            .collect();
        let buffers: Vec<WGPUVertexBufferLayout> = desc
            .vertex
            .buffers
            .iter()
            .zip(&attributes)
            .map(|(layout, attributes)| WGPUVertexBufferLayout {
                nextInChain: core::ptr::null_mut(),
                // Undefined with no attributes is an unused slot.
                stepMode: layout.as_ref().map_or(WGPUVertexStepMode_Undefined, |l| conv::vertex_step_mode(l.step_mode)),
                arrayStride: layout.as_ref().map_or(0, |l| l.array_stride),
                attributeCount: attributes.len(),
                attributes: attributes.as_ptr(),
            })
            .collect();
        let vertex_constants = constants(desc.vertex.compilation_options.constants);
        let vertex = WGPUVertexState {
            nextInChain: core::ptr::null_mut(),
            module: shader_module(desc.vertex.module).raw.raw(),
            entryPoint: label(desc.vertex.entry_point),
            constantCount: vertex_constants.len(),
            constants: vertex_constants.as_ptr(),
            bufferCount: buffers.len(),
            buffers: buffers.as_ptr(),
        };

        let depth_stencil = desc.depth_stencil.as_ref().map(|ds| WGPUDepthStencilState {
            nextInChain: core::ptr::null_mut(),
            format: conv::texture_format(ds.format),
            depthWriteEnabled: match ds.depth_write_enabled {
                None => WGPUOptionalBool_Undefined,
                Some(true) => WGPUOptionalBool_True,
                Some(false) => WGPUOptionalBool_False,
            },
            depthCompare: conv::optional_compare_function(ds.depth_compare),
            stencilFront: stencil_face(&ds.stencil.front),
            stencilBack: stencil_face(&ds.stencil.back),
            stencilReadMask: ds.stencil.read_mask,
            stencilWriteMask: ds.stencil.write_mask,
            depthBias: ds.bias.constant,
            depthBiasSlopeScale: ds.bias.slope_scale,
            depthBiasClamp: ds.bias.clamp,
        });

        let fragment_targets = desc.fragment.as_ref().map_or(&[][..], |f| f.targets);
        let blends: Vec<Option<WGPUBlendState>> = fragment_targets
            .iter()
            .map(|t| {
                t.as_ref().and_then(|t| t.blend).map(|b| WGPUBlendState {
                    color: conv::blend_component(&b.color),
                    alpha: conv::blend_component(&b.alpha),
                })
            })
            .collect();
        let targets: Vec<WGPUColorTargetState> = fragment_targets
            .iter()
            .zip(&blends)
            .map(|(target, blend)| WGPUColorTargetState {
                nextInChain: core::ptr::null_mut(),
                // Undefined is an unused color attachment slot.
                format: conv::optional_texture_format(target.as_ref().map(|t| t.format)),
                blend: blend.as_ref().map_or(core::ptr::null(), core::ptr::from_ref),
                writeMask: target.as_ref().map_or(0, |t| conv::color_write_mask(t.write_mask)),
            })
            .collect();
        let fragment_constants =
            desc.fragment.as_ref().map(|f| constants(f.compilation_options.constants)).unwrap_or_default();
        let fragment = desc.fragment.as_ref().map(|f| WGPUFragmentState {
            nextInChain: core::ptr::null_mut(),
            module: shader_module(f.module).raw.raw(),
            entryPoint: label(f.entry_point),
            constantCount: fragment_constants.len(),
            constants: fragment_constants.as_ptr(),
            targetCount: targets.len(),
            targets: targets.as_ptr(),
        });

        let raw_desc = WGPURenderPipelineDescriptor {
            nextInChain: core::ptr::null_mut(),
            label: label(desc.label),
            layout: desc.layout.map_or(core::ptr::null_mut(), |l| pipeline_layout(l).raw.raw()),
            vertex,
            primitive: WGPUPrimitiveState {
                nextInChain: core::ptr::null_mut(),
                topology: conv::primitive_topology(primitive.topology),
                stripIndexFormat: primitive.strip_index_format.map_or(WGPUIndexFormat_Undefined, conv::index_format),
                frontFace: conv::front_face(primitive.front_face),
                cullMode: conv::cull_mode(primitive.cull_mode),
                unclippedDepth: to_bool(primitive.unclipped_depth),
            },
            depthStencil: depth_stencil.as_ref().map_or(core::ptr::null(), core::ptr::from_ref),
            multisample: WGPUMultisampleState {
                nextInChain: core::ptr::null_mut(),
                count: desc.multisample.count,
                mask: desc.multisample.mask as u32,
                alphaToCoverageEnabled: to_bool(desc.multisample.alpha_to_coverage_enabled),
            },
            fragment: fragment.as_ref().map_or(core::ptr::null(), core::ptr::from_ref),
        };
        let raw = unsafe { RenderPipelineHandle::from_raw(wgpuDeviceCreateRenderPipeline(self.raw(), &raw_desc)) };
        DispatchRenderPipeline::custom(RenderPipeline { raw: created(raw, "render pipeline") })
    }

    fn create_mesh_pipeline(&self, _desc: &wgpu::MeshPipelineDescriptor<'_>) -> DispatchRenderPipeline {
        unimplemented!("mln_wgpu_webgpu_h: create_mesh_pipeline")
    }

    fn create_compute_pipeline(&self, _desc: &wgpu::ComputePipelineDescriptor<'_>) -> DispatchComputePipeline {
        unimplemented!("mln_wgpu_webgpu_h: create_compute_pipeline")
    }

    unsafe fn create_pipeline_cache(&self, _desc: &wgpu::PipelineCacheDescriptor<'_>) -> DispatchPipelineCache {
        unimplemented!("mln_wgpu_webgpu_h: create_pipeline_cache")
    }

    fn create_buffer(&self, desc: &wgpu::BufferDescriptor<'_>) -> DispatchBuffer {
        let raw_desc = WGPUBufferDescriptor {
            nextInChain: core::ptr::null_mut(),
            label: label(desc.label),
            usage: conv::buffer_usage(desc.usage),
            size: desc.size,
            mappedAtCreation: to_bool(desc.mapped_at_creation),
        };
        let raw = unsafe { BufferHandle::from_raw(wgpuDeviceCreateBuffer(self.raw(), &raw_desc)) };
        DispatchBuffer::custom(Buffer::new(created(raw, "buffer"), desc, self.state.clone()))
    }

    fn create_texture(&self, desc: &wgpu::TextureDescriptor<'_>) -> DispatchTexture {
        let view_formats: Vec<WGPUTextureFormat> =
            desc.view_formats.iter().copied().map(conv::texture_format).collect();
        let raw_desc = WGPUTextureDescriptor {
            nextInChain: core::ptr::null_mut(),
            label: label(desc.label),
            usage: conv::texture_usage(desc.usage),
            dimension: conv::texture_dimension(desc.dimension),
            size: conv::extent(desc.size),
            format: conv::texture_format(desc.format),
            mipLevelCount: desc.mip_level_count,
            sampleCount: desc.sample_count,
            viewFormatCount: view_formats.len(),
            viewFormats: view_formats.as_ptr(),
        };
        let raw = unsafe { TextureHandle::from_raw(wgpuDeviceCreateTexture(self.raw(), &raw_desc)) };
        DispatchTexture::custom(Texture { raw: created(raw, "texture") })
    }

    fn create_external_texture(
        &self,
        _desc: &wgpu::ExternalTextureDescriptor<'_>,
        _planes: &[&wgpu::TextureView],
    ) -> DispatchExternalTexture {
        unimplemented!("mln_wgpu_webgpu_h: create_external_texture")
    }

    fn create_blas(
        &self,
        _desc: &wgpu::CreateBlasDescriptor<'_>,
        _sizes: wgpu::BlasGeometrySizeDescriptors,
    ) -> (Option<u64>, DispatchBlas) {
        unimplemented!("mln_wgpu_webgpu_h: create_blas")
    }

    fn create_tlas(&self, _desc: &wgpu::CreateTlasDescriptor<'_>) -> DispatchTlas {
        unimplemented!("mln_wgpu_webgpu_h: create_tlas")
    }

    fn create_sampler(&self, desc: &wgpu::SamplerDescriptor<'_>) -> DispatchSampler {
        if desc.border_color.is_some() {
            unimplemented!("mln_wgpu_webgpu_h: sampler border colors");
        }
        let raw_desc = WGPUSamplerDescriptor {
            nextInChain: core::ptr::null_mut(),
            label: label(desc.label),
            addressModeU: conv::address_mode(desc.address_mode_u),
            addressModeV: conv::address_mode(desc.address_mode_v),
            addressModeW: conv::address_mode(desc.address_mode_w),
            magFilter: conv::filter_mode(desc.mag_filter),
            minFilter: conv::filter_mode(desc.min_filter),
            mipmapFilter: conv::mipmap_filter_mode(desc.mipmap_filter),
            lodMinClamp: desc.lod_min_clamp,
            lodMaxClamp: desc.lod_max_clamp,
            compare: conv::optional_compare_function(desc.compare),
            maxAnisotropy: desc.anisotropy_clamp,
        };
        let raw = unsafe { SamplerHandle::from_raw(wgpuDeviceCreateSampler(self.raw(), &raw_desc)) };
        DispatchSampler::custom(Sampler { raw: created(raw, "sampler") })
    }

    fn create_query_set(&self, _desc: &wgpu::QuerySetDescriptor<'_>) -> DispatchQuerySet {
        unimplemented!("mln_wgpu_webgpu_h: create_query_set")
    }

    fn create_command_encoder(&self, desc: &wgpu::CommandEncoderDescriptor<'_>) -> DispatchCommandEncoder {
        let raw_desc = WGPUCommandEncoderDescriptor { nextInChain: core::ptr::null_mut(), label: label(desc.label) };
        let raw = unsafe { CommandEncoderHandle::from_raw(wgpuDeviceCreateCommandEncoder(self.raw(), &raw_desc)) };
        DispatchCommandEncoder::custom(crate::command::CommandEncoder { raw: created(raw, "command encoder") })
    }

    fn create_render_bundle_encoder(
        &self,
        _desc: &wgpu::RenderBundleEncoderDescriptor<'_>,
    ) -> DispatchRenderBundleEncoder {
        unimplemented!("mln_wgpu_webgpu_h: create_render_bundle_encoder")
    }

    fn set_device_lost_callback(&self, device_lost_callback: BoxDeviceLostCallback) {
        *self.state.callbacks.lost.borrow_mut() = Some(device_lost_callback);
    }

    fn on_uncaptured_error(&self, handler: Arc<dyn wgpu::UncapturedErrorHandler>) {
        *self.state.callbacks.uncaptured.borrow_mut() = Some(handler);
    }

    fn push_error_scope(&self, filter: wgpu::ErrorFilter) -> u32 {
        let index = self.state.error_scopes.get();
        self.state.error_scopes.set(index.checked_add(1).expect("mln_wgpu_webgpu_h: too many error scopes"));
        unsafe { wgpuDevicePushErrorScope(self.raw(), conv::error_filter(filter)) };
        index
    }

    fn pop_error_scope(&self, index: u32) -> Pin<Box<dyn PopErrorScopeFuture>> {
        let count = self.state.error_scopes.get();
        if !std::thread::panicking() {
            assert!(count > 0, "mln_wgpu_webgpu_h: pop_error_scope without an error scope");
            assert_eq!(index + 1, count, "mln_wgpu_webgpu_h: error scopes must be popped in reverse order");
        }
        self.state.error_scopes.set(count.saturating_sub(1));
        // Waits now rather than in the future: an unpolled future (wgpu pops
        // scopes on drop) would leave emdawnwebgpu's promise behind.
        let result = slot::<PopErrorScopeResult>();
        let info = WGPUPopErrorScopeCallbackInfo {
            nextInChain: core::ptr::null_mut(),
            mode: WGPUCallbackMode_WaitAnyOnly,
            callback: Some(event::pop_error_scope_callback),
            userdata1: userdata(&result),
            userdata2: core::ptr::null_mut(),
        };
        let future = unsafe { wgpuDevicePopErrorScope(self.raw(), info) };
        event::wait_forever(&self.state.instance.inner.raw, future);
        let (status, type_, message) = take(&result, "popErrorScope");
        let error = if status == WGPUPopErrorScopeStatus_Success {
            event::error(type_, message)
        } else {
            eprintln!("mln_wgpu_webgpu_h: popErrorScope failed ({status}): {message}");
            None
        };
        Box::pin(core::future::ready(error))
    }

    unsafe fn start_graphics_debugger_capture(&self) {}

    unsafe fn stop_graphics_debugger_capture(&self) {}

    fn poll(&self, poll_type: wgpu::wgt::PollType<u64>) -> Result<wgpu::PollStatus, wgpu::PollError> {
        self.state.poll(poll_type)
    }

    fn get_internal_counters(&self) -> wgpu::InternalCounters {
        wgpu::InternalCounters::default()
    }

    fn generate_allocator_report(&self) -> Option<wgpu::AllocatorReport> {
        None
    }

    fn destroy(&self) {
        unsafe { wgpuDeviceDestroy(self.raw()) };
    }
}

impl QueueInterface for Queue {
    fn write_buffer(&self, buffer: &DispatchBuffer, offset: wgpu::BufferAddress, data: &[u8]) {
        let buffer = dispatch_buffer(buffer).raw.raw();
        unsafe { wgpuQueueWriteBuffer(self.state.queue.raw(), buffer, offset, data.as_ptr().cast(), data.len()) };
    }

    fn create_staging_buffer(&self, size: wgpu::BufferSize) -> Option<DispatchQueueWriteBuffer> {
        let size = usize::try_from(size.get()).ok()?;
        Some(DispatchQueueWriteBuffer::custom(StagingBuffer { data: vec![0; size] }))
    }

    fn validate_write_buffer(
        &self,
        _buffer: &DispatchBuffer,
        _offset: wgpu::BufferAddress,
        _size: wgpu::BufferSize,
    ) -> Option<()> {
        // The browser validates the write.
        Some(())
    }

    fn write_staging_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: wgpu::BufferAddress,
        staging_buffer: &DispatchQueueWriteBuffer,
    ) {
        let staging = staging_buffer.as_custom::<StagingBuffer>().expect("mln_wgpu_webgpu_h: foreign staging buffer");
        self.write_buffer(buffer, offset, &staging.data);
    }

    fn write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        let destination = conv::texel_copy_texture_info(&texture);
        let layout = conv::texel_copy_buffer_layout(data_layout);
        let size = conv::extent(size);
        unsafe {
            wgpuQueueWriteTexture(
                self.state.queue.raw(),
                &destination,
                data.as_ptr().cast(),
                data.len(),
                &layout,
                &size,
            );
        }
    }

    fn submit(&self, command_buffers: &mut dyn Iterator<Item = DispatchCommandBuffer>) -> u64 {
        let command_buffers: Vec<DispatchCommandBuffer> = command_buffers.collect();
        let raw: Vec<WGPUCommandBuffer> = command_buffers
            .iter()
            .map(|cb| {
                cb.as_custom::<crate::command::CommandBuffer>()
                    .expect("mln_wgpu_webgpu_h: foreign command buffer")
                    .raw
                    .raw()
            })
            .collect();
        unsafe { wgpuQueueSubmit(self.state.queue.raw(), raw.len(), raw.as_ptr()) };
        // Work submitted so far may still use them; the browser frees each
        // once that work completes.
        for buffer in self.state.dropped_buffers.take() {
            unsafe { wgpuBufferDestroy(buffer.raw()) };
        }
        let index = self.state.submissions.get() + 1;
        self.state.submissions.set(index);
        index
    }

    fn get_timestamp_period(&self) -> f32 {
        1.0
    }

    fn on_submitted_work_done(&self, callback: BoxSubmittedWorkDoneCallback) {
        self.state.on_submitted_work_done(Box::new(move |_| callback()));
    }

    fn compact_blas(&self, _blas: &DispatchBlas) -> (Option<u64>, DispatchBlas) {
        unimplemented!("mln_wgpu_webgpu_h: compact_blas")
    }

    fn present(&self, detail: &DispatchSurfaceOutputDetail) {
        crate::surface::present(detail);
    }
}
