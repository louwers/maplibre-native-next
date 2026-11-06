// Ported gtests of test/text/*.test.cpp that need mln_renderer (GlyphManager,
// SymbolLayout) or sit above mln_text (Formatted is mln_expression's), one
// module per C++ file. The others are ported next to their code: bidi,
// tagged_string, glyph_pbf, language_tag and shaping in mln_text;
// quads, get_anchors and cross_tile_symbol_index in mln_renderer::text;
// local_glyph_rasterizer in mln_platform_default (it renders a map).

mod calculate_tile_distances;
mod formatted;
mod glyph_manager;
mod glyph_manager_bitmaps;

use std::path::PathBuf;

use mln_util::logging::{Event, EventSeverity, Log, Observer};

/// A path in the runfiles tree of the C++ repository (`maplibre+`).
pub fn cpp_path(relative: &str) -> PathBuf {
    let base = std::env::var("TEST_SRCDIR").or_else(|_| std::env::var("RUNFILES_DIR")).expect("TEST_SRCDIR");
    PathBuf::from(base).join("maplibre+").join(relative)
}

/// `util::read_file` on a C++ test fixture.
pub fn read_file(relative: &str) -> Vec<u8> {
    let path = cpp_path(relative);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

struct NullObserver;

impl Observer for NullObserver {
    fn on_record(&self, _: EventSeverity, _: Event, _: i64, _: &str) -> bool {
        true
    }
}

/// `Log::setObserver(std::make_unique<Log::NullObserver>())` for the
/// calling thread, undone on drop.
pub struct SquelchLog;

impl SquelchLog {
    pub fn new() -> Self {
        Log::set_thread_observer(Some(Box::new(NullObserver)));
        SquelchLog
    }
}

impl Drop for SquelchLog {
    fn drop(&mut self) {
        Log::set_thread_observer(None);
    }
}
