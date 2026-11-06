// wgpu::Instance and wgpu::Adapter over WGPUInstance and WGPUAdapter.

use std::cell::RefCell;
use std::pin::Pin;
use std::rc::{Rc, Weak};

use wgpu::custom::{
    AdapterInterface, DispatchAdapter, DispatchDevice, DispatchQueue, DispatchSurface, EnumerateAdapterFuture,
    InstanceInterface, RequestAdapterFuture, RequestDeviceFuture,
};

use crate::conv;
use crate::device::{Device, DeviceState, Queue};
use crate::event::{self, RequestAdapterResult, RequestDeviceResult, slot, take, userdata};
use crate::handle::{AdapterHandle, DeviceHandle, InstanceHandle, native_send_sync};
use crate::sys::*;

/// A `wgpu::Instance` backend over webgpu.h. Create it with
/// [`crate::create_instance`].
#[derive(Debug, Clone)]
pub struct Instance {
    pub(crate) inner: Rc<InstanceInner>,
}

#[derive(Debug)]
pub(crate) struct InstanceInner {
    pub(crate) raw: InstanceHandle,
    backends: wgpu::Backends,
    /// For `poll_all_devices`.
    pub(crate) devices: RefCell<Vec<Weak<DeviceState>>>,
}

native_send_sync!(Instance, Adapter);

impl Instance {
    pub(crate) fn raw(&self) -> &InstanceHandle {
        &self.inner.raw
    }

    /// An instance over `raw`, which the host created (see
    /// [`crate::wrap_host_objects`]).
    pub(crate) fn from_host(raw: InstanceHandle) -> Self {
        Instance {
            inner: Rc::new(InstanceInner {
                raw,
                backends: wgpu::Backends::BROWSER_WEBGPU,
                devices: RefCell::default(),
            }),
        }
    }

    fn request_adapter_now(&self, options: &wgpu::RequestAdapterOptions<'_, '_>) -> Option<Adapter> {
        let raw_options = WGPURequestAdapterOptions {
            nextInChain: core::ptr::null_mut(),
            featureLevel: WGPUFeatureLevel_Core,
            powerPreference: conv::power_preference(options.power_preference),
            forceFallbackAdapter: to_bool(options.force_fallback_adapter),
            backendType: WGPUBackendType_Undefined,
            compatibleSurface: options
                .compatible_surface
                .map_or(core::ptr::null_mut(), |s| crate::surface::surface(s).raw.raw()),
        };
        let result = slot::<RequestAdapterResult>();
        let callback = WGPURequestAdapterCallbackInfo {
            nextInChain: core::ptr::null_mut(),
            mode: WGPUCallbackMode_WaitAnyOnly,
            callback: Some(event::request_adapter_callback),
            userdata1: userdata(&result),
            userdata2: core::ptr::null_mut(),
        };
        let future = unsafe { wgpuInstanceRequestAdapter(self.raw().raw(), &raw_options, callback) };
        event::wait_forever(self.raw(), future);
        let (status, adapter, message) = take(&result, "requestAdapter");
        let adapter = unsafe { AdapterHandle::from_raw(adapter) };
        if status != WGPURequestAdapterStatus_Success || adapter.is_none() {
            eprintln!("mln_wgpu_webgpu_h: requestAdapter failed ({status}): {message}");
            return None;
        }
        Some(Adapter { raw: adapter?, instance: self.clone() })
    }

    fn not_found(&self) -> wgpu::RequestAdapterError {
        wgpu::RequestAdapterError::NotFound {
            active_backends: wgpu::Backends::BROWSER_WEBGPU,
            requested_backends: self.inner.backends,
            supported_backends: wgpu::Backends::BROWSER_WEBGPU,
            no_fallback_backends: wgpu::Backends::empty(),
            no_adapter_backends: wgpu::Backends::BROWSER_WEBGPU,
            incompatible_surface_backends: wgpu::Backends::empty(),
        }
    }
}

