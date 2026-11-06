// Ported gtests of test/storage/offline_download.test.cpp and their helpers.

mod offline_download;

use std::path::PathBuf;

/// A path in the runfiles tree of the C++ repository (`maplibre+`).
pub fn cpp_path(relative: &str) -> PathBuf {
    let base = std::env::var("TEST_SRCDIR").or_else(|_| std::env::var("RUNFILES_DIR")).expect("TEST_SRCDIR");
    PathBuf::from(base).join("maplibre+").join(relative)
}
