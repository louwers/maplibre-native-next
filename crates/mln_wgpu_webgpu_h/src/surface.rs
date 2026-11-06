// Surfaces on canvases. emdawnwebgpu creates a WGPUSurface from a canvas
// selector (WGPUEmscriptenSurfaceSourceCanvasHTMLSelector) that Emscripten's
// findCanvasEventTarget resolves: on a pthread, a canvas transferred to it as
// an OffscreenCanvas (GL.offscreenCanvases) or one registered in
// specialHTMLTargets; there is no DOM there.
//
// wgpuSurfacePresent aborts on Emscripten: the browser shows the current
// texture when the thread's task ends. A PROXY_TO_PTHREAD thread never
// returns to its event loop, so Queue::present waits for the submitted work,
// which suspends the wasm stack (JSPI) and lets the browser present.

use core::ffi::CStr;
use core::ptr::NonNull;
use std::cell::RefCell;
use std::ffi::CString;
use std::rc::Rc;

use wgpu::custom::*;
use wgpu::rwh;

use crate::conv;
use crate::device::Device;
use crate::event::{self, MessageResult, slot, take, userdata};
use crate::handle::{SurfaceHandle, TextureHandle, native_send_sync};
use crate::instance::{Adapter, Instance};
use crate::resource::{Texture, created};
use crate::sys::*;

/// A surface target for the canvas `selector` (`#canvas`, or a name
/// registered in Emscripten's `specialHTMLTargets`), for
/// `wgpu::Instance::create_surface_unsafe` on an instance from
/// [`crate::create_instance`].
///
/// The selector travels as the pointer of a `WebCanvas` window handle,
/// which only this backend understands. `RawWindowHandle::Web(id)` (wgpu's
/// WebGPU convention) works too and selects `[data-raw-handle="<id>"]`,
/// which needs the DOM (the main thread).
pub fn canvas_surface_target(selector: &'static CStr) -> wgpu::SurfaceTargetUnsafe {
    let handle = rwh::WebCanvasWindowHandle::new(NonNull::from(selector).cast());
    wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(rwh::RawDisplayHandle::Web(rwh::WebDisplayHandle::new())),
        raw_window_handle: rwh::RawWindowHandle::WebCanvas(handle),
    }
}

/// A surface target for a `WGPUSurface` that the host created on this
/// thread, for `wgpu::Instance::create_surface_unsafe` on an instance from
/// [`crate::wrap_host_objects`]. The surface wrapper takes its own
/// reference.
///
/// The surface travels as the pointer of a `WebOffscreenCanvas` window
/// handle, which only this backend understands. Presenting to it doesn't
/// wait for the GPU: the host yields to the browser after each frame, which
/// shows the frame.
///
/// # Safety
///
/// `surface` is a valid `WGPUSurface` created on this thread.
pub unsafe fn host_surface_target(surface: NonNull<core::ffi::c_void>) -> wgpu::SurfaceTargetUnsafe {
    wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(rwh::RawDisplayHandle::Web(rwh::WebDisplayHandle::new())),
        raw_window_handle: rwh::RawWindowHandle::WebOffscreenCanvas(rwh::WebOffscreenCanvasWindowHandle::new(surface)),
    }
}

/// A `wgpu::Surface` backend over a WGPUSurface.
#[derive(Debug)]
pub struct Surface {
    pub(crate) raw: SurfaceHandle,
    instance: Instance,
    /// The configured device's queue state, for `present`.
    device: RefCell<Option<Rc<crate::device::DeviceState>>>,
    /// The host's surface (`host_surface_target`): the host yields after
    /// each frame, so `present` needn't.
    host: bool,
}

/// What `get_current_texture` hands wgpu next to the texture.
#[derive(Debug)]
pub struct SurfaceOutputDetail {
    surface: SurfaceHandle,
    device: Option<Rc<crate::device::DeviceState>>,
    host: bool,
}

native_send_sync!(Surface, SurfaceOutputDetail);

pub(crate) fn surface<'a>(surface: &'a wgpu::Surface<'_>) -> &'a Surface {
    surface.as_custom::<Surface>().expect("mln_wgpu_webgpu_h: wgpu::Surface from another wgpu backend")
}

