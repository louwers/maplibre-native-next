// Ports test/api/annotations.test.cpp (a module of the gtest_api test
// crate). The fixtures are test/fixtures/annotations/<name>/expected.png,
// compared with checkImage(.., 0.0002, 0.1) as on non-Windows C++.

use crate::support::*;
use maplibre_native_base::{Identifier, Point};
use mln_map::{
    Annotation, AnnotationID, BoundOptions, CameraOptions, FillAnnotation, LineAnnotation, MapDebugOptions, MapMode,
    ShapeAnnotationGeometry, SymbolAnnotation,
};
use mln_renderer::query::RenderedQueryOptions;
use mln_storage::stub_file_source::{ResponseType, StubFileSource};
use mln_style::PropertyValue;
use mln_style::image::Image;
use mln_util::Color;
use mln_util::feature::Feature;
use mln_util::geo::{EdgeInsets, LatLng, ScreenCoordinate, WrapMode};
use mln_util::image::PremultipliedImage;
use std::sync::Arc;

/// `namedImage(name)`: test/fixtures/sprites/<name>.png.
fn named_image(name: &str) -> PremultipliedImage {
    decode_image(&format!("test/fixtures/sprites/{name}.png"))
}

/// `std::make_unique<style::Image>(id, image, pixelRatio)`.
fn image(id: &str, image: PremultipliedImage, pixel_ratio: f32) -> Image {
    Image::new(id.to_owned(), image, pixel_ratio, false, Vec::new(), Vec::new(), None, None, None)
        .expect("style::Image")
}

/// `namedMarker(name)`.
fn named_marker(name: &str) -> Image {
    image(name, named_image(name), 1.0)
}

fn point(x: f64, y: f64) -> Point<f64> {
    Point::new(x, y)
}

fn line_string(points: &[(f64, f64)]) -> ShapeAnnotationGeometry {
    ShapeAnnotationGeometry::LineString(points.iter().map(|&(x, y)| point(x, y)).collect())
}

fn polygon(points: &[(f64, f64)]) -> ShapeAnnotationGeometry {
    ShapeAnnotationGeometry::Polygon(vec![points.iter().map(|&(x, y)| point(x, y)).collect()])
}

fn symbol(x: f64, y: f64, icon: &str) -> Annotation {
    Annotation::Symbol(SymbolAnnotation::new(point(x, y), icon))
}

fn ll(lat: f64, lon: f64) -> LatLng {
    LatLng::new(lat, lon).expect("LatLng")
}

fn cam() -> CameraOptions {
    CameraOptions::default()
}

/// `AnnotationTest`: a static 256x256 map over a `StubFileSource()`.
// TODO(port): `StubFileSource()` uses the MapTiler tile server options.
struct AnnotationTest(MapTest);

impl AnnotationTest {
    fn new() -> Self {
        let run_loop = mln_actor::RunLoop::new();
        let file_source = Arc::new(StubFileSource::new(ResponseType::Asynchronous));
        AnnotationTest(MapTest::create(run_loop, file_source, 1.0, MapMode::Static))
    }

    fn map(&self) -> std::cell::RefMut<'_, mln_map::Map> {
        self.0.map()
    }

    /// `test.map.getStyle().loadJSON(util::read_file("test/fixtures/api/empty.json"))`.
    fn load_empty_style(&self) {
        self.0.load_json(&read_file("test/fixtures/api/empty.json"));
    }

    fn add_annotation(&self, annotation: Annotation) -> AnnotationID {
        self.map().add_annotation(&annotation).expect("addAnnotation")
    }

    fn update_annotation(&self, id: AnnotationID, annotation: Annotation) {
        self.map().update_annotation(id, &annotation).expect("updateAnnotation");
    }

    /// `test.frontend.render(test.map)`.
    fn render(&self) {
        self.0.render();
    }

    fn check_rendering(&self, name: &str) {
        check_image(&format!("test/fixtures/annotations/{name}"), &self.0.render().image, 0.0002, 0.1);
    }

    /// The `ScreenBox` of the whole view.
    fn view_box(&self) -> (ScreenCoordinate, ScreenCoordinate) {
        let view_size = self.0.frontend.get_size();
        (
            ScreenCoordinate::new(0.0, 0.0),
            ScreenCoordinate::new(f64::from(view_size.width), f64::from(view_size.height)),
        )
    }

    /// `test.frontend.getRenderer()->queryRenderedFeatures(point)`.
    fn query_rendered_features_at(&self, point: &ScreenCoordinate) -> Vec<Feature> {
        self.0.frontend.with_renderer(|renderer| {
            renderer.query_rendered_features_at_point(point, &RenderedQueryOptions::default())
        })
    }

    /// `test.frontend.getRenderer()->queryRenderedFeatures(box, {})`.
    fn query_rendered_features_in(&self, (min, max): &(ScreenCoordinate, ScreenCoordinate)) -> Vec<Feature> {
        self.0.frontend.with_renderer(|renderer| {
            renderer.query_rendered_features_in_box(min, max, &RenderedQueryOptions::default())
        })
    }
}

