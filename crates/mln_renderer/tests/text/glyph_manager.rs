// Ports test/text/glyph_manager.test.cpp.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use mln_actor::RunLoop;
use mln_renderer::text::glyph_manager::{GlyphManager, GlyphManagerObserver, GlyphRequestor};
use mln_storage::response::{Error, Reason};
use mln_storage::stub_file_source::{ResponseType, StubFileSource};
use mln_storage::{FileSource, Kind, Resource, Response};
use mln_text::glyph::{
    GLYPH_RANGES_PER_FONT_STACK, GLYPHS_PER_GLYPH_RANGE, Glyph, GlyphDependencies, GlyphID, GlyphIDType, GlyphIDs,
    GlyphMap, GlyphMetrics, GlyphRange, HBShapeRequests, NON_IDEOGRAPH_GLYPH_RANGES_PER_FONT_STACK,
};
use mln_text::local_glyph_rasterizer::LocalGlyphRasterizer;
use mln_text::tagged_string::font_stack_hash;
use mln_util::exception::ExceptionPtr;
use mln_util::font_stack::FontStack;
use mln_util::i18n;
use mln_util::image::AlphaImage;
use mln_util::size::Size;

use super::glyph_manager_bitmaps::{SDF_BITMAP, STUB_BITMAP};
use super::{SquelchLog, read_file};

fn stub_glyph(glyph_id: GlyphID) -> Glyph {
    Glyph {
        id: glyph_id,
        metrics: GlyphMetrics { width: 24, height: 24, left: 0.0, top: -8.0, advance: 24, ..Default::default() },
        bitmap: AlphaImage::from_slice(Size::new(30, 30), &STUB_BITMAP).unwrap(),
    }
}

/// `StubLocalGlyphRasterizer`.
struct StubLocalGlyphRasterizer;

impl LocalGlyphRasterizer for StubLocalGlyphRasterizer {
    fn can_rasterize_glyph(&self, _font_stack: &FontStack, glyph_id: GlyphID) -> bool {
        i18n::allows_fixed_width_glyph_generation(glyph_id.code)
    }

    fn rasterize_glyph(&self, _font_stack: &FontStack, glyph_id: GlyphID) -> Glyph {
        stub_glyph(glyph_id)
    }
}

/// `KatakanaOnlyLocalGlyphRasterizer`.
struct KatakanaOnlyLocalGlyphRasterizer;

impl LocalGlyphRasterizer for KatakanaOnlyLocalGlyphRasterizer {
    fn can_rasterize_glyph(&self, _font_stack: &FontStack, glyph_id: GlyphID) -> bool {
        glyph_id.code == 'テ' as u16
    }

    fn rasterize_glyph(&self, _font_stack: &FontStack, glyph_id: GlyphID) -> Glyph {
        stub_glyph(glyph_id)
    }
}

type LoadedFn = Box<dyn FnMut(&FontStack, &GlyphRange)>;
type ErrorFn = Box<dyn FnMut(&FontStack, &GlyphRange, ExceptionPtr)>;

/// `StubGlyphManagerObserver`.
#[derive(Default)]
struct StubGlyphManagerObserver {
    glyphs_loaded: RefCell<Option<LoadedFn>>,
    glyphs_error: RefCell<Option<ErrorFn>>,
}

impl GlyphManagerObserver for StubGlyphManagerObserver {
    fn on_glyphs_loaded(&self, font_stack: &FontStack, range: &GlyphRange) {
        if let Some(f) = self.glyphs_loaded.borrow_mut().as_mut() {
            f(font_stack, range);
        }
    }

    fn on_glyphs_error(&self, font_stack: &FontStack, range: &GlyphRange, error: ExceptionPtr) {
        if let Some(f) = self.glyphs_error.borrow_mut().as_mut() {
            f(font_stack, range, error);
        }
    }
}

/// `StubGlyphRequestor`. The callback is an `Rc<dyn Fn>` because C++'s
/// LocalCJKGlyphPrefersLocalRasterizationOverRange calls `getGlyphs` from
/// inside it, which notifies the requestor again.
#[derive(Default)]
struct StubGlyphRequestor {
    glyphs_available: RefCell<Option<Rc<dyn Fn(GlyphMap)>>>,
}

