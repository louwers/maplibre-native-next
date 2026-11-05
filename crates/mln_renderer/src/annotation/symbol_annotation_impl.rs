// Ports src/mln/annotation/symbol_annotation_impl.hpp and
// src/mln/annotation/symbol_annotation_impl.cpp.
//
// The boost::geometry traits of the header (LatLng as a cartesian point,
// LatLngBounds as a box, the R-tree indexable of a SymbolAnnotationImpl)
// serve the AnnotationManager's symbol tree; see `lat_lng` and
// `AnnotationManager`'s `symbol_tree`.

use crate::annotation::annotation::{AnnotationID, SymbolAnnotation};
use crate::annotation::annotation_tile::AnnotationTileLayerData;
use mln_map_types::tile_coordinate::TileCoordinate;
use mln_tile_id::geometry_tile_data::GeometryCollection;
use mln_tile_id::{CanonicalTileID, UnwrappedTileID};
use mln_util::LatLng;
use mln_util::geometry::FeatureType;
use std::collections::HashMap;

/// `SymbolAnnotationImpl`.
#[derive(Debug)]
pub struct SymbolAnnotationImpl {
    pub id: AnnotationID,
    pub annotation: SymbolAnnotation,
    /// The R-tree indexable, `LatLng{p.y, p.x}`, computed (and validated:
    /// the C++ `LatLng` constructor throws) on insertion.
    lat_lng: LatLng,
}

impl SymbolAnnotationImpl {
    /// `SymbolAnnotationImpl(AnnotationID, SymbolAnnotation)`, plus the
    /// indexable that C++ computes when the tree inserts it: the `LatLng`
    /// constructor's error for an invalid latitude or longitude.
    pub fn new(id: AnnotationID, annotation: SymbolAnnotation) -> Result<Self, String> {
        let lat_lng = LatLng::new(annotation.geometry.y, annotation.geometry.x)?;
        Ok(SymbolAnnotationImpl { id, annotation, lat_lng })
    }

    /// `indexable<std::shared_ptr<const SymbolAnnotationImpl>>`.
    pub fn lat_lng(&self) -> &LatLng {
        &self.lat_lng
    }

    /// `updateLayer(const CanonicalTileID&, AnnotationTileLayer&)`.
    pub fn update_layer(&self, tile_id: &CanonicalTileID, layer: &mut AnnotationTileLayerData) {
        let mut feature_properties = HashMap::new();
        feature_properties.insert(
            "sprite".to_owned(),
            if self.annotation.icon.is_empty() {
                "default_marker".to_owned()
            } else {
                self.annotation.icon.clone()
            },
        );

        let lat_lng = self.lat_lng;
        let coordinate = TileCoordinate::from_lat_lng(0.0, &lat_lng);
        let tile_point =
            TileCoordinate::to_geometry_coordinate(&UnwrappedTileID::with_wrap(0, *tile_id), &coordinate.p);
        layer.add_feature(
            self.id,
            FeatureType::Point,
            GeometryCollection::from(vec![vec![tile_point]]),
            feature_properties,
        );
    }
}