/// `feature.id.get<uint64_t>()`.
fn uint_id(feature: &Feature) -> u64 {
    match feature.id {
        Identifier::Uint(id) => id,
        ref id => panic!("feature id {id:?} is not a uint64_t"),
    }
}

/// Sorts the features by id and erases repeated ids (`std::ranges::sort`
/// with `sortID`, then `std::unique` with `sameID`).
fn unique_by_id(mut features: Vec<Feature>) -> Vec<Feature> {
    features.sort_by_key(uint_id);
    features.dedup_by(|a, b| a.id == b.id);
    features
}

// gtest: Annotations.SymbolAnnotation
#[test]
fn symbol_annotation() {
    let test = AnnotationTest::new();

    test.load_empty_style();
    test.map().add_annotation_image(named_marker("default_marker"));
    test.add_annotation(symbol(0.0, 0.0, "default_marker"));
    test.check_rendering("point_annotation");

    //    auto size = test.frontend.getSize();
    //    auto screenBox = ScreenBox { {}, { double(size.width),
    //    double(size.height) } }; for (uint8_t zoom = test.map.getMinZoom();
    //    zoom <= test.map.getMaxZoom(); ++zoom) {
    //        test.map.jumpTo(CameraOptions().withZoom(zoom));
    //        test.checkRendering("point_annotation");
    //        EXPECT_EQ(test.map.queryPointAnnotations(screenBox).size(), 1u);
    //    }
}

// gtest: Annotations.SymbolAnnotationTileBoundary
#[test]
fn symbol_annotation_tile_boundary() {
    // Almost exactly the same as SymbolAnnotation test above, but offset my
    // fractions of a degree tests precision issue from
    // https://github.com/mapbox/mapbox-gl-native/issues/12472
    let test = AnnotationTest::new();

    test.load_empty_style();
    test.map().add_annotation_image(named_marker("default_marker"));
    test.add_annotation(symbol(0.000000000000001, 0.00000000000001, "default_marker"));
    test.map().jump_to(&cam().with_zoom(Some(10.0)));
    test.check_rendering("point_annotation");
}

// gtest: Annotations.LineAnnotation
#[test]
fn line_annotation() {
    let test = AnnotationTest::new();

    let line = line_string(&[(0.0, 0.0), (45.0, 45.0), (30.0, 0.0)]);
    let mut annotation = LineAnnotation::new(line);
    annotation.color = PropertyValue::Constant(Color::red());
    annotation.width = PropertyValue::Constant(5.0);

    test.load_empty_style();
    test.add_annotation(annotation.into());
    test.check_rendering("line_annotation");

    let max_zoom = test.map().get_bounds().max_zoom.expect("maxZoom");
    test.map().jump_to(&cam().with_zoom(Some(max_zoom)));
    test.check_rendering("line_annotation_max_zoom");
}

// gtest: Annotations.FillAnnotation
#[test]
fn fill_annotation() {
    let test = AnnotationTest::new();

    let polygon = polygon(&[(0.0, 0.0), (0.0, 45.0), (45.0, 45.0), (45.0, 0.0)]);
    let mut annotation = FillAnnotation::new(polygon);
    annotation.color = PropertyValue::Constant(Color::red());

    test.load_empty_style();
    test.add_annotation(annotation.into());
    test.check_rendering("fill_annotation");

    let max_zoom = test.map().get_bounds().max_zoom.expect("maxZoom");
    test.map().jump_to(&cam().with_zoom(Some(max_zoom)));
    test.check_rendering("fill_annotation_max_zoom");
}

