// Ported gtests of test/storage/offline_download.test.cpp and their helpers.

mod offline_download;

use std::path::PathBuf;

/// A path of the C++ repository, in the copy of its files these tests read
/// (testdata/).
pub fn cpp_path(relative: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata").join(relative)
}