impl GlyphRequestor for StubGlyphRequestor {
    fn on_glyphs_available(&self, glyphs: GlyphMap, _requests: HBShapeRequests) {
        let f = self.glyphs_available.borrow().clone();
        if let Some(f) = f {
            f(glyphs);
        }
    }
}

struct GlyphManagerTest {
    run_loop: RunLoop,
    file_source: Arc<StubFileSource>,
    observer: Rc<StubGlyphManagerObserver>,
    requestor: Rc<StubGlyphRequestor>,
    glyph_manager: Rc<GlyphManager>,
    _log: Option<SquelchLog>,
}

impl GlyphManagerTest {
    fn new() -> Self {
        Self::with(Box::new(StubLocalGlyphRasterizer), ResponseType::Asynchronous)
    }

    fn with(local_glyph_rasterizer: Box<dyn LocalGlyphRasterizer>, response_type: ResponseType) -> Self {
        let run_loop = RunLoop::new();
        GlyphManagerTest {
            run_loop,
            file_source: Arc::new(StubFileSource::new(response_type)),
            observer: Rc::new(StubGlyphManagerObserver::default()),
            requestor: Rc::new(StubGlyphRequestor::default()),
            glyph_manager: GlyphManager::new(Some(local_glyph_rasterizer)),
            _log: None,
        }
    }

    fn file_source(&self) -> Arc<dyn FileSource> {
        self.file_source.clone()
    }

    fn requestor(&self) -> Rc<dyn GlyphRequestor> {
        self.requestor.clone()
    }

    fn run(&mut self, url: &str, dependencies: GlyphDependencies) {
        // Squelch logging.
        self._log = Some(SquelchLog::new());

        self.glyph_manager.set_url(url);
        self.glyph_manager.set_observer(Some(self.observer.clone()));
        self.glyph_manager.get_glyphs(&self.requestor(), dependencies, Some(&self.file_source()));

        self.run_loop.run();
    }

    fn end(&self) {
        self.run_loop.stop();
    }
}

fn test_stack() -> FontStack {
    vec!["Test Stack".to_owned()]
}

/// `GlyphDependencies{.glyphs = {{{{"Test Stack"}}, {chars...}}}, .shapes = {}}`.
fn dependencies(chars: &[char]) -> GlyphDependencies {
    let ids: GlyphIDs = chars.iter().map(|c| GlyphID::new(*c as u32)).collect();
    GlyphDependencies { glyphs: BTreeMap::from([(test_stack(), ids)]), shapes: Default::default() }
}

fn gid(c: char) -> GlyphID {
    GlyphID::new(c as u32)
}

fn glyphs_response(path: &'static str) -> impl Fn(&Resource) -> Option<Response> {
    move |_resource: &Resource| {
        let mut response = Response::new();
        response.data = Some(Arc::new(read_file(path)));
        Some(response)
    }
}

// gtest: GlyphManager.LoadingSuccess
#[test]
fn loading_success() {
    let mut test = GlyphManagerTest::new();

    test.file_source.set_response(Kind::Glyphs, |resource: &Resource| {
        assert_eq!(Kind::Glyphs, resource.kind);
        let mut response = Response::new();
        response.data = Some(Arc::new(read_file("test/fixtures/resources/glyphs.pbf")));
        Some(response)
    });

    *test.observer.glyphs_error.borrow_mut() = Some(Box::new(|_, _, _| panic!("glyphsError")));

    *test.observer.glyphs_loaded.borrow_mut() = Some(Box::new(|font_stack, range| {
        assert_eq!(*font_stack, test_stack());
        assert_eq!(*range, GlyphRange::new(0, 255, GlyphIDType::FontPBF));
    }));

    let available = Rc::new(Cell::new(false));
    let (a, run_loop) = (available.clone(), test.run_loop.clone());
    *test.requestor.glyphs_available.borrow_mut() = Some(Rc::new(move |glyphs: GlyphMap| {
        let test_positions = &glyphs[&font_stack_hash(&test_stack())];

        assert_eq!(test_positions.len(), 3);
        assert!(test_positions.contains_key(&gid('a')));
        assert!(test_positions.contains_key(&gid('å')));
        assert!(test_positions.contains_key(&gid(' ')));
        assert!(test_positions[&gid(' ')].is_some());

        a.set(true);
        run_loop.stop();
    }));

    test.run("test/fixtures/resources/glyphs.pbf", dependencies(&['a', 'å', ' ']));
    assert!(available.get());
}