// gtest: Annotations.AntimeridianAnnotationSmall
#[test]
fn antimeridian_annotation_small() {
    let test = AnnotationTest::new();

    let antimeridian = 180.0;
    test.map()
        .jump_to(&cam().with_center(Some(ll(0.0, antimeridian))).with_zoom(Some(0.0)));
    test.load_empty_style();

    let line = line_string(&[(antimeridian, 20.0), (antimeridian, -20.0)]);
    let mut line_annotation = LineAnnotation::new(line);
    line_annotation.color = PropertyValue::Constant(Color::red());
    line_annotation.width = PropertyValue::Constant(2.0);
    test.add_annotation(line_annotation.into());

    let polygon = polygon(&[
        (antimeridian + 10.0, 0.0),
        (antimeridian - 10.0, 10.0),
        (antimeridian - 10.0, -10.0),
    ]);
    let mut polygon_annotation = FillAnnotation::new(polygon);
    polygon_annotation.color = PropertyValue::Constant(Color::blue());
    test.add_annotation(polygon_annotation.into());

    test.check_rendering("antimeridian_annotation_small");
}

// gtest: Annotations.AntimeridianAnnotationLarge
#[test]
fn antimeridian_annotation_large() {
    let test = AnnotationTest::new();

    let antimeridian = 180.0;
    test.map()
        .jump_to(&cam().with_center(Some(ll(0.0, antimeridian))).with_zoom(Some(0.0)));
    test.load_empty_style();

    let line = line_string(&[(antimeridian, 20.0), (antimeridian, -20.0)]);
    let mut line_annotation = LineAnnotation::new(line);
    line_annotation.color = PropertyValue::Constant(Color::red());
    line_annotation.width = PropertyValue::Constant(2.0);
    test.add_annotation(line_annotation.into());

    let polygon = polygon(&[
        (antimeridian - 10.0, 0.0),
        (-antimeridian + 10.0, 10.0),
        (-antimeridian + 10.0, -10.0),
    ]);
    let mut polygon_annotation = FillAnnotation::new(polygon);
    polygon_annotation.color = PropertyValue::Constant(Color::blue());
    test.add_annotation(polygon_annotation.into());

    test.check_rendering("antimeridian_annotation_large");
}

// gtest: Annotations.OverlappingFillAnnotation
#[test]
fn overlapping_fill_annotation() {
    let test = AnnotationTest::new();

    let polygon = polygon(&[(0.0, 0.0), (0.0, 45.0), (45.0, 45.0), (45.0, 0.0)]);
    let mut underlaid_annotation = FillAnnotation::new(polygon.clone());
    underlaid_annotation.color = PropertyValue::Constant(Color::green());
    let mut overlaid_annotation = FillAnnotation::new(polygon);
    overlaid_annotation.color = PropertyValue::Constant(Color::red());

    test.load_empty_style();
    test.add_annotation(underlaid_annotation.into());
    test.add_annotation(overlaid_annotation.into());
    test.check_rendering("overlapping_fill_annotation");
}

// gtest: Annotations.AddMultiple
#[test]
fn add_multiple() {
    let test = AnnotationTest::new();

    test.load_empty_style();
    test.map().add_annotation_image(named_marker("default_marker"));
    test.add_annotation(symbol(-10.0, 0.0, "default_marker"));

    test.render();

    test.add_annotation(symbol(10.0, 0.0, "default_marker"));
    test.check_rendering("add_multiple");
}

// gtest: Annotations.NonImmediateAdd
#[test]
fn non_immediate_add() {
    let test = AnnotationTest::new();

    test.load_empty_style();
    test.render();

    let polygon = polygon(&[(0.0, 0.0), (0.0, 45.0), (45.0, 45.0), (45.0, 0.0)]);
    let mut annotation = FillAnnotation::new(polygon);
    annotation.color = PropertyValue::Constant(Color::red());

    test.add_annotation(annotation.into());
    test.check_rendering("non_immediate_add");
}

// gtest: Annotations.UpdateSymbolAnnotationGeometry
#[test]
fn update_symbol_annotation_geometry() {
    let test = AnnotationTest::new();

    test.load_empty_style();
    test.map().add_annotation_image(named_marker("default_marker"));
    test.map().add_annotation_image(named_marker("flipped_marker"));
    let point = test.add_annotation(symbol(0.0, 0.0, "default_marker"));

    test.render();

    test.update_annotation(point, symbol(-10.0, 0.0, "default_marker"));
    test.check_rendering("update_point");
}