pub(crate) fn create_surface(
    instance: &Instance,
    target: wgpu::SurfaceTargetUnsafe,
) -> Result<DispatchSurface, wgpu::CreateSurfaceError> {
    #[allow(irrefutable_let_patterns)]
    let wgpu::SurfaceTargetUnsafe::RawHandle { raw_window_handle, .. } = target else {
        unimplemented!("mln_wgpu_webgpu_h: surfaces need a canvas window handle");
    };
    // host_surface_target: the host's own WGPUSurface.
    if let rwh::RawWindowHandle::WebOffscreenCanvas(handle) = raw_window_handle {
        let surface: WGPUSurface = handle.obj.as_ptr().cast();
        unsafe { wgpuSurfaceAddRef(surface) };
        let raw = unsafe { SurfaceHandle::from_raw(surface) };
        let raw = created(raw, "surface (a null WGPUSurface)");
        return Ok(DispatchSurface::custom(Surface {
            raw,
            instance: instance.clone(),
            device: RefCell::new(None),
            host: true,
        }));
    }
    // emdawnwebgpu reads the selector as a C string, whatever its length.
    let selector = match raw_window_handle {
        // canvas_surface_target: a NUL-terminated selector.
        rwh::RawWindowHandle::WebCanvas(handle) => unsafe { CStr::from_ptr(handle.obj.as_ptr().cast()) }.to_owned(),
        rwh::RawWindowHandle::Web(handle) => {
            CString::new(format!("[data-raw-handle=\"{}\"]", handle.id)).expect("no NUL in a number")
        }
        other => unimplemented!("mln_wgpu_webgpu_h: surfaces on {other:?}"),
    };
    let source = WGPUEmscriptenSurfaceSourceCanvasHTMLSelector {
        chain: WGPUChainedStruct {
            next: core::ptr::null_mut(),
            sType: WGPUSType_EmscriptenSurfaceSourceCanvasHTMLSelector,
        },
        selector: WGPUStringView { data: selector.as_ptr(), length: WGPU_STRLEN },
    };
    let desc = WGPUSurfaceDescriptor { nextInChain: (&raw const source.chain).cast_mut(), label: NULL_STRING };
    let raw = unsafe { SurfaceHandle::from_raw(wgpuInstanceCreateSurface(instance.raw().raw(), &desc)) };
    let raw = created(raw, "surface (no canvas with a WebGPU context for the selector?)");
    Ok(DispatchSurface::custom(Surface { raw, instance: instance.clone(), device: RefCell::new(None), host: false }))
}

pub(crate) fn texture_format_from(format: WGPUTextureFormat) -> Option<wgpu::TextureFormat> {
    Some(match format {
        WGPUTextureFormat_BGRA8Unorm => wgpu::TextureFormat::Bgra8Unorm,
        WGPUTextureFormat_RGBA8Unorm => wgpu::TextureFormat::Rgba8Unorm,
        WGPUTextureFormat_RGBA16Float => wgpu::TextureFormat::Rgba16Float,
        _ => return None,
    })
}

fn slice<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if ptr.is_null() { &[] } else { unsafe { core::slice::from_raw_parts(ptr, len) } }
}

impl SurfaceInterface for Surface {
    fn get_capabilities(&self, adapter: &DispatchAdapter) -> wgpu::SurfaceCapabilities {
        let adapter = adapter.as_custom::<Adapter>().expect("mln_wgpu_webgpu_h: adapter from another wgpu backend");
        let mut raw: WGPUSurfaceCapabilities = unsafe { core::mem::zeroed() };
        let status = unsafe { wgpuSurfaceGetCapabilities(self.raw.raw(), adapter.raw(), &mut raw) };
        assert_eq!(status, WGPUStatus_Success, "mln_wgpu_webgpu_h: wgpuSurfaceGetCapabilities failed");
        let formats: Vec<_> =
            slice(raw.formats, raw.formatCount).iter().filter_map(|f| texture_format_from(*f)).collect();
        let present_modes = slice(raw.presentModes, raw.presentModeCount)
            .iter()
            .filter_map(|m| match *m {
                WGPUPresentMode_Fifo => Some(wgpu::PresentMode::Fifo),
                WGPUPresentMode_FifoRelaxed => Some(wgpu::PresentMode::FifoRelaxed),
                WGPUPresentMode_Immediate => Some(wgpu::PresentMode::Immediate),
                WGPUPresentMode_Mailbox => Some(wgpu::PresentMode::Mailbox),
                _ => None,
            })
            .collect();
        let alpha_modes = slice(raw.alphaModes, raw.alphaModeCount)
            .iter()
            .filter_map(|m| match *m {
                WGPUCompositeAlphaMode_Opaque => Some(wgpu::CompositeAlphaMode::Opaque),
                WGPUCompositeAlphaMode_Premultiplied => Some(wgpu::CompositeAlphaMode::PreMultiplied),
                WGPUCompositeAlphaMode_Unpremultiplied => Some(wgpu::CompositeAlphaMode::PostMultiplied),
                WGPUCompositeAlphaMode_Inherit => Some(wgpu::CompositeAlphaMode::Inherit),
                _ => None,
            })
            .collect();
        let usages = wgpu::TextureUsages::from_bits_truncate(raw.usages as u32);
        unsafe { wgpuSurfaceCapabilitiesFreeMembers(raw) };
        // Canvases take "srgb" (the default) for every format; the color
        // management chain struct isn't passed (configure ignores
        // config.color_space), so SRGB is all this reports.
        let format_capabilities = formats
            .iter()
            .map(|&format| wgpu::SurfaceFormatCapabilities { format, color_spaces: wgpu::SurfaceColorSpaces::SRGB })
            .collect();
        wgpu::SurfaceCapabilities { formats, format_capabilities, present_modes, alpha_modes, usages }
    }

