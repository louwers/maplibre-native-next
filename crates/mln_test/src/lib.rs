//! Ports the shared helpers of the C++ unit tests in `test/src/mln/test`
//! (namespace `mln::test`) that more than one crate's tests use.

pub mod http_server;

pub use http_server::{HttpServer, closed_port_url};

use std::path::PathBuf;

/// A path in the runfiles tree of the C++ repository (`maplibre+`), where
/// the C++ tests' relative paths ("test/fixtures/...") point.
pub fn cpp_path(relative: &str) -> PathBuf {
    let base = std::env::var("TEST_SRCDIR")
        .or_else(|_| std::env::var("RUNFILES_DIR"))
        .expect("TEST_SRCDIR");
    PathBuf::from(base).join("maplibre+").join(relative)
}