// gtest: Annotations.UpdateSymbolAnnotationIcon
#[test]
fn update_symbol_annotation_icon() {
    let test = AnnotationTest::new();

    test.load_empty_style();
    test.map().add_annotation_image(named_marker("default_marker"));
    test.map().add_annotation_image(named_marker("flipped_marker"));
    let point = test.add_annotation(symbol(0.0, 0.0, "default_marker"));

    test.render();

    test.update_annotation(point, symbol(0.0, 0.0, "flipped_marker"));
    test.check_rendering("update_icon");
}

// gtest: Annotations.UpdateLineAnnotationGeometry
#[test]
fn update_line_annotation_geometry() {
    let test = AnnotationTest::new();

    let mut annotation = LineAnnotation::new(line_string(&[(0.0, 0.0), (45.0, 45.0), (30.0, 0.0)]));
    annotation.color = PropertyValue::Constant(Color::red());
    annotation.width = PropertyValue::Constant(5.0);

    test.load_empty_style();
    let line = test.add_annotation(annotation.clone().into());

    test.render();

    annotation.geometry = line_string(&[(0.0, 0.0), (-45.0, -45.0)]);
    test.update_annotation(line, annotation.into());
    test.check_rendering("update_line_geometry");
}

// gtest: Annotations.UpdateLineAnnotationStyle
#[test]
fn update_line_annotation_style() {
    let test = AnnotationTest::new();

    let mut annotation = LineAnnotation::new(line_string(&[(0.0, 0.0), (45.0, 45.0), (30.0, 0.0)]));
    annotation.color = PropertyValue::Constant(Color::red());
    annotation.width = PropertyValue::Constant(5.0);

    test.load_empty_style();
    let line = test.add_annotation(annotation.clone().into());

    test.render();

    annotation.color = PropertyValue::Constant(Color::green());
    annotation.width = PropertyValue::Constant(2.0);
    test.update_annotation(line, annotation.into());
    test.check_rendering("update_line_style");
}

// gtest: Annotations.UpdateFillAnnotationGeometry
#[test]
fn update_fill_annotation_geometry() {
    let test = AnnotationTest::new();

    let mut annotation = FillAnnotation::new(polygon(&[(0.0, 0.0), (0.0, 45.0), (45.0, 45.0), (45.0, 0.0)]));
    annotation.color = PropertyValue::Constant(Color::red());

    test.load_empty_style();
    let fill = test.add_annotation(annotation.clone().into());

    test.render();

    annotation.geometry = polygon(&[(0.0, 0.0), (0.0, 45.0), (45.0, 0.0)]);
    test.update_annotation(fill, annotation.into());
    test.check_rendering("update_fill_geometry");
}

// gtest: Annotations.UpdateFillAnnotationStyle
#[test]
fn update_fill_annotation_style() {
    let test = AnnotationTest::new();

    let polygon = polygon(&[(0.0, 0.0), (0.0, 45.0), (45.0, 45.0), (45.0, 0.0)]);
    let mut annotation = FillAnnotation::new(polygon);
    annotation.color = PropertyValue::Constant(Color::red());

    test.load_empty_style();
    let fill = test.add_annotation(annotation.clone().into());

    test.render();

    annotation.color = PropertyValue::Constant(Color::green());
    test.update_annotation(fill, annotation.into());
    test.check_rendering("update_fill_style");
}

// gtest: Annotations.RemovePoint
#[test]
fn remove_point() {
    let test = AnnotationTest::new();

    test.load_empty_style();
    test.map().add_annotation_image(named_marker("default_marker"));
    let point = test.add_annotation(symbol(0.0, 0.0, "default_marker"));

    test.render();

    test.map().remove_annotation(point);
    test.check_rendering("remove_point");
}

// gtest: Annotations.RemoveShape
#[test]
fn remove_shape() {
    let test = AnnotationTest::new();

    let line = line_string(&[(0.0, 0.0), (45.0, 45.0)]);
    let mut annotation = LineAnnotation::new(line);
    annotation.color = PropertyValue::Constant(Color::red());
    annotation.width = PropertyValue::Constant(5.0);

    test.load_empty_style();
    let shape = test.add_annotation(annotation.into());

    test.render();

    test.map().remove_annotation(shape);
    test.check_rendering("remove_shape");
}