/// The observer's `glyphsError` for LoadingFail / LoadingCorrupted.
fn expect_error(test: &GlyphManagerTest, message: &'static str) -> Rc<Cell<bool>> {
    let called = Rc::new(Cell::new(false));
    let (c, run_loop) = (called.clone(), test.run_loop.clone());
    *test.observer.glyphs_error.borrow_mut() = Some(Box::new(move |font_stack, glyph_range, error| {
        assert_eq!(*font_stack, test_stack());
        assert_eq!(*glyph_range, GlyphRange::new(0, 255, GlyphIDType::FontPBF));

        assert_eq!(error.to_string(), message);

        c.set(true);
        run_loop.stop();
    }));

    *test.requestor.glyphs_available.borrow_mut() = Some(Rc::new(|_| panic!("glyphsAvailable")));
    called
}

// gtest: GlyphManager.LoadingFail
#[test]
fn loading_fail() {
    let mut test = GlyphManagerTest::new();

    test.file_source.set_response(Kind::Glyphs, |_: &Resource| {
        let mut response = Response::new();
        response.error = Some(Box::new(Error::new(Reason::Other, "Failed by the test case")));
        Some(response)
    });

    let called = expect_error(&test, "Failed by the test case");

    test.run("test/fixtures/resources/glyphs.pbf", dependencies(&['a', 'å']));
    assert!(called.get());
}

// gtest: GlyphManager.LoadingCorrupted
#[test]
fn loading_corrupted() {
    let mut test = GlyphManagerTest::new();

    test.file_source.set_response(Kind::Glyphs, |_: &Resource| {
        let mut response = Response::new();
        response.data = Some(Arc::new(b"CORRUPTED".to_vec()));
        Some(response)
    });

    let called = expect_error(&test, "unknown pbf field type exception");

    test.run("test/fixtures/resources/glyphs.pbf", dependencies(&['a', 'å']));
    assert!(called.get());
}

// gtest: GlyphManager.LoadingCancel
#[test]
fn loading_cancel() {
    let mut test = GlyphManagerTest::new();

    let run_loop = test.run_loop.clone();
    test.file_source.set_response(Kind::Glyphs, move |_: &Resource| {
        run_loop.stop();
        None
    });

    *test.observer.glyphs_loaded.borrow_mut() = Some(Box::new(|_, _| panic!("Should never be called")));

    test.run("test/fixtures/resources/glyphs.pbf", dependencies(&['a', 'å']));
}

// gtest: GlyphManager.LoadLocalCJKGlyph
#[test]
fn load_local_cjk_glyph() {
    let mut test = GlyphManagerTest::new();
    let glyph_responses = Rc::new(Cell::new(0));

    let g = glyph_responses.clone();
    test.file_source.set_response(Kind::Glyphs, move |_: &Resource| {
        g.set(g.get() + 1);
        None
    });

    let g = glyph_responses.clone();
    *test.observer.glyphs_loaded.borrow_mut() = Some(Box::new(move |_, _| g.set(g.get() + 1)));

    let available = Rc::new(Cell::new(false));
    let (a, g, run_loop) = (available.clone(), glyph_responses.clone(), test.run_loop.clone());
    *test.requestor.glyphs_available.borrow_mut() = Some(Rc::new(move |glyphs: GlyphMap| {
        assert_eq!(g.get(), 0); // Local generation should prevent requesting any glyphs

        let test_positions = &glyphs[&font_stack_hash(&test_stack())];

        assert_eq!(test_positions.len(), 1);
        assert!(test_positions.contains_key(&gid('中')));

        let glyph = test_positions[&gid('中')].clone().expect("glyph");
        assert_eq!(glyph.id.code, '中' as u16);
        assert_eq!(glyph.metrics.width, 24);
        assert_eq!(glyph.metrics.height, 24);
        assert_eq!(glyph.metrics.left, 0.0);
        assert_eq!(glyph.metrics.top, -8.0);
        assert_eq!(glyph.metrics.advance, 24);
        assert_eq!(glyph.bitmap.size, Size::new(30, 30));

        let pixel_count = (glyph.bitmap.size.width * glyph.bitmap.size.height) as usize;
        for i in 0..pixel_count {
            assert_eq!(glyph.bitmap.data[i], SDF_BITMAP[i], "pixel {i}");
        }

        a.set(true);
        run_loop.stop();
    }));

    test.run("test/fixtures/resources/glyphs.pbf", dependencies(&['中']));
    assert!(available.get());
}

