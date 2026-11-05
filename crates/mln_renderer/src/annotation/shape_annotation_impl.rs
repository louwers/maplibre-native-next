// Ports src/mln/annotation/shape_annotation_impl.hpp and
// src/mln/annotation/shape_annotation_impl.cpp.
//
// C++ `ShapeAnnotationImpl` is an abstract class with data members; the data
// is `ShapeAnnotationImplBase` and the virtual interface is the
// `ShapeAnnotationImpl` trait.

use crate::annotation::annotation::{AnnotationID, ShapeAnnotationGeometry};
use crate::annotation::annotation_manager::SHAPE_LAYER_ID;
use crate::annotation::annotation_tile::AnnotationTileData;
use geojson_vt::GeoJSONVT;
use maplibre_native_base::{Feature, Geometry};
use mln_style::style_impl::Impl as StyleImpl;
use mln_tile::{fixup_polygons, to_geometry_collection};
use mln_tile_id::CanonicalTileID;
use mln_util::constants;
use mln_util::geometry::{FeatureType, to_feature_type};
use mln_util::logging::{Event, Log};
use std::collections::HashMap;

/// The data members of `ShapeAnnotationImpl`.
pub struct ShapeAnnotationImplBase {
    pub id: AnnotationID,
    pub layer_id: String,
    /// `std::unique_ptr<mapbox::geojsonvt::GeoJSONVT> shapeTiler`.
    pub shape_tiler: Option<GeoJSONVT>,
}

impl ShapeAnnotationImplBase {
    /// `ShapeAnnotationImpl(AnnotationID)`.
    pub fn new(id: AnnotationID) -> Self {
        ShapeAnnotationImplBase {
            id,
            layer_id: format!("{SHAPE_LAYER_ID}{}", mln_util::string::to_string_u64(id)),
            shape_tiler: None,
        }
    }

    /// `updateTileData(const CanonicalTileID&, AnnotationTileData&)`, with
    /// the derived class's `geometry()`.
    pub fn update_tile_data(
        &mut self,
        geometry: &ShapeAnnotationGeometry,
        tile_id: &CanonicalTileID,
        data: &mut AnnotationTileData,
    ) {
        const BASE_TOLERANCE: f64 = 4.0;

        let shape_tiler = self.shape_tiler.get_or_insert_with(|| {
            let features = vec![Feature::new(to_geometry(geometry))];
            let options = geojson_vt::Options {
                // The annotation source is currently hard coded to maxzoom
                // 16, so we're topping out at z16 here as well.
                max_zoom: 16,
                tile: geojson_vt::TileOptions {
                    buffer: 255,
                    extent: constants::EXTENT as u16,
                    tolerance: BASE_TOLERANCE,
                    ..Default::default()
                },
                ..Default::default()
            };
            GeoJSONVT::new(&features, options)
        });

        // C++ `getTile` throws for a zoom above 16, which the annotation
        // source's zoom range rules out.
        let shape_tile = match shape_tiler.get_tile(tile_id.z, tile_id.x, tile_id.y) {
            Ok(shape_tile) => shape_tile,
            Err(e) => {
                Log::error(Event::General, e);
                return;
            }
        };
        if shape_tile.features.is_empty() {
            return;
        }

        let layer = data.add_layer(&self.layer_id);

        for shape_feature in &shape_tile.features {
            let feature_type = to_feature_type(&shape_feature.geometry);
            let mut render_geometry = to_geometry_collection(&shape_feature.geometry);

            debug_assert!(feature_type != FeatureType::Unknown);

            // https://github.com/mapbox/geojson-vt-cpp/issues/44
            if feature_type == FeatureType::Polygon {
                render_geometry = match fixup_polygons(&render_geometry) {
                    Ok(fixed) => fixed,
                    Err(e) => {
                        // C++ lets the wagyu exception propagate.
                        Log::error(Event::General, e.to_string());
                        continue;
                    }
                };
            }

            // C++ passes the default `properties = {{}}`: one empty key
            // mapped to an empty string.
            layer.add_feature(
                self.id,
                feature_type,
                render_geometry,
                HashMap::from([(String::new(), String::new())]),
            );
        }
    }
}

/// `ShapeAnnotationImpl`.
pub trait ShapeAnnotationImpl: Send {
    fn base(&self) -> &ShapeAnnotationImplBase;

    /// The base and `geometry()` together, for `updateTileData` (C++ reaches
    /// both through `this`).
    fn base_mut_and_geometry(&mut self) -> (&mut ShapeAnnotationImplBase, &ShapeAnnotationGeometry);

    /// `updateStyle(style::Style::Impl&)`.
    fn update_style(&self, style: &mut StyleImpl);

    /// `geometry()`.
    fn geometry(&self) -> &ShapeAnnotationGeometry;

    /// `updateTileData(const CanonicalTileID&, AnnotationTileData&)`.
    fn update_tile_data(&mut self, tile_id: &CanonicalTileID, data: &mut AnnotationTileData) {
        let (base, geometry) = self.base_mut_and_geometry();
        base.update_tile_data(geometry, tile_id, data);
    }
}

/// The `Feature{geom}` of the shape's geometry.
fn to_geometry(geometry: &ShapeAnnotationGeometry) -> Geometry<f64> {
    match geometry {
        ShapeAnnotationGeometry::LineString(geom) => Geometry::LineString(geom.clone()),
        ShapeAnnotationGeometry::Polygon(geom) => Geometry::Polygon(geom.clone()),
        ShapeAnnotationGeometry::MultiLineString(geom) => Geometry::MultiLineString(geom.clone()),
        ShapeAnnotationGeometry::MultiPolygon(geom) => Geometry::MultiPolygon(geom.clone()),
    }
}

/// `CloseShapeAnnotation`: closes the rings of polygons.
pub fn close_shape_annotation(geometry: &ShapeAnnotationGeometry) -> ShapeAnnotationGeometry {
    fn close(ring: &mut Vec<maplibre_native_base::Point<f64>>) {
        if !ring.is_empty() && ring.first() != ring.last() {
            ring.push(ring[0]);
        }
    }
    match geometry {
        ShapeAnnotationGeometry::LineString(_) | ShapeAnnotationGeometry::MultiLineString(_) => geometry.clone(),
        ShapeAnnotationGeometry::Polygon(geom) => {
            let mut closed = geom.clone();
            for ring in &mut closed {
                close(ring);
            }
            ShapeAnnotationGeometry::Polygon(closed)
        }
        ShapeAnnotationGeometry::MultiPolygon(geom) => {
            let mut closed = geom.clone();
            for polygon in &mut closed {
                for ring in polygon {
                    close(ring);
                }
            }
            ShapeAnnotationGeometry::MultiPolygon(closed)
        }
    }
}
