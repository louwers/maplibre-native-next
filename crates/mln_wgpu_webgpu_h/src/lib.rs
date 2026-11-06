//! A wgpu backend over the WebGPU C API (`webgpu.h`) of Dawn's
//! emdawnwebgpu port, so wgpu renders with the browser's WebGPU inside an
//! Emscripten module (`--use-port=emdawnwebgpu`). wgpu 30 only has its
//! GLES backend on `wasm32-unknown-emscripten`; this implements the
//! `wgpu::custom` interfaces instead, for what maplibre-native-next uses:
//! adapters and devices, buffers (writes and mapped readback), textures,
//! views, samplers, WGSL shader modules, bind groups, render pipelines,
//! command encoders, render passes, copies, error scopes and uncaptured
//! errors, and surfaces on canvases ([`canvas_surface_target`]). Compute,
//! queries and render bundles are `unimplemented!`.
//!
//! # Threads
//!
//! - Use the backend from a pthread, never the browser main thread. wgpu's
//!   blocking points (adapter and device requests, `Device::poll(Wait)`,
//!   popping error scopes, compilation info) wait with
//!   `wgpuInstanceWaitAny`, which suspends the wasm stack (JSPI or
//!   Asyncify) until the browser resolves the promise. Link with `-sJSPI`
//!   (or `-sASYNCIFY`) and run the work under `-sPROXY_TO_PTHREAD` or in a
//!   `std::thread`.
//! - Every object belongs to the thread that created its instance: browser
//!   WebGPU objects live in that worker's JS realm. On wasm32 wgpu doesn't
//!   require `Send`/`Sync`, and with this backend compiled in its types are
//!   neither, so safe code can't move them; every use also asserts it is on
//!   the creating thread.
//! - Map and work-done callbacks run inside `Device::poll`, as on
//!   wgpu-core. The device-lost and uncaptured-error callbacks run when the
//!   thread yields to its event loop (inside a blocking wait).
//!
//! On other targets the crate compiles (webgpu.h functions are panicking
//! stubs) but must not be used.

// webgpu.h constants keep their C names, also in patterns.
#![allow(non_upper_case_globals)]
// The crate is FFI to webgpu.h.
#![allow(unsafe_code)]

mod command;
mod conv;
mod device;
mod event;
mod handle;
mod instance;
mod resource;
mod surface;
mod sys;

pub use command::{CommandBuffer, CommandEncoder, RenderPass};
pub use device::{Device, Queue};
pub use instance::{Adapter, Instance};
pub use resource::{
    BindGroup, BindGroupLayout, Buffer, MappedRange, PipelineLayout, RenderPipeline, Sampler, ShaderModule,
    StagingBuffer, Texture, TextureView,
};
pub use surface::{Surface, SurfaceOutputDetail, canvas_surface_target, host_surface_target};

/// wgpu objects over a `WGPUInstance` and `WGPUDevice` that the host created
/// through webgpu.h on this thread: the instance and the device with its
/// queue. Each wrapper takes its own reference, so the host keeps its own.
/// Make surfaces with [`host_surface_target`].
///
/// The instance must have the `TimedWaitAny` feature: the backend blocks in
/// `wgpuInstanceWaitAny`. The device's lost and uncaptured-error callbacks
/// stay the host's, so `wgpu::Device::on_uncaptured_error` handlers don't
/// run for it.
///
/// # Safety
///
/// `instance` and `device` are valid handles created on this thread, and
/// `device` comes from `instance`.
pub unsafe fn wrap_host_objects(
    instance: core::ptr::NonNull<core::ffi::c_void>,
    device: core::ptr::NonNull<core::ffi::c_void>,
) -> (wgpu::Instance, wgpu::Device, wgpu::Queue) {
    let raw_instance: sys::WGPUInstance = instance.as_ptr().cast();
    let raw_device: sys::WGPUDevice = device.as_ptr().cast();
    unsafe {
        sys::wgpuInstanceAddRef(raw_instance);
        sys::wgpuDeviceAddRef(raw_device);
    }
    let instance =
        Instance::from_host(unsafe { handle::InstanceHandle::from_raw(raw_instance) }.expect("a non-null instance"));
    let device = unsafe { handle::DeviceHandle::from_raw(raw_device) }.expect("a non-null device");
    let (device, queue) = device::DeviceState::create(device::next_device_id(), device, instance.clone());
    (wgpu::Instance::from_custom(instance), wgpu::Device::from_custom(device), wgpu::Queue::from_custom(queue))
}

/// The wgpu format of a `WGPUTextureFormat` that a canvas surface can have
/// (BGRA8Unorm, RGBA8Unorm, RGBA16Float), or `None`.
pub fn surface_texture_format(format: u32) -> Option<wgpu::TextureFormat> {
    surface::texture_format_from(format as sys::WGPUTextureFormat)
}

/// Creates a `wgpu::Instance` on the browser's WebGPU through webgpu.h.
///
/// Call it on the thread that will use the instance and everything created
/// from it (see the crate docs). Panics if webgpu.h can't create an
/// instance that can block (no JSPI or Asyncify).
pub fn create_instance(desc: wgpu::InstanceDescriptor) -> wgpu::Instance {
    use wgpu::custom::InstanceInterface;
    wgpu::Instance::from_custom(Instance::new(desc))
}
