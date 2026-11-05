// Port-only: the clocks of std::time. On wasm32-unknown-unknown (the browser
// build, tools/web-test) std's `Instant::now()` and `SystemTime::now()` panic,
// as there is no OS clock; there they come from the web-time crate, which
// reads `performance.now()` and `Date.now()`. Everywhere else this is
// std::time itself.

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub use web_time::{Instant, SystemTime, UNIX_EPOCH};