// gtest: Annotations.ImmediateRemoveShape
#[test]
fn immediate_remove_shape() {
    let test = AnnotationTest::new();

    let shape = test.add_annotation(LineAnnotation::new(ShapeAnnotationGeometry::LineString(Vec::new())).into());
    test.map().remove_annotation(shape);
    test.load_empty_style();

    test.render();
}

// gtest: Annotations.SwitchStyle
#[test]
fn switch_style() {
    let test = AnnotationTest::new();

    test.load_empty_style();
    test.map().add_annotation_image(named_marker("default_marker"));
    test.add_annotation(symbol(0.0, 0.0, "default_marker"));

    test.render();

    test.load_empty_style();
    test.check_rendering("switch_style");
}

// gtest: Annotations.ReaddImage
#[test]
fn readd_image() {
    let test = AnnotationTest::new();

    test.load_empty_style();
    test.map().add_annotation_image(named_marker("default_marker"));
    test.add_annotation(symbol(0.0, 0.0, "default_marker"));

    test.render();

    test.map()
        .add_annotation_image(image("default_marker", named_image("flipped_marker"), 1.0));
    test.check_rendering("readd_image");
}

// gtest: Annotations.QueryRenderedFeatures
#[test]
fn query_rendered_features() {
    let test = AnnotationTest::new();

    test.load_empty_style();
    test.map().add_annotation_image(named_marker("default_marker"));
    test.add_annotation(symbol(0.0, 0.0, "default_marker"));
    test.add_annotation(symbol(0.0, 50.0, "default_marker"));

    test.render();

    // Batch conversion of latLngs to pixels
    let points = test.map().pixels_for_lat_lngs(&[ll(0.0, 0.0), ll(50.0, 0.0)]);
    assert_eq!(2, points.len());
    // Single conversion of latLng to pixel
    let point0 = test.map().pixel_for_lat_lng(&ll(0.0, 0.0));
    assert_near(points[0].x, point0.x, 1e-8);
    assert_near(points[0].y, point0.y, 1e-8);

    let point1 = test.map().pixel_for_lat_lng(&ll(50.0, 0.0));
    assert_near(points[1].x, point1.x, 1e-8);
    assert_near(points[1].y, point1.y, 1e-8);

    let features = test.query_rendered_features_at(&point0);
    assert_eq!(features.len(), 1);
    assert_ne!(features[0].id, Identifier::Null);
    assert_eq!(features[0].id, Identifier::Uint(0));

    let features2 = test.query_rendered_features_at(&point1);
    assert_eq!(features2.len(), 1);
    assert_ne!(features[0].id, Identifier::Null);
    assert_eq!(features2[0].id, Identifier::Uint(1));
}

// gtest: Annotations.QueryFractionalZoomLevels
#[test]
fn query_fractional_zoom_levels() {
    let test = AnnotationTest::new();

    let view_box = test.view_box();

    test.load_empty_style();
    test.map().add_annotation_image(named_marker("default_marker"));

    let mut ids: Vec<AnnotationID> = Vec::new();
    for longitude in (0..10).step_by(2) {
        for latitude in (0..10).step_by(2) {
            ids.push(test.add_annotation(symbol(f64::from(latitude), f64::from(longitude), "default_marker")));
        }
    }

    test.map()
        .jump_to(&cam().with_center(Some(ll(5.0, 5.0))).with_zoom(Some(0.0)));
    for zoom_steps in 10u16..=20 {
        test.map().jump_to(&cam().with_zoom(Some(f64::from(zoom_steps) / 10.0)));
        test.render();
        let features = test.query_rendered_features_in(&view_box);

        // Filter out repeated features.
        // See 'edge-cases/null-island' query-test for reference.
        let features = unique_by_id(features);
        assert_eq!(features.len(), ids.len(), "at zoom {}", f64::from(zoom_steps) / 10.0);
    }
}

