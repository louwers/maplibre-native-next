// webgpu.h futures and callbacks.
//
// Every asynchronous webgpu.h call here uses WGPUCallbackMode_WaitAnyOnly:
// its callback runs inside wgpuInstanceWaitAny on the calling thread. The
// callbacks only store their arguments in a `Slot`; the code that waited
// turns them into wgpu values and calls wgpu's callbacks, outside any
// borrow. With a timeout, wgpuInstanceWaitAny suspends the wasm stack (JSPI
// or Asyncify) and lets the worker's event loop run until the browser
// resolves the promise, so blocking needs a pthread (never the browser main
// thread) and an instance created with WGPUInstanceFeatureName_TimedWaitAny.
//
// emdawnwebgpu tracks a future's promise until a timed wait returns it, so
// every future is eventually waited with a timeout (never only polled with
// timeout 0, which would leak the promise).

use core::ffi::c_void;
use std::cell::RefCell;
use std::rc::Rc;

use crate::handle::InstanceHandle;
use crate::sys::*;

pub(crate) type Slot<T> = Rc<RefCell<Option<T>>>;

pub(crate) fn slot<T>() -> Slot<T> {
    Rc::new(RefCell::new(None))
}

/// A `userdata1` for a callback that fills `slot`; the callback consumes it.
pub(crate) fn userdata<T>(slot: &Slot<T>) -> *mut c_void {
    Rc::into_raw(slot.clone()).cast_mut().cast()
}

/// # Safety
///
/// `userdata` comes from `userdata::<T>` and is used once.
unsafe fn fill<T>(userdata: *mut c_void, value: T) {
    let slot = unsafe { Rc::from_raw(userdata.cast_const().cast::<RefCell<Option<T>>>()) };
    *slot.borrow_mut() = Some(value);
}

pub(crate) fn take<T>(slot: &Slot<T>, what: &str) -> T {
    slot.borrow_mut().take().unwrap_or_else(|| panic!("mln_wgpu_webgpu_h: {what} callback did not run"))
}

/// Blocks until `future` completes or `timeout_ns` passes; true if it
/// completed (its callback has run).
pub(crate) fn wait(instance: &InstanceHandle, future: WGPUFuture, timeout_ns: u64) -> bool {
    assert_ne!(future.id, 0, "mln_wgpu_webgpu_h: webgpu.h returned a null future");
    let mut info = WGPUFutureWaitInfo { future, completed: WGPU_FALSE };
    match unsafe { wgpuInstanceWaitAny(instance.raw(), 1, &mut info, timeout_ns.max(1)) } {
        WGPUWaitStatus_Success => info.completed != WGPU_FALSE,
        WGPUWaitStatus_TimedOut => false,
        status => panic!(
            "mln_wgpu_webgpu_h: wgpuInstanceWaitAny failed ({status}); blocking needs JSPI or \
             Asyncify and an instance with TimedWaitAny"
        ),
    }
}

pub(crate) fn wait_forever(instance: &InstanceHandle, future: WGPUFuture) {
    while !wait(instance, future, u64::MAX) {}
}

pub(crate) type MessageResult<S> = (S, String);
pub(crate) type RequestAdapterResult = (WGPURequestAdapterStatus, WGPUAdapter, String);
pub(crate) type RequestDeviceResult = (WGPURequestDeviceStatus, WGPUDevice, String);
pub(crate) type PopErrorScopeResult = (WGPUPopErrorScopeStatus, WGPUErrorType, String);

pub(crate) unsafe extern "C" fn request_adapter_callback(
    status: WGPURequestAdapterStatus,
    adapter: WGPUAdapter,
    message: WGPUStringView,
    userdata1: *mut c_void,
    _: *mut c_void,
) {
    unsafe { fill::<RequestAdapterResult>(userdata1, (status, adapter, to_string(message))) };
}

pub(crate) unsafe extern "C" fn request_device_callback(
    status: WGPURequestDeviceStatus,
    device: WGPUDevice,
    message: WGPUStringView,
    userdata1: *mut c_void,
    _: *mut c_void,
) {
    unsafe { fill::<RequestDeviceResult>(userdata1, (status, device, to_string(message))) };
}

pub(crate) unsafe extern "C" fn pop_error_scope_callback(
    status: WGPUPopErrorScopeStatus,
    type_: WGPUErrorType,
    message: WGPUStringView,
    userdata1: *mut c_void,
    _: *mut c_void,
) {
    unsafe { fill::<PopErrorScopeResult>(userdata1, (status, type_, to_string(message))) };
}

pub(crate) unsafe extern "C" fn map_callback(
    status: WGPUMapAsyncStatus,
    message: WGPUStringView,
    userdata1: *mut c_void,
    _: *mut c_void,
) {
    unsafe { fill::<MessageResult<WGPUMapAsyncStatus>>(userdata1, (status, to_string(message))) };
}

pub(crate) unsafe extern "C" fn work_done_callback(
    status: WGPUQueueWorkDoneStatus,
    message: WGPUStringView,
    userdata1: *mut c_void,
    _: *mut c_void,
) {
    unsafe { fill::<MessageResult<WGPUQueueWorkDoneStatus>>(userdata1, (status, to_string(message))) };
}

pub(crate) unsafe extern "C" fn compilation_info_callback(
    status: WGPUCompilationInfoRequestStatus,
    info: *const WGPUCompilationInfo,
    userdata1: *mut c_void,
    _: *mut c_void,
) {
    let mut messages = Vec::new();
    if status == WGPUCompilationInfoRequestStatus_Success && !info.is_null() {
        let info = unsafe { &*info };
        let raw = if info.messages.is_null() {
            &[][..]
        } else {
            unsafe { core::slice::from_raw_parts(info.messages, info.messageCount) }
        };
        for m in raw {
            messages.push(wgpu::CompilationMessage {
                message: unsafe { to_string(m.message) },
                message_type: match m.type_ {
                    WGPUCompilationMessageType_Error => wgpu::CompilationMessageType::Error,
                    WGPUCompilationMessageType_Warning => wgpu::CompilationMessageType::Warning,
                    _ => wgpu::CompilationMessageType::Info,
                },
                location: (m.lineNum != 0).then_some(wgpu::SourceLocation {
                    line_number: m.lineNum as u32,
                    line_position: m.linePos as u32,
                    offset: m.offset as u32,
                    length: m.length as u32,
                }),
            });
        }
    }
    unsafe { fill::<Vec<wgpu::CompilationMessage>>(userdata1, messages) };
}

/// Error type and message of a WGPUErrorType, `None` for NoError.
pub(crate) fn error(type_: WGPUErrorType, message: String) -> Option<wgpu::Error> {
    let source = || -> wgpu::ErrorSource { Box::<dyn core::error::Error + Send + Sync>::from(message.clone()) };
    match type_ {
        WGPUErrorType_NoError => None,
        WGPUErrorType_Validation => Some(wgpu::Error::Validation { source: source(), description: message }),
        WGPUErrorType_OutOfMemory => Some(wgpu::Error::OutOfMemory { source: source() }),
        _ => Some(wgpu::Error::Internal { source: source(), description: message }),
    }
}