// gtest: GlyphManager.LoadLocalCJKSymbols
#[test]
fn load_local_cjk_symbols() {
    let mut test = GlyphManagerTest::new();
    let glyph_responses = Rc::new(Cell::new(0));

    let g = glyph_responses.clone();
    test.file_source.set_response(Kind::Glyphs, move |_: &Resource| {
        g.set(g.get() + 1);
        None
    });

    let g = glyph_responses.clone();
    *test.observer.glyphs_loaded.borrow_mut() = Some(Box::new(move |_, _| g.set(g.get() + 1)));

    let available = Rc::new(Cell::new(false));
    let (a, g, run_loop) = (available.clone(), glyph_responses.clone(), test.run_loop.clone());
    *test.requestor.glyphs_available.borrow_mut() = Some(Rc::new(move |glyphs: GlyphMap| {
        assert_eq!(g.get(), 0); // Local generation should prevent requesting any glyphs.

        let test_positions = &glyphs[&font_stack_hash(&test_stack())];

        assert_eq!(test_positions.len(), 2);
        assert!(test_positions.contains_key(&gid('〒')));
        assert!(test_positions.contains_key(&gid('！')));

        a.set(true);
        run_loop.stop();
    }));

    test.run("test/fixtures/resources/glyphs.pbf", dependencies(&['〒', '！']));
    assert!(available.get());
}

// gtest: GlyphManager.LocalCJKGlyphPrefersLocalRasterizationOverRange
#[test]
fn local_cjk_glyph_prefers_local_rasterization_over_range() {
    // Verifies that locally-rasterized glyphs (like テ) are produced by the
    // local rasterizer instead of using a glyph from an already-loaded PBF
    // range.
    let mut test = GlyphManagerTest::with(Box::new(KatakanaOnlyLocalGlyphRasterizer), ResponseType::Asynchronous);
    let first_glyph_response = Rc::new(Cell::new(false));

    let f = first_glyph_response.clone();
    test.file_source.set_response(Kind::Glyphs, move |_: &Resource| {
        f.set(true);
        let mut response = Response::new();
        response.data = Some(Arc::new(read_file("test/fixtures/resources/glyphs-12244-12543.pbf")));
        Some(response)
    });

    let done = Rc::new(Cell::new(false));
    let (f, d, run_loop) = (first_glyph_response.clone(), done.clone(), test.run_loop.clone());
    let glyph_manager = Rc::downgrade(&test.glyph_manager);
    let requestor = Rc::downgrade(&test.requestor);
    let file_source = test.file_source();
    *test.requestor.glyphs_available.borrow_mut() = Some(Rc::new(move |glyphs: GlyphMap| {
        let test_positions = &glyphs[&font_stack_hash(&test_stack())];

        if f.get() {
            f.set(false);
            assert_eq!(test_positions.len(), 1);
            assert!(test_positions.contains_key(&gid('々')));

            // Katakana letter te, should be locally rasterized
            //  instead of using the glyph received from the range
            //  for the ideagraphic mark
            let requestor: Rc<dyn GlyphRequestor> = requestor.upgrade().unwrap();
            glyph_manager.upgrade().unwrap().get_glyphs(
                &requestor,
                dependencies(&['テ']), // 0x30c6
                Some(&file_source),
            );
        } else {
            assert_eq!(test_positions.len(), 1);
            assert!(test_positions.contains_key(&gid('テ')));

            let glyph = test_positions[&gid('テ')].clone().expect("glyph");
            assert_eq!(glyph.id.code, 'テ' as u16);
            assert_eq!(glyph.metrics.width, 24);
            assert_eq!(glyph.metrics.height, 24);
            assert_eq!(glyph.metrics.left, 0.0);
            assert_eq!(glyph.metrics.top, -8.0);
            assert_eq!(glyph.metrics.advance, 24);
            assert_eq!(glyph.bitmap.size, Size::new(30, 30));

            d.set(true);
            run_loop.stop();
        }
    }));

    test.run(
        "test/fixtures/resources/glyphs-12244-12543.pbf",
        dependencies(&['々']), // 0x3005
    );
    assert!(done.get());
}