// gtest: Annotations.VisibleFeatures
#[test]
fn visible_features() {
    let test = AnnotationTest::new();

    let view_box = test.view_box();

    test.load_empty_style();
    test.map().add_annotation_image(named_marker("default_marker"));
    test.map()
        .jump_to(&cam().with_center(Some(ll(5.0, 5.0))).with_zoom(Some(3.0)));

    let mut ids: Vec<AnnotationID> = Vec::new();
    for longitude in (0..10).step_by(2) {
        for latitude in (0..=10).step_by(2) {
            ids.push(test.add_annotation(symbol(f64::from(latitude), f64::from(longitude), "default_marker")));
        }
    }

    // Change bearing *after* adding annotations causes them to be reordered.
    test.map().jump_to(&cam().with_bearing(Some(45.0)));
    test.render();

    let features = unique_by_id(test.query_rendered_features_in(&view_box));
    assert_eq!(features.len(), ids.len());

    test.map().jump_to(&cam().with_zoom(Some(4.0)).with_bearing(Some(0.0)));
    test.render();
    let features = unique_by_id(test.query_rendered_features_in(&view_box));
    assert_eq!(features.len(), ids.len());
}

// gtest: Annotations.ViewFrustumCulling
#[test]
fn view_frustum_culling() {
    // The purpose of the test is to control that annotations outside screen
    // rectangle are not rendered for different camera setup, especially when
    // using edge insets - viewport center is then offsetted.

    // Important premise of this test is "static const float viewportPadding =
    // 100;" as defined in collision_index.cpp: tests using edge insets are
    // written so that padding is 128 (half of viewSize width). If increasing
    // viewportPadding, increase the padding in test cases below.
    let test = AnnotationTest::new();

    let view_size = test.0.frontend.get_size();
    let (width, height) = (f64::from(view_size.width), f64::from(view_size.height));
    let view_box = test.view_box();

    test.load_empty_style();
    test.map().add_annotation_image(named_marker("default_marker"));
    let center = ll(5.0, 5.0);
    test.map().jump_to(&cam().with_center(Some(center)).with_zoom(Some(3.0)));

    // Batch conversion of pixels to latLngs
    let batch_lat_lngs = test.map().lat_lngs_for_pixels(
        &[
            ScreenCoordinate::new(0.0, 0.0),
            ScreenCoordinate::new(width, height),
            ScreenCoordinate::new(width, 0.0),
            ScreenCoordinate::new(0.0, height),
        ],
        WrapMode::Unwrapped,
    );
    assert_eq!(4, batch_lat_lngs.len());

    // Single conversion of pixel to latLng
    let tl = test.map().lat_lng_for_pixel(&ScreenCoordinate::new(0.0, 0.0), WrapMode::Unwrapped);
    assert_near(batch_lat_lngs[0].latitude(), tl.latitude(), 1e-8);
    assert_near(batch_lat_lngs[0].longitude(), tl.longitude(), 1e-8);

    let br = test.map().lat_lng_for_pixel(&ScreenCoordinate::new(width, height), WrapMode::Unwrapped);
    assert_near(batch_lat_lngs[1].latitude(), br.latitude(), 1e-8);
    assert_near(batch_lat_lngs[1].longitude(), br.longitude(), 1e-8);

    let bl = test.map().lat_lng_for_pixel(&ScreenCoordinate::new(width, 0.0), WrapMode::Unwrapped);
    assert_near(batch_lat_lngs[2].latitude(), bl.latitude(), 1e-8);
    assert_near(batch_lat_lngs[2].longitude(), bl.longitude(), 1e-8);

    let tr = test.map().lat_lng_for_pixel(&ScreenCoordinate::new(0.0, height), WrapMode::Unwrapped);
    assert_near(batch_lat_lngs[3].latitude(), tr.latitude(), 1e-8);
    assert_near(batch_lat_lngs[3].longitude(), tr.longitude(), 1e-8);

    let lat_lngs = [tl, bl, tr, br, center];

    let mut ids: Vec<AnnotationID> = Vec::new();
    for lat_lng in lat_lngs {
        ids.push(test.add_annotation(symbol(lat_lng.longitude(), lat_lng.latitude(), "default_marker")));
    }

    let padding = |top: f64, left: f64, bottom: f64, right: f64| {
        Some(EdgeInsets::new(top, left, bottom, right).expect("EdgeInsets"))
    };
    let expected_visible_for_camera: Vec<(CameraOptions, Vec<u64>)> = vec![
        // Start with all markers visible.
        (cam(), vec![0, 1, 2, 3, 4]),
        // Move center to topLeft: only former center and top left (now center) are visible.
        (cam().with_center(Some(tl)), vec![0, 4]),
        // Reset center. With pitch: only top row markers and center are visible.
        (cam().with_center(Some(center)).with_pitch(Some(45.0)), vec![0, 1, 4]),
        // Reset pitch, and use padding to move viewport center: only topleft and center are visible.
        (
            cam().with_pitch(Some(0.0)).with_padding(padding(height * 0.5, width * 0.5, 0.0, 0.0)),
            vec![0, 4],
        ),
        // Use opposite padding to move viewport center: only bottom right and center are visible.
        (
            cam().with_pitch(Some(0.0)).with_padding(padding(0.0, 0.0, height * 0.5, width * 0.5)),
            vec![3, 4],
        ),
        // Use top padding to move viewport center: top row and center are visible.
        (
            cam().with_pitch(Some(0.0)).with_padding(padding(height * 0.5, 0.0, 0.0, 0.0)),
            vec![0, 1, 4],
        ),
        // Use bottom padding: only bottom right and center are visible.
        (
            cam().with_pitch(Some(0.0)).with_padding(padding(0.0, 0.0, height * 0.5, 0.0)),
            vec![2, 3, 4],
        ),
        // Left padding and pitch: top left, bottom left and center are visible.
        (
            cam().with_pitch(Some(45.0)).with_padding(padding(0.0, width * 0.5, 0.0, 0.0)),
            vec![0, 2, 4],
        ),
    ];

    for (i, (camera, expected)) in expected_visible_for_camera.iter().enumerate() {
        test.map().jump_to(camera);
        test.render();
        let features = test.query_rendered_features_in(&view_box);
        for &id in expected {
            assert!(
                features.iter().any(|feature| id == uint_id(feature)),
                "Point with id {id} is missing in test case {i}"
            );
            assert_eq!(features.len(), expected.len(), " in test case {i}");
        }
    }
}