impl InstanceInterface for Instance {
    fn new(desc: wgpu::InstanceDescriptor) -> Self {
        // TimedWaitAny lets wgpuInstanceWaitAny block (see event.rs).
        let features = [WGPUInstanceFeatureName_TimedWaitAny];
        let raw_desc = WGPUInstanceDescriptor {
            nextInChain: core::ptr::null_mut(),
            requiredFeatureCount: features.len(),
            requiredFeatures: features.as_ptr(),
            requiredLimits: core::ptr::null(),
        };
        let raw = unsafe { InstanceHandle::from_raw(wgpuCreateInstance(&raw_desc)) }
            .expect("mln_wgpu_webgpu_h: wgpuCreateInstance failed (TimedWaitAny needs JSPI or Asyncify)");
        Instance { inner: Rc::new(InstanceInner { raw, backends: desc.backends, devices: RefCell::default() }) }
    }

    unsafe fn create_surface(
        &self,
        target: wgpu::SurfaceTargetUnsafe,
    ) -> Result<DispatchSurface, wgpu::CreateSurfaceError> {
        crate::surface::create_surface(self, target)
    }

    fn request_adapter(&self, options: &wgpu::RequestAdapterOptions<'_, '_>) -> Pin<Box<dyn RequestAdapterFuture>> {
        let result = if self.inner.backends.contains(wgpu::Backends::BROWSER_WEBGPU) {
            self.request_adapter_now(options).map(DispatchAdapter::custom).ok_or_else(|| self.not_found())
        } else {
            Err(self.not_found())
        };
        Box::pin(core::future::ready(result))
    }

    fn poll_all_devices(&self, force_wait: bool) -> bool {
        let devices: Vec<_> = self.inner.devices.borrow().iter().filter_map(Weak::upgrade).collect();
        let poll_type =
            || if force_wait { wgpu::wgt::PollType::wait_indefinitely() } else { wgpu::wgt::PollType::Poll };
        devices.iter().all(|d| matches!(d.poll(poll_type()), Ok(status) if status.is_queue_empty()))
    }

    fn wgsl_language_features(&self) -> wgpu::WgslLanguageFeatures {
        let mut raw = WGPUSupportedWGSLLanguageFeatures { featureCount: 0, features: core::ptr::null() };
        unsafe { wgpuInstanceGetWGSLLanguageFeatures(self.raw().raw(), &mut raw) };
        let names = if raw.features.is_null() {
            &[][..]
        } else {
            unsafe { core::slice::from_raw_parts(raw.features, raw.featureCount) }
        };
        let mut features = wgpu::WgslLanguageFeatures::empty();
        for &name in names {
            features |= match name {
                WGPUWGSLLanguageFeatureName_Packed4x8IntegerDotProduct => {
                    wgpu::WgslLanguageFeatures::Packed4x8IntegerDotProduct
                }
                WGPUWGSLLanguageFeatureName_PointerCompositeAccess => {
                    wgpu::WgslLanguageFeatures::PointerCompositeAccess
                }
                _ => wgpu::WgslLanguageFeatures::empty(),
            };
        }
        unsafe { wgpuSupportedWGSLLanguageFeaturesFreeMembers(raw) };
        features
    }

    fn enumerate_adapters(&self, backends: wgpu::Backends) -> Pin<Box<dyn EnumerateAdapterFuture>> {
        let adapters = if backends.contains(wgpu::Backends::BROWSER_WEBGPU) {
            self.request_adapter_now(&wgpu::RequestAdapterOptions::default())
                .map(DispatchAdapter::custom)
                .into_iter()
                .collect()
        } else {
            Vec::new()
        };
        Box::pin(core::future::ready(adapters))
    }
}

/// A `wgpu::Adapter` backend over a WGPUAdapter.
#[derive(Debug)]
pub struct Adapter {
    raw: AdapterHandle,
    instance: Instance,
}

impl Adapter {
    pub(crate) fn raw(&self) -> WGPUAdapter {
        self.raw.raw()
    }

