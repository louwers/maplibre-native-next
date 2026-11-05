// Port-only: waiting for the browser in the browser build (wasm32-unknown-
// unknown, tools/web-test). The module runs on the page's thread, and the
// browser's asynchronous work (a GPU buffer mapping, an adapter or device
// request, a fetch) completes only when control returns to its event loop.
// Where the native code blocks (pollster's `block_on`, `Device::poll(Wait)`),
// the browser build calls `wait`, which returns to the event loop for one
// turn and resumes there through JSPI (WebAssembly JavaScript Promise
// Integration): the host provides the import `mln_browser.wait` as a
// `WebAssembly.Suspending` function and enters the module through
// `WebAssembly.promising`.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

#[link(wasm_import_module = "mln_browser")]
unsafe extern "C" {
    /// Suspends the module until the browser's event loop has run one task.
    #[link_name = "wait"]
    fn mln_browser_wait();
}

/// Lets the browser's event loop run one task, then continues.
pub fn wait() {
    // SAFETY: the import takes no arguments and touches no memory of the
    // module; the host suspends and resumes the calling stack.
    unsafe { mln_browser_wait() }
}

/// `pollster::block_on` for the browser: polls `future`, letting the event
/// loop run between polls, until it is ready.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        wait();
    }
}