    fn configure(&self, device: &DispatchDevice, config: &wgpu::SurfaceConfiguration) {
        let device = device.as_custom::<Device>().expect("mln_wgpu_webgpu_h: device from another wgpu backend");
        if !matches!(config.present_mode, wgpu::PresentMode::Fifo | wgpu::PresentMode::AutoVsync) {
            // Canvases only have Fifo; like wgpu's WebGPU backend, use it.
            eprintln!(
                "mln_wgpu_webgpu_h: present mode {:?} is not available on a canvas; using Fifo",
                config.present_mode
            );
        }
        let view_formats: Vec<WGPUTextureFormat> =
            config.view_formats.iter().copied().map(conv::texture_format).collect();
        let raw_config = WGPUSurfaceConfiguration {
            nextInChain: core::ptr::null_mut(),
            device: device.state.raw.raw(),
            format: conv::texture_format(config.format),
            usage: conv::texture_usage(config.usage),
            width: config.width,
            height: config.height,
            viewFormatCount: view_formats.len(),
            viewFormats: view_formats.as_ptr(),
            alphaMode: match config.alpha_mode {
                wgpu::CompositeAlphaMode::Auto => WGPUCompositeAlphaMode_Auto,
                wgpu::CompositeAlphaMode::Opaque => WGPUCompositeAlphaMode_Opaque,
                wgpu::CompositeAlphaMode::PreMultiplied => WGPUCompositeAlphaMode_Premultiplied,
                wgpu::CompositeAlphaMode::PostMultiplied => WGPUCompositeAlphaMode_Unpremultiplied,
                wgpu::CompositeAlphaMode::Inherit => WGPUCompositeAlphaMode_Inherit,
            },
            presentMode: WGPUPresentMode_Fifo,
        };
        unsafe { wgpuSurfaceConfigure(self.raw.raw(), &raw_config) };
        *self.device.borrow_mut() = Some(device.state.clone());
    }

    fn get_current_texture(&self) -> (Option<DispatchTexture>, wgpu::SurfaceStatus, DispatchSurfaceOutputDetail) {
        let mut raw: WGPUSurfaceTexture = unsafe { core::mem::zeroed() };
        unsafe { wgpuSurfaceGetCurrentTexture(self.raw.raw(), &mut raw) };
        let status = match raw.status {
            WGPUSurfaceGetCurrentTextureStatus_SuccessOptimal => wgpu::SurfaceStatus::Good,
            WGPUSurfaceGetCurrentTextureStatus_SuccessSuboptimal => wgpu::SurfaceStatus::Suboptimal,
            WGPUSurfaceGetCurrentTextureStatus_Timeout => wgpu::SurfaceStatus::Timeout,
            WGPUSurfaceGetCurrentTextureStatus_Outdated => wgpu::SurfaceStatus::Outdated,
            WGPUSurfaceGetCurrentTextureStatus_Lost => wgpu::SurfaceStatus::Lost,
            _ => wgpu::SurfaceStatus::Validation,
        };
        let texture =
            unsafe { TextureHandle::from_raw(raw.texture) }.map(|raw| DispatchTexture::custom(Texture { raw }));
        let detail =
            SurfaceOutputDetail { surface: self.raw.clone(), device: self.device.borrow().clone(), host: self.host };
        (texture, status, DispatchSurfaceOutputDetail::custom(detail))
    }
}

impl SurfaceOutputDetailInterface for SurfaceOutputDetail {
    fn texture_discard(&self) {}

    fn texture_release(&self) {}
}

/// `Queue::present`: waits for the submitted work, which yields to the
/// browser so it shows the canvas texture. A host surface returns at once:
/// its host yields after the frame, and the next frame's CPU work overlaps
/// this one's GPU work.
pub(crate) fn present(detail: &DispatchSurfaceOutputDetail) {
    let detail = detail.as_custom::<SurfaceOutputDetail>().expect("mln_wgpu_webgpu_h: foreign surface texture");
    let _ = &detail.surface;
    if detail.host {
        return;
    }
    let Some(device) = &detail.device else { return };
    let result = slot::<MessageResult<WGPUQueueWorkDoneStatus>>();
    let info = WGPUQueueWorkDoneCallbackInfo {
        nextInChain: core::ptr::null_mut(),
        mode: WGPUCallbackMode_WaitAnyOnly,
        callback: Some(event::work_done_callback),
        userdata1: userdata(&result),
        userdata2: core::ptr::null_mut(),
    };
    let future = unsafe { wgpuQueueOnSubmittedWorkDone(device.queue.raw(), info) };
    event::wait_forever(&device.instance.inner.raw, future);
    take(&result, "onSubmittedWorkDone");
}

impl Surface {
    #[allow(dead_code)]
    fn instance(&self) -> &Instance {
        &self.instance
    }
}
