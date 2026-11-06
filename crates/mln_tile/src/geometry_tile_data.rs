// Ports the free functions of src/mln/tile/geometry_tile_data.cpp
// (signedArea, toWagyuPath, toGeometryCollection, fixupPolygons,
// classifyRings, limitHoles, both convertGeometry overloads, convertFeature)
// and `ToGeometryCollection` from geometry_tile_data.hpp. The interfaces
// (GeometryTileFeature & co.) are in mln_tile_id::geometry_tile_data.
//
// TODO(port): roundPolygonCorners (fill-extrusion rounded corners; needs
// util/math.hpp dist/normal/perp/crossProduct/angle_between).

use crate::stl_algo::nth_element;
use feature::Feature;
use geometry::{Geometry, LineString, LinearRing, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use mln_tile_id::{CanonicalTileID, GeometryCollection, GeometryCoordinates, GeometryTileFeature};
use mln_util::constants::EXTENT;
use mln_util::geometry::FeatureType;
use std::f64::consts::PI;
use vector_tile::Error;

/// `signedArea` (anonymous namespace). The products are C++ `int`
/// arithmetic, which can overflow for coordinates near the int16 limits;
/// x86-64 wraps.
fn signed_area(ring: &GeometryCoordinates) -> f64 {
    let mut sum = 0.0;

    let len = ring.len();
    let mut j = len.wrapping_sub(1);
    for i in 0..len {
        let p1 = ring[i];
        let p2 = ring[j];
        let dx = i32::from(p2.x) - i32::from(p1.x);
        let sy = i32::from(p1.y) + i32::from(p2.y);
        sum += f64::from(dx.wrapping_mul(sy));
        j = i;
    }

    sum
}

/// `toWagyuPath`.
fn to_wagyu_path(ring: &GeometryCoordinates) -> LinearRing<i32> {
    let mut result = LinearRing::with_capacity(ring.len());
    for p in ring {
        result.push(Point::new(i32::from(p.x), i32::from(p.y)));
    }
    result
}

/// `toGeometryCollection(MultiPolygon<int16_t>&&)`.
fn multipolygon_to_geometry_collection(multipolygon: MultiPolygon<i16>) -> GeometryCollection {
    let mut result = GeometryCollection::new();
    for polygon in multipolygon {
        for ring in polygon {
            result.push(ring);
        }
    }
    result
}

/// `fixupPolygons`: fix up possibly-non-V2-compliant polygon geometry using
/// angus clipper. The result is guaranteed to have correctly wound, strictly
/// simple rings. wagyu throws `std::runtime_error` in a few degenerate
/// cases; those propagate as in C++.
pub fn fixup_polygons(rings: &GeometryCollection) -> Result<GeometryCollection, Error> {
    let mut clipper = wagyu::Wagyu::new();

    for ring in rings.iter() {
        clipper.add_ring(&to_wagyu_path(ring), wagyu::PolygonType::Subject).map_err(Error::Runtime)?;
    }

    let mut multipolygon: MultiPolygon<i16> = MultiPolygon::new();
    clipper
        .execute(wagyu::ClipType::Union, &mut multipolygon, wagyu::FillType::EvenOdd, wagyu::FillType::EvenOdd)
        .map_err(Error::Runtime)?;

    Ok(multipolygon_to_geometry_collection(multipolygon))
}

/// `classifyRings`: classifies an array of rings into polygons with outer
/// rings and holes.
pub fn classify_rings(rings: &GeometryCollection) -> Vec<GeometryCollection> {
    let mut polygons: Vec<GeometryCollection> = Vec::new();

    let len = rings.len();

    if len <= 1 {
        polygons.push(rings.clone());
        return polygons;
    }

    let mut polygon = GeometryCollection::new();
    let mut ccw: i8 = 0;

    for ring in rings.iter() {
        let area = signed_area(ring);
        if area == 0.0 {
            continue;
        }

        if ccw == 0 {
            ccw = if area < 0.0 { -1 } else { 1 };
        }

        if ccw == (if area < 0.0 { -1 } else { 1 }) && !polygon.is_empty() {
            polygons.push(std::mem::take(&mut polygon));
        }

        polygon.push(ring.clone());
    }

    if !polygon.is_empty() {
        polygons.push(polygon);
    }

    polygons
}

/// `limitHoles`: truncate polygon to the largest `maxHoles` inner rings by
/// area, in the order libstdc++'s `std::nth_element` leaves them.
pub fn limit_holes(polygon: &mut GeometryCollection, max_holes: u32) {
    let max_holes = max_holes as usize;
    if polygon.len() > 1 + max_holes {
        nth_element(&mut polygon[1..], max_holes, |a, b| signed_area(a).abs() > signed_area(b).abs());
        polygon.truncate(1 + max_holes);
    }
}

/// `convertGeometry(const GeometryTileFeature&, const CanonicalTileID&)`:
/// tile coordinates to longitude/latitude. Errors: `std::out_of_range` from
/// `geometries.at(0)` for a point feature without rings. For
/// `FeatureType::Unknown` the C++ asserts (debug builds) and returns
/// `Point(NaN, NaN)`.
pub fn convert_geometry(
    geometry_tile_feature: &dyn GeometryTileFeature,
    tile_id: &CanonicalTileID,
) -> Result<Geometry<f64>, Error> {
    let size = f64::from(EXTENT) * 2f64.powi(i32::from(tile_id.z));
    let x0 = f64::from(EXTENT) * f64::from(tile_id.x);
    let y0 = f64::from(EXTENT) * f64::from(tile_id.y);

    let tile_coordinates_to_lat_lng = |p: &Point<i16>| {
        let y2 = 180.0 - (f64::from(p.y) + y0) * 360.0 / size;
        Point::new((f64::from(p.x) + x0) * 360.0 / size - 180.0, (y2 * PI / 180.0).exp().atan() * 360.0 / PI - 90.0)
    };

    let geometries = geometry_tile_feature.get_geometries();

    match geometry_tile_feature.get_type() {
        FeatureType::Unknown => {
            debug_assert!(false, "convertGeometry of a feature of unknown type");
            Ok(Geometry::Point(Point::new(f64::NAN, f64::NAN)))
        }

        FeatureType::Point => {
            let mut multi_point: MultiPoint<f64> = MultiPoint::new();
            let first = geometries.first().ok_or_else(|| Error::vector_range_check(0, geometries.len()))?;
            for p in first {
                multi_point.push(tile_coordinates_to_lat_lng(p));
            }
            if multi_point.len() == 1 {
                Ok(Geometry::Point(multi_point[0]))
            } else {
                Ok(Geometry::MultiPoint(multi_point))
            }
        }

        FeatureType::LineString => {
            let mut multi_line_string: MultiLineString<f64> = MultiLineString::new();
            for g in geometries.iter() {
                let mut line_string: LineString<f64> = LineString::new();
                for p in g {
                    line_string.push(tile_coordinates_to_lat_lng(p));
                }
                multi_line_string.push(line_string);
            }
            if multi_line_string.len() == 1 {
                Ok(Geometry::LineString(multi_line_string.swap_remove(0)))
            } else {
                Ok(Geometry::MultiLineString(multi_line_string))
            }
        }

        FeatureType::Polygon => {
            let mut multi_polygon: MultiPolygon<f64> = MultiPolygon::new();
            for pg in classify_rings(geometries) {
                let mut polygon: Polygon<f64> = Polygon::new();
                for r in pg.iter() {
                    let mut linear_ring: LinearRing<f64> = LinearRing::new();
                    for p in r {
                        linear_ring.push(tile_coordinates_to_lat_lng(p));
                    }
                    polygon.push(linear_ring);
                }
                multi_polygon.push(polygon);
            }
            if multi_polygon.len() == 1 {
                Ok(Geometry::Polygon(multi_polygon.swap_remove(0)))
            } else {
                Ok(Geometry::MultiPolygon(multi_polygon))
            }
        }
    }
}

/// `static_cast<int16_t>(double)` as clang emits it on x86-64: cvttsd2si to
/// int32 (0x80000000 for NaN and out-of-range values; undefined behaviour in
/// C++), then the low 16 bits.
fn int16_cast_x86_64(v: f64) -> i16 {
    let i = if v.is_nan() || v >= 2147483648.0 || v <= -2147483649.0 { i32::MIN } else { v as i32 };
    i as i16
}

/// `convertGeometry(const Feature::geometry_type&, const CanonicalTileID&)`:
/// longitude/latitude to tile coordinates. The C++ clamps *after* the
/// narrowing cast, so the clamp has no effect; the cast is reproduced as
/// x86-64 executes it (see `int16_cast_x86_64`).
pub fn convert_geometry_to_tile(geometry: &Geometry<f64>, tile_id: &CanonicalTileID) -> GeometryCollection {
    let size = f64::from(EXTENT) * 2f64.powi(i32::from(tile_id.z));
    let x0 = f64::from(EXTENT) * f64::from(tile_id.x);
    let y0 = f64::from(EXTENT) * f64::from(tile_id.y);

    let lat_lon_to_tile_coodinates = |c: &Point<f64>| {
        let x = (c.x + 180.0) * size / 360.0 - x0;
        let px = int16_cast_x86_64(x);

        let y = (180.0 - (((c.y + 90.0) * PI / 360.0).tan().ln() * 180.0 / PI)) * size / 360.0 - y0;
        let py = int16_cast_x86_64(y);

        Point::new(px, py)
    };

    let ring =
        |points: &[Point<f64>]| -> GeometryCoordinates { points.iter().map(&lat_lon_to_tile_coodinates).collect() };

    match geometry {
        Geometry::Point(point) => GeometryCollection::from(vec![vec![lat_lon_to_tile_coodinates(point)]]),
        Geometry::MultiPoint(points) => GeometryCollection::from(vec![ring(points)]),
        Geometry::LineString(line_string) => GeometryCollection::from(vec![ring(line_string)]),
        Geometry::MultiLineString(line_strings) => {
            GeometryCollection::from(line_strings.iter().map(|line| ring(line)).collect::<Vec<_>>())
        }
        Geometry::Polygon(polygon) => GeometryCollection::from(polygon.iter().map(|r| ring(r)).collect::<Vec<_>>()),
        Geometry::MultiPolygon(polygons) => {
            GeometryCollection::from(polygons.iter().flat_map(|pg| pg.iter().map(|r| ring(r))).collect::<Vec<_>>())
        }
        Geometry::Empty | Geometry::GeometryCollection(_) => GeometryCollection::new(),
    }
}

/// `convertFeature`: convert from GeometryTileFeature to Feature
/// (eventually we should eliminate GeometryTileFeature).
pub fn convert_feature(
    geometry_tile_feature: &dyn GeometryTileFeature,
    tile_id: &CanonicalTileID,
) -> Result<Feature<f64>, Error> {
    let mut feature = Feature::new(convert_geometry(geometry_tile_feature, tile_id)?);
    feature.properties = geometry_tile_feature.get_properties().clone();
    feature.id = geometry_tile_feature.get_id();
    Ok(feature)
}

/// `ToGeometryCollection`.
pub fn to_geometry_collection(geom: &Geometry<i16>) -> GeometryCollection {
    match geom {
        Geometry::Empty => GeometryCollection::new(),
        Geometry::Point(p) => GeometryCollection::from(vec![vec![*p]]),
        Geometry::MultiPoint(g) | Geometry::LineString(g) => GeometryCollection::from(vec![g.clone()]),
        Geometry::MultiLineString(g) | Geometry::Polygon(g) => GeometryCollection::from(g.clone()),
        Geometry::MultiPolygon(g) => {
            GeometryCollection::from(g.iter().flat_map(|polygon| polygon.iter().cloned()).collect::<Vec<_>>())
        }
        Geometry::GeometryCollection(_) => GeometryCollection::new(),
    }
}

#[cfg(test)]
mod tests {
    // Ports test/tile/geometry_tile_data.test.cpp.
    use super::*;

    fn ring(points: &[(i16, i16)]) -> GeometryCoordinates {
        points.iter().map(|&(x, y)| Point::new(x, y)).collect()
    }

    fn collection(rings: &[&[(i16, i16)]]) -> GeometryCollection {
        GeometryCollection::from(rings.iter().map(|r| ring(r)).collect::<Vec<_>>())
    }

    fn test_signed_area(ring: &GeometryCoordinates) -> f64 {
        signed_area(ring)
    }

    // gtest: GeometryTileData.classifyRings1
    #[test]
    fn classify_rings1() {
        let polygons = classify_rings(&collection(&[&[(0, 0), (0, 40), (40, 40), (40, 0), (0, 0)]]));

        // output: 1 polygon
        assert_eq!(polygons.len(), 1);
        // output: polygon 1 has 1 exterior
        assert_eq!(polygons[0].len(), 1);
    }

    // gtest: GeometryTileData.classifyRings2
    #[test]
    fn classify_rings2() {
        let polygons = classify_rings(&collection(&[
            &[(0, 0), (0, 40), (40, 40), (40, 0), (0, 0)],
            &[(10, 10), (20, 10), (20, 20), (10, 10)],
        ]));

        // output: 1 polygon
        assert_eq!(polygons.len(), 1);
        // output: polygon 1 has 1 exterior, 1 interior
        assert_eq!(polygons[0].len(), 2);
    }

    // gtest: GeometryTileData.limitHoles1
    #[test]
    fn limit_holes1() {
        let mut polygon = collection(&[
            &[(0, 0), (0, 40), (40, 40), (40, 0), (0, 0)],
            &[(30, 30), (32, 30), (32, 32), (30, 30)],
            &[(10, 10), (20, 10), (20, 20), (10, 10)],
        ]);

        limit_holes(&mut polygon, 1);

        // output: polygon 1 has 1 exterior, 1 interior
        assert_eq!(polygon.len(), 2);

        // ensure we've kept the right rings (ones with largest areas)
        assert_eq!(polygon[0][0].x, 0);
        assert_eq!(polygon[1][0].x, 10);
    }

    // gtest: GeometryTileData.limitHoles2
    #[test]
    fn limit_holes2() {
        let mut polygon = collection(&[
            &[(0, 0), (0, 40), (40, 40), (40, 0), (0, 0)],
            &[(10, 10), (20, 10), (20, 20), (10, 10)],
            &[(30, 30), (32, 30), (32, 32), (30, 30)],
        ]);

        limit_holes(&mut polygon, 1);

        // output: polygon 1 has 1 exterior, 1 interior
        assert_eq!(polygon.len(), 2);

        // ensure we've kept the right rings (ones with largest areas)
        assert_eq!(polygon[0][0].x, 0);
        assert_eq!(polygon[1][0].x, 10);
    }

    // gtest: GeometryTileData.limitHoles3
    #[test]
    fn limit_holes3() {
        // real world polygon with interior rings with negative areas
        // that need to be sorted in `limitHoles` by comparing absolute
        // area not signed
        let mut polygon = collection(&[
            &[
                (7336, -248),
                (7304, -248),
                (7272, -168),
                (7176, -200),
                (7080, -136),
                (7048, -56),
                (7128, -8),
                (7176, -56),
                (7288, -56),
                (7316, 0),
                (6918, 0),
                (6904, -40),
                (6984, -72),
                (6952, -88),
                (6952, -168),
                (6888, -88),
                (6856, -88),
                (6856, -8),
                (6872, 0),
                (6170, 0),
                (6184, -40),
                (6136, -72),
                (6104, -56),
                (6132, 0),
                (6028, 0),
                (6104, -152),
                (6184, -200),
                (6206, -256),
                (6272, -256),
                (6264, -248),
                (6248, -120),
                (6280, -136),
                (6280, -232),
                (6288, -256),
                (6790, -256),
                (6792, -248),
                (6800, -256),
                (7058, -256),
                (7064, -248),
                (7096, -256),
                (7338, -256),
                (7336, -248),
            ],
            &[(6344, -104), (6264, -8), (6392, -72), (6360, -200), (6344, -104)],
            &[(6744, -24), (6760, -72), (6728, -104), (6744, -24)],
            &[(6616, -104), (6648, -88), (6632, -72), (6664, -56), (6664, -120), (6616, -104)],
        ]);

        // make a copy for later testing
        let original = polygon.clone();

        assert_eq!(polygon.len(), 4);
        assert_eq!(test_signed_area(&polygon[0]), 515360.0); // exterior
        assert_eq!(test_signed_area(&polygon[1]), -12288.0); // biggest interior ring
        assert_eq!(test_signed_area(&polygon[2]), -2048.0); // smallest interior ring
        assert_eq!(test_signed_area(&polygon[3]), -3072.0); // second largest interior ring

        limit_holes(&mut polygon, 2);

        // output: polygon 1 has 1 exterior, 2 interior
        assert_eq!(polygon.len(), 3);

        // ensure we've kept the exterior ring
        assert_eq!(original[0], polygon[0]);

        // ensure we've kept the two largest interior rings
        assert_eq!(original[1], polygon[1]);
        assert_eq!(original[3], polygon[2]);
    }
}
