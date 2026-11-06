// Ports src/mln/tile/geojson_tile_data.hpp.
//
// Implements a simple in-memory Tile type that holds GeoJSON values. A
// GeoJSON tile can only have one layer, and it is always returned
// regardless of which layer is requested.

use feature::{Feature, Identifier, PropertyMap, Value};
use mln_tile::{fixup_polygons, to_geometry_collection};
use mln_tile_id::geometry_tile_data::{GeometryCollection, GeometryTileData, GeometryTileFeature, GeometryTileLayer};
use mln_util::geometry::{FeatureType, to_feature_type};
use std::cell::OnceCell;
use std::sync::Arc;

/// `mapbox::feature::feature_collection<int16_t>`.
pub type TileFeatureCollection = Vec<Feature<i16>>;

/// `GeoJSONTileFeature`.
pub struct GeoJSONTileFeature<'a> {
    pub feature: &'a Feature<i16>,
    /// `mutable std::optional<GeometryCollection> geometry`.
    geometry: OnceCell<GeometryCollection>,
}

impl<'a> GeoJSONTileFeature<'a> {
    pub fn new(feature: &'a Feature<i16>) -> Self {
        GeoJSONTileFeature {
            feature,
            geometry: OnceCell::new(),
        }
    }
}

impl GeometryTileFeature for GeoJSONTileFeature<'_> {
    fn get_type(&self) -> FeatureType {
        to_feature_type(&self.feature.geometry)
    }

    fn get_properties(&self) -> &PropertyMap {
        &self.feature.properties
    }

    fn get_id(&self) -> Identifier {
        self.feature.id.clone()
    }

    fn get_geometries(&self) -> &GeometryCollection {
        self.geometry.get_or_init(|| {
            let geometry = to_geometry_collection(&self.feature.geometry);

            // https://github.com/mapbox/geojson-vt-cpp/issues/44
            if self.get_type() == FeatureType::Polygon {
                // C++ lets a wagyu exception propagate (the worker catches
                // it and reports a tile error); the port panics with its
                // message and the worker catches the panic.
                return fixup_polygons(&geometry).unwrap_or_else(|e| panic!("{e}"));
            }
            geometry
        })
    }

    fn get_value(&self, key: &str) -> Option<Value> {
        self.feature.properties.get(key).cloned()
    }
}

/// `GeoJSONTileLayer`.
pub struct GeoJSONTileLayer {
    features: Arc<TileFeatureCollection>,
}

impl GeoJSONTileLayer {
    pub fn new(features: Arc<TileFeatureCollection>) -> Self {
        GeoJSONTileLayer { features }
    }
}

impl GeometryTileLayer for GeoJSONTileLayer {
    fn feature_count(&self) -> usize {
        self.features.len()
    }

    fn get_feature(&self, i: usize) -> Box<dyn GeometryTileFeature + '_> {
        Box::new(GeoJSONTileFeature::new(&self.features[i]))
    }

    fn get_name(&self) -> String {
        String::new()
    }
}

/// `GeoJSONTileData`.
#[derive(Clone)]
pub struct GeoJSONTileData {
    features: Arc<TileFeatureCollection>,
}

impl GeoJSONTileData {
    pub fn new(features: TileFeatureCollection) -> Self {
        GeoJSONTileData {
            features: Arc::new(features),
        }
    }

    pub fn from_shared(features: Arc<TileFeatureCollection>) -> Self {
        GeoJSONTileData { features }
    }

    pub fn features(&self) -> &Arc<TileFeatureCollection> {
        &self.features
    }
}

impl GeometryTileData for GeoJSONTileData {
    fn clone_data(&self) -> Box<dyn GeometryTileData> {
        Box::new(GeoJSONTileData::from_shared(self.features.clone()))
    }

    fn get_layer(&self, _name: &str) -> Option<Box<dyn GeometryTileLayer + '_>> {
        Some(Box::new(GeoJSONTileLayer::new(self.features.clone())))
    }
}