// gtest: GlyphManager.LoadingInvalid
#[test]
fn loading_invalid() {
    let mut test = GlyphManagerTest::new();

    test.file_source.set_response(Kind::Glyphs, |resource: &Resource| {
        assert_eq!(Kind::Glyphs, resource.kind);
        let mut response = Response::new();
        response.data = Some(Arc::new(read_file("test/fixtures/resources/fake_glyphs-0-255.pbf")));
        Some(response)
    });

    *test.observer.glyphs_error.borrow_mut() = Some(Box::new(|_, _, _| panic!("glyphsError")));

    *test.observer.glyphs_loaded.borrow_mut() = Some(Box::new(|font_stack, range| {
        assert_eq!(*font_stack, test_stack());
        assert_eq!(*range, GlyphRange::new(0, 255, GlyphIDType::FontPBF));
    }));

    let available = Rc::new(Cell::new(false));
    let (a, run_loop) = (available.clone(), test.run_loop.clone());
    *test.requestor.glyphs_available.borrow_mut() = Some(Rc::new(move |glyphs: GlyphMap| {
        let test_positions = &glyphs[&font_stack_hash(&test_stack())];

        assert_eq!(test_positions.len(), 2);
        assert!(test_positions[&gid('A')].is_none());
        assert!(test_positions[&gid('E')].is_some());

        a.set(true);
        run_loop.stop();
    }));

    test.run("test/fixtures/resources/glyphs.pbf", dependencies(&['A', 'E']));
    assert!(available.get());
}

// gtest: GlyphManager.ImmediateFileSource
#[test]
fn immediate_file_source() {
    // `GlyphManagerTestSynchronous`: the default GlyphManager (no local
    // rasterizer) and a synchronous StubFileSource.
    let mut test = GlyphManagerTest::with(
        Box::new(mln_text::local_glyph_rasterizer::DefaultLocalGlyphRasterizer::new(None)),
        ResponseType::Synchronous,
    );

    test.file_source.set_response(Kind::Glyphs, glyphs_response("test/fixtures/resources/glyphs.pbf"));

    *test.observer.glyphs_error.borrow_mut() = Some(Box::new(|_, _, _| panic!("glyphsError")));

    let available = Rc::new(Cell::new(false));
    let (a, run_loop) = (available.clone(), test.run_loop.clone());
    *test.requestor.glyphs_available.borrow_mut() = Some(Rc::new(move |_| {
        a.set(true);
        run_loop.stop();
    }));

    test.run("test/fixtures/resources/glyphs.pbf", dependencies(&['a', 'å', ' ']));
    assert!(available.get());
    test.end();
}

// Guards against silent drift between allowsFixedWidthGlyphGeneration's range
// coverage and the precomputed NON_IDEOGRAPH_GLYPH_RANGES_PER_FONT_STACK used
// for offline download progress estimation.
// gtest: i18n.GlyphRangeLocalGenerationCountMatchesConstant
#[test]
fn glyph_range_local_generation_count_matches_constant() {
    let mut locally_generated_ranges: u32 = 0;
    for i in 0..GLYPH_RANGES_PER_FONT_STACK {
        if i18n::glyph_range_is_entirely_locally_generated((i * GLYPHS_PER_GLYPH_RANGE) as u16) {
            locally_generated_ranges += 1;
        }
    }
    assert_eq!(GLYPH_RANGES_PER_FONT_STACK - locally_generated_ranges, NON_IDEOGRAPH_GLYPH_RANGES_PER_FONT_STACK);
}