    fn request_device_now(
        &self,
        desc: &wgpu::DeviceDescriptor<'_>,
    ) -> Result<(Device, Queue), wgpu::RequestDeviceError> {
        let features = conv::feature_names(desc.required_features);
        let limits = conv::limits_to(&desc.required_limits);
        // Device-lost and uncaptured-error callbacks find the device through
        // a thread-local registry, so a late callback can't reach a freed
        // device (see device.rs).
        let id = crate::device::next_device_id();
        let raw_desc = WGPUDeviceDescriptor {
            nextInChain: core::ptr::null_mut(),
            label: label(desc.label),
            requiredFeatureCount: features.len(),
            requiredFeatures: features.as_ptr(),
            requiredLimits: &limits,
            defaultQueue: WGPUQueueDescriptor { nextInChain: core::ptr::null_mut(), label: NULL_STRING },
            deviceLostCallbackInfo: WGPUDeviceLostCallbackInfo {
                nextInChain: core::ptr::null_mut(),
                mode: WGPUCallbackMode_AllowSpontaneous,
                callback: Some(crate::device::device_lost_callback),
                userdata1: id as *mut core::ffi::c_void,
                userdata2: core::ptr::null_mut(),
            },
            uncapturedErrorCallbackInfo: WGPUUncapturedErrorCallbackInfo {
                nextInChain: core::ptr::null_mut(),
                callback: Some(crate::device::uncaptured_error_callback),
                userdata1: id as *mut core::ffi::c_void,
                userdata2: core::ptr::null_mut(),
            },
        };
        let result = slot::<RequestDeviceResult>();
        let callback = WGPURequestDeviceCallbackInfo {
            nextInChain: core::ptr::null_mut(),
            mode: WGPUCallbackMode_WaitAnyOnly,
            callback: Some(event::request_device_callback),
            userdata1: userdata(&result),
            userdata2: core::ptr::null_mut(),
        };
        let future = unsafe { wgpuAdapterRequestDevice(self.raw.raw(), &raw_desc, callback) };
        event::wait_forever(self.instance.raw(), future);
        let (status, device, message) = take(&result, "requestDevice");
        let device = unsafe { DeviceHandle::from_raw(device) };
        match device {
            Some(device) if status == WGPURequestDeviceStatus_Success => {
                Ok(DeviceState::create(id, device, self.instance.clone()))
            }
            _ => Err(wgpu::RequestDeviceError::from_message(format!(
                "mln_wgpu_webgpu_h: requestDevice failed ({status}): {message}"
            ))),
        }
    }
}

impl AdapterInterface for Adapter {
    fn request_device(&self, desc: &wgpu::DeviceDescriptor<'_>) -> Pin<Box<dyn RequestDeviceFuture>> {
        let result = self
            .request_device_now(desc)
            .map(|(device, queue)| (DispatchDevice::custom(device), DispatchQueue::custom(queue)));
        Box::pin(core::future::ready(result))
    }

    fn is_surface_supported(&self, _surface: &DispatchSurface) -> bool {
        true
    }

    fn features(&self) -> wgpu::Features {
        let mut raw = WGPUSupportedFeatures { featureCount: 0, features: core::ptr::null() };
        unsafe { wgpuAdapterGetFeatures(self.raw.raw(), &mut raw) };
        let features = unsafe { conv::features(&raw) };
        unsafe { wgpuSupportedFeaturesFreeMembers(raw) };
        features
    }

    fn limits(&self) -> wgpu::Limits {
        let mut raw = conv::undefined_limits();
        let status = unsafe { wgpuAdapterGetLimits(self.raw.raw(), &mut raw) };
        assert_eq!(status, WGPUStatus_Success, "mln_wgpu_webgpu_h: wgpuAdapterGetLimits failed");
        conv::limits_from(&raw)
    }

    fn downlevel_capabilities(&self) -> wgpu::DownlevelCapabilities {
        wgpu::DownlevelCapabilities::default()
    }

    fn get_info(&self) -> wgpu::AdapterInfo {
        let mut raw: WGPUAdapterInfo = unsafe { core::mem::zeroed() };
        let status = unsafe { wgpuAdapterGetInfo(self.raw.raw(), &mut raw) };
        assert_eq!(status, WGPUStatus_Success, "mln_wgpu_webgpu_h: wgpuAdapterGetInfo failed");
        let info = unsafe { conv::adapter_info(&raw) };
        unsafe { wgpuAdapterInfoFreeMembers(raw) };
        info
    }

    fn get_texture_format_features(&self, format: wgpu::TextureFormat) -> wgpu::TextureFormatFeatures {
        format.guaranteed_format_features(AdapterInterface::features(self))
    }

    fn get_presentation_timestamp(&self) -> wgpu::PresentationTimestamp {
        wgpu::PresentationTimestamp::INVALID_TIMESTAMP
    }

    fn cooperative_matrix_properties(&self) -> Vec<wgpu::wgt::CooperativeMatrixProperties> {
        Vec::new()
    }
}
