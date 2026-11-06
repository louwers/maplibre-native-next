// Ports vendor/earcut.hpp/test/test.cpp: the "empty" test and `areaTest`
// for every fixture (triangle count and area deviation). The libtess2 half
// of `areaTest` tests libtess2, the comparison tesselator, not earcut, and is
// not ported.

use earcut::earcut;
use earcut_test_fixtures::{CoordType, Fixture, load};

fn triangle_area(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    ((a[0] - c[0]) * (b[1] - a[1]) - (a[0] - b[0]) * (c[1] - a[1])).abs() / 2.0
}

fn triangles_area(vertices: &[[f64; 2]], indices: &[u32]) -> f64 {
    let mut area = 0.0;
    for t in indices.chunks(3) {
        area += triangle_area(vertices[t[0] as usize], vertices[t[1] as usize], vertices[t[2] as usize]);
    }
    area
}

fn ring_area(points: &[(f64, f64)]) -> f64 {
    let mut sum = 0.0;
    let len = points.len();
    let mut j = len.wrapping_sub(1);
    for i in 0..len {
        sum += (points[i].0 - points[j].0) * (points[i].1 + points[j].1);
        j = i;
    }
    sum.abs() / 2.0
}

fn polygon_area(rings: &[Vec<(f64, f64)>]) -> f64 {
    if rings.is_empty() {
        return 0.0;
    }
    let mut sum = ring_area(&rings[0]);
    for ring in &rings[1..] {
        sum -= ring_area(ring);
    }
    if sum < 0.0 { 0.0 } else { sum }
}

fn area_test(fixture: &Fixture) {
    let expected_area = polygon_area(&fixture.polygon);
    let expected_triangles = fixture.expected_triangles;

    let indices: Vec<u32> = match fixture.coord_type {
        CoordType::Short => earcut(&fixture.short_polygon()),
        CoordType::Double => earcut(&fixture.polygon),
    };
    let vertices: Vec<[f64; 2]> = fixture.polygon.iter().flatten().map(|&(x, y)| [x, y]).collect();

    let earcut_triangles = indices.len() / 3;
    assert_eq!(
        earcut_triangles, expected_triangles,
        "{}: {earcut_triangles} triangles when expected {expected_triangles}",
        fixture.name
    );

    if expected_triangles > 0 {
        let area = triangles_area(&vertices, &indices);
        let deviation = if expected_area == area {
            0.0
        } else if expected_area == 0.0 {
            f64::INFINITY
        } else {
            (area - expected_area).abs() / expected_area
        };
        assert!(
            deviation <= fixture.expected_earcut_deviation,
            "{}: earcut deviation {deviation} is not less than {}",
            fixture.name,
            fixture.expected_earcut_deviation
        );
    }
}

#[test]
fn empty() {
    let polygon: Vec<Vec<(i32, i32)>> = Vec::new();
    let indices: Vec<u32> = earcut(&polygon);
    assert!(indices.is_empty(), "empty input produces empty result");
}

macro_rules! fixture_tests {
    ($($name:ident),* $(,)?) => {$(
        #[test]
        fn $name() {
            area_test(&load(stringify!($name)));
        }
    )*};
}

fixture_tests!(
    bad_diagonals,
    bad_hole,
    boxy,
    building,
    collinear_diagonal,
    degenerate,
    dude,
    eberly_3,
    eberly_6,
    empty_square,
    hilbert,
    hole_touching_outer,
    hourglass,
    issue107,
    issue111,
    issue119,
    issue16,
    issue17,
    issue29,
    issue34,
    issue35,
    issue45,
    issue52,
    issue83,
    outside_ring,
    rain,
    self_touching,
    shared_points,
    simplified_us_border,
    steiner,
    touching2,
    touching3,
    touching_holes,
    water,
    water2,
    water3,
    water3b,
    water4,
    water_huge,
    water_huge2,
);

#[test]
fn every_fixture_has_a_test() {
    assert_eq!(earcut_test_fixtures::load_all().len(), 40);
}