// gtest: Annotations.TopOffsetPixels
#[test]
fn top_offset_pixels() {
    let test = AnnotationTest::new();

    test.map().add_annotation_image(named_marker("default_marker"));
    assert_eq!(test.map().get_top_offset_pixels_for_annotation_image("default_marker"), -28.0);
}

// gtest: Annotations.DebugEmpty
#[test]
fn debug_empty() {
    // This test should render nothing, not even the tile borders. Tile borders
    // are only rendered when there is an actual tile we're trying to render,
    // but since there is no annotation, we should not render them.
    let test = AnnotationTest::new();

    test.load_empty_style();
    test.map().set_debug(MapDebugOptions::TILE_BORDERS);
    test.map().jump_to(&cam().with_zoom(Some(1.0)));

    test.check_rendering("debug_empty");
}

// gtest: Annotations.DebugSparse
#[test]
#[ignore = "OpenGL-only in C++ (returns early on other backends, so the C++ WebGPU and Metal builds never compare debug_sparse; it passes on the port's Metal run, unchecked on lavapipe); see annotations.test.cpp:580"]
fn debug_sparse() {
    // C++: `if (gfx::Backend::GetType() != gfx::Backend::Type::OpenGL) return;`

    // This test should only render the top right tile with the associated tile
    // border, but no other tiles because they're all empty.
    let test = AnnotationTest::new();

    test.load_empty_style();
    test.map().set_debug(MapDebugOptions::TILE_BORDERS);
    test.map().jump_to(&cam().with_zoom(Some(1.0)));
    test.map().add_annotation_image(named_marker("default_marker"));
    test.add_annotation(symbol(10.0, 10.0, "default_marker"));

    test.check_rendering("debug_sparse");
}

// gtest: Annotations.ChangeMaxZoom
#[test]
fn change_max_zoom() {
    let test = AnnotationTest::new();

    let line = line_string(&[(0.0, 0.0), (45.0, 45.0), (30.0, 0.0)]);
    let mut annotation = LineAnnotation::new(line);
    annotation.color = PropertyValue::Constant(Color::red());
    annotation.width = PropertyValue::Constant(5.0);

    test.map().set_bounds(&BoundOptions::default().with_max_zoom(6.0)).expect("setBounds");
    test.load_empty_style();
    test.add_annotation(annotation.into());
    test.map().set_bounds(&BoundOptions::default().with_max_zoom(14.0)).expect("setBounds");
    let max_zoom = test.map().get_bounds().max_zoom.expect("maxZoom");
    test.map().jump_to(&cam().with_zoom(Some(max_zoom)));
    test.check_rendering("line_annotation_max_zoom");
}
