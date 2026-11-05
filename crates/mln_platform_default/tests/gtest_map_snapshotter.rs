// Ports test/map/map_snapshotter.test.cpp (a module of the gtest_map test
// crate). The server tests use the in-process HTTP test server
// (//crates/mln_test, a free port per test instead of 127.0.0.1:3000) and
// take `network_serial()` (NetworkStatus and the FileSourceManager's file
// sources are process-wide; gtest runs one test at a time).

use crate::support::*;
use mln_actor::{RunLoop, Timer};
use mln_platform_default::map_snapshotter::{MapSnapshotter, MapSnapshotterObserver};
use mln_storage::{ClientOptions, ResourceOptions};
use mln_style::PropertyValue;
use mln_style::layer::Layer as _;
use mln_style::layers::fill_layer::FillLayer;
use mln_test::HttpServer;
use mln_util::Color;
use mln_util::size::Size;
use std::rc::Rc;

const GREEN_BACKGROUND_STYLE: &str = r#"{
        "version": 8,
        "layers": [{
            "id": "background",
            "type": "background",
            "paint": {"background-color": "green"}
        }]
    }"#;

#[derive(Default)]
struct SnapshotterObserver {
    did_finish_loading_style_callback: CallbackSlot<()>,
}

impl MapSnapshotterObserver for SnapshotterObserver {
    fn on_did_finish_loading_style(&self) {
        self.did_finish_loading_style_callback.call(());
    }
}

// gtest: MapSnapshotter.setStyleJSON
#[test]
fn set_style_json() {
    let run_loop = RunLoop::new();
    let snapshotter = MapSnapshotter::new(
        Size::new(32, 16),
        1.0,
        &ResourceOptions::default(),
        &ClientOptions::default(),
    );
    snapshotter.set_style_json(GREEN_BACKGROUND_STYLE);

    let stop = run_loop.clone();
    snapshotter.snapshot(Box::new(move |error, image, _, _, _| {
        assert!(error.is_none(), "{error:?}");
        assert_eq!(32, image.size.width);
        assert_eq!(16, image.size.height);
        stop.stop();
    }));

    run_with_timeout(&run_loop, 60);
}

// gtest: MapSnapshotter.setSize
#[test]
fn set_size() {
    let run_loop = RunLoop::new();
    let snapshotter = MapSnapshotter::new(
        Size::new(32, 16),
        1.0,
        &ResourceOptions::default(),
        &ClientOptions::default(),
    );
    snapshotter.set_style_json(GREEN_BACKGROUND_STYLE);

    snapshotter.set_size(Size::new(16, 32)).expect("setSize");

    let stop = run_loop.clone();
    snapshotter.snapshot(Box::new(move |error, image, _, _, _| {
        assert!(error.is_none(), "{error:?}");
        assert_eq!(16, image.size.width);
        assert_eq!(32, image.size.height);
        stop.stop();
    }));

    run_with_timeout(&run_loop, 60);
}

// gtest: MapSnapshotter.setStyleURL
#[test]
fn set_style_url() {
    let _serial = network_serial();
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let snapshotter = MapSnapshotter::new(
        Size::new(64, 32),
        1.0,
        &ResourceOptions::default(),
        &ClientOptions::default(),
    );
    snapshotter.set_style_url(&server.url("/online/style.json"));
    let stop = run_loop.clone();
    snapshotter.snapshot(Box::new(move |error, image, _, _, _| {
        assert!(error.is_none(), "{error:?}");
        assert_eq!(64, image.size.width);
        assert_eq!(32, image.size.height);
        stop.stop();
    }));

    run_with_timeout(&run_loop, 60);
}

// gtest: MapSnapshotter.cancel
#[test]
fn cancel() {
    let _serial = network_serial();
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let snapshotter = MapSnapshotter::new(
        Size::new(1, 1),
        1.0,
        &ResourceOptions::default(),
        &ClientOptions::default(),
    );
    snapshotter.set_style_url(&server.url("/online/style.json"));
    snapshotter.snapshot(Box::new(|_, _, _, _, _| panic!("a cancelled snapshot called its callback")));

    snapshotter.cancel();

    let timer = Timer::new();
    let stop = run_loop.clone();
    timer.start(
        mln_util::chrono::Duration::from_millis(100),
        mln_util::chrono::Duration::zero(),
        move || stop.stop(),
    );

    run_with_timeout(&run_loop, 60);
}

// gtest: MapSnapshotter.runtimeStyling
#[test]
fn runtime_styling() {
    let _serial = network_serial();
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let observer = Rc::new(SnapshotterObserver::default());
    let snapshotter = Rc::new(MapSnapshotter::with_observer(
        Size::new(256, 128),
        1.0,
        &ResourceOptions::default(),
        &ClientOptions::default(),
        observer.clone(),
    ));
    snapshotter.set_style_url(&server.url("/online/style.json"));

    let (weak_snapshotter, stop) = (Rc::downgrade(&snapshotter), run_loop.clone());
    observer.did_finish_loading_style_callback.set(move |_| {
        let Some(snapshotter) = weak_snapshotter.upgrade() else {
            return;
        };
        let mut fill_layer = FillLayer::new("green_fill", "mapbox");
        fill_layer.set_source_layer("water");
        fill_layer.set_fill_color(&PropertyValue::Constant(Color::new(0.25, 0.88, 0.82, 1.0)));
        snapshotter
            .with_style(|style| style.add_layer(Box::new(fill_layer), None))
            .expect("addLayer");
        let stop = stop.clone();
        snapshotter.snapshot(Box::new(move |error, image, _, _, _| {
            assert!(error.is_none(), "{error:?}");
            assert_eq!(256, image.size.width);
            assert_eq!(128, image.size.height);
            check_image("test/fixtures/map/map_snapshotter_overlay", &image, 0.005, 0.1);
            stop.stop();
        }));
    });

    run_with_timeout(&run_loop, 60);
}

// gtest: MapSnapshotter.annotation
#[test]
#[ignore = "TODO(port): annotations (MapSnapshotter::addAnnotationImage/addAnnotation over Map::addAnnotation*, AnnotationManager, the annotation source); see map_snapshotter.test.cpp:150"]
fn annotation() {
    // Symbol, fill and line annotations show in the snapshot of
    // /online/style.json (test/fixtures/map/map_snapshotter_annotation,
    // 0.005, 0.1).
}
