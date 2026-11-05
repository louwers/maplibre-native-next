// Ports src/mln/annotation/annotation_tile.hpp and
// src/mln/annotation/annotation_tile.cpp.
//
// C++ registers the tile itself with the AnnotationManager, which calls
// `tile->setData(...)` from `updateData`. The manager is shared with the
// renderer across threads and the tile is not `Send`, so the tile registers
// an `AnnotationTileSlot` instead: `updateData` puts the data there and
// `RenderAnnotationSource::update` hands it to the tile in the same frame
// (`take_pending_data`). `addTile` returns the first data, which the tile
// sets at once, as in C++.

use crate::annotation::annotation::AnnotationID;
use crate::annotation::annotation_manager::{AnnotationManager, SOURCE_ID};
use crate::geometry::feature_index::{QueryLayers, QueryResults};
use crate::tile::geometry_tile::GeometryTile;
use crate::tile::geometry_tile_worker::SharedTileData;
use crate::tile::tile::{Tile, TileBase};
use crate::tile::tile_observer::TileObserver;
use crate::tile_parameters::TileParameters;
use crate::tile_render_data::TileRenderData;
use maplibre_native_base::{Identifier, PropertyMap, Value};
use mln_style::expression::GlobalStateMap;
use mln_style::layer_properties::LayerProperties;
use mln_tile_id::geometry_tile_data::{
    GeometryCollection, GeometryCoordinates, GeometryTileData, GeometryTileFeature, GeometryTileLayer,
};
use mln_tile_id::{CanonicalTileID, OverscaledTileID};
use mln_util::geometry::FeatureType;
use mln_util::immutable::Immutable;
use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex, Weak};

/// The data `AnnotationManager::getTileData` builds (`nullptr` is `None`).
pub type AnnotationTileDataPtr = Option<Arc<AnnotationTileData>>;

/// What the AnnotationManager keeps of a registered tile (C++ keeps the
/// `AnnotationTile*` in `tiles`): its id and the data `updateData` set for
/// it, until the tile takes it.
#[derive(Debug)]
pub struct AnnotationTileSlot {
    /// `tile->id.canonical`.
    pub id: CanonicalTileID,
    data: Mutex<Option<AnnotationTileDataPtr>>,
}

impl AnnotationTileSlot {
    pub fn new(id: CanonicalTileID) -> Self {
        AnnotationTileSlot { id, data: Mutex::new(None) }
    }

    /// `tile->setData(data)`, delivered when the tile takes it.
    pub fn set_data(&self, data: AnnotationTileDataPtr) {
        *self.data.lock().unwrap() = Some(data);
    }

    fn take(&self) -> Option<AnnotationTileDataPtr> {
        self.data.lock().unwrap().take()
    }
}

fn shared(data: AnnotationTileDataPtr) -> Option<SharedTileData> {
    data.map(|data| data as SharedTileData)
}

/// `AnnotationTile`.
pub struct AnnotationTile {
    tile: GeometryTile,
    /// `mapbox::base::WeakPtr<AnnotationManager> annotationManager`.
    annotation_manager: Weak<AnnotationManager>,
    slot: Arc<AnnotationTileSlot>,
}

impl AnnotationTile {
    /// `AnnotationTile(const OverscaledTileID&, const TileParameters&,
    /// TileObserver*)`.
    pub fn new(
        overscaled_tile_id: OverscaledTileID,
        parameters: &TileParameters<'_>,
        observer: Option<Rc<dyn TileObserver>>,
    ) -> Self {
        let tile = AnnotationTile {
            tile: GeometryTile::new(overscaled_tile_id, SOURCE_ID.to_owned(), parameters, observer),
            annotation_manager: parameters.annotation_manager.clone(),
            slot: Arc::new(AnnotationTileSlot::new(overscaled_tile_id.canonical)),
        };
        if let Some(annotation_manager) = tile.annotation_manager.upgrade() {
            let data = annotation_manager.add_tile(&tile.slot);
            tile.tile.set_data(shared(data));
        }
        tile
    }

    /// Port-only: sets the data that `AnnotationManager::updateData` set for
    /// this tile since the last call (see the file comment).
    pub fn take_pending_data(&self) {
        if let Some(data) = self.slot.take() {
            self.tile.set_data(shared(data));
        }
    }

    pub fn geometry_tile(&self) -> &GeometryTile {
        &self.tile
    }
}

impl Drop for AnnotationTile {
    fn drop(&mut self) {
        if let Some(annotation_manager) = self.annotation_manager.upgrade() {
            annotation_manager.remove_tile(&self.slot);
        }
    }
}

impl Tile for AnnotationTile {
    fn base(&self) -> &TileBase {
        self.tile.base()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn create_render_data(&self) -> Box<dyn TileRenderData> {
        self.tile.create_render_data()
    }

    fn set_observer(&self, observer: Rc<dyn TileObserver>) {
        self.tile.set_observer(observer);
    }

    fn cancel(&self) {
        self.tile.cancel();
    }

    fn layer_properties_updated(&self, layer_properties: &Immutable<dyn LayerProperties>) -> bool {
        self.tile.layer_properties_updated(layer_properties)
    }

    fn set_show_collision_boxes(&self, show: bool) {
        self.tile.set_show_collision_boxes(show);
    }

    fn set_feature_state(&self, states: &crate::bucket::LayerFeatureStates) {
        self.tile.set_feature_state(states);
    }

    fn set_layers(&self, layers: &[Immutable<dyn LayerProperties>], global_state: Option<Arc<GlobalStateMap>>) {
        self.tile.set_layers(layers, global_state);
    }

    fn hold_for_fade(&self) -> bool {
        self.tile.hold_for_fade()
    }

    fn mark_rendered_ideal(&self) {
        self.tile.mark_rendered_ideal();
    }

    fn mark_rendered_previously(&self) {
        self.tile.mark_rendered_previously();
    }

    fn performed_fade_placement(&self) {
        self.tile.performed_fade_placement();
    }

    // GeometryTile's queries.
    fn query_rendered_features(
        &self,
        result: &mut QueryResults,
        query_geometry: &GeometryCoordinates,
        transform_state: &mln_map_types::TransformState,
        layers: &QueryLayers<'_>,
        options: &crate::query::RenderedQueryOptions,
        global_state: Option<&GlobalStateMap>,
        proj_matrix: &mln_util::mat4::Mat4,
        feature_state: &crate::source_state::SourceFeatureState,
    ) {
        self.tile.query_rendered_features(
            result,
            query_geometry,
            transform_state,
            layers,
            options,
            global_state,
            proj_matrix,
            feature_state,
        );
    }

    fn query_source_features(
        &self,
        result: &mut Vec<mln_util::feature::Feature>,
        options: &crate::query::SourceQueryOptions,
        global_state: Option<&GlobalStateMap>,
    ) {
        self.tile.query_source_features(result, options, global_state);
    }

    fn get_query_padding(&self, layers: &QueryLayers<'_>) -> f32 {
        self.tile.get_query_padding(layers)
    }
}

/// `AnnotationTileFeatureData`.
#[derive(Debug)]
pub struct AnnotationTileFeatureData {
    pub id: AnnotationID,
    pub type_: FeatureType,
    pub geometries: GeometryCollection,
    pub properties: HashMap<String, String>,
}

/// `AnnotationTileFeature`.
pub struct AnnotationTileFeature {
    data: Arc<AnnotationTileFeatureData>,
}

impl AnnotationTileFeature {
    pub fn new(data: Arc<AnnotationTileFeatureData>) -> Self {
        AnnotationTileFeature { data }
    }
}

impl GeometryTileFeature for AnnotationTileFeature {
    fn get_type(&self) -> FeatureType {
        self.data.type_
    }

    fn get_value(&self, key: &str) -> Option<Value> {
        self.data.properties.get(key).map(|value| Value::String(value.clone()))
    }

    fn get_id(&self) -> Identifier {
        Identifier::Uint(self.data.id)
    }

    fn get_geometries(&self) -> &GeometryCollection {
        &self.data.geometries
    }

    /// Port-only (see the trait): C++ leaves `getProperties` at the empty
    /// default, while `getValue` reads the feature's properties.
    fn get_values(&self) -> Option<PropertyMap> {
        Some(
            self.data
                .properties
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                .collect(),
        )
    }
}

/// `AnnotationTileLayerData`.
#[derive(Clone, Debug)]
pub struct AnnotationTileLayerData {
    pub name: String,
    pub features: Vec<Arc<AnnotationTileFeatureData>>,
}

impl AnnotationTileLayerData {
    pub fn new(name: String) -> Self {
        AnnotationTileLayerData { name, features: Vec::new() }
    }

    /// `AnnotationTileLayer::addFeature(id, type, geometries, properties)`
    /// (C++ adds through the layer handle that shares this data).
    pub fn add_feature(
        &mut self,
        id: AnnotationID,
        type_: FeatureType,
        geometries: GeometryCollection,
        properties: HashMap<String, String>,
    ) {
        self.features.push(Arc::new(AnnotationTileFeatureData { id, type_, geometries, properties }));
    }
}

/// `AnnotationTileLayer`.
pub struct AnnotationTileLayer {
    layer: Arc<AnnotationTileLayerData>,
}

impl AnnotationTileLayer {
    pub fn new(layer: Arc<AnnotationTileLayerData>) -> Self {
        AnnotationTileLayer { layer }
    }
}

impl GeometryTileLayer for AnnotationTileLayer {
    fn feature_count(&self) -> usize {
        self.layer.features.len()
    }

    fn get_feature(&self, i: usize) -> Box<dyn GeometryTileFeature + '_> {
        Box::new(AnnotationTileFeature::new(self.layer.features[i].clone()))
    }

    fn get_name(&self) -> String {
        self.layer.name.clone()
    }
}

/// `AnnotationTileData`.
#[derive(Clone, Debug, Default)]
pub struct AnnotationTileData {
    layers: HashMap<String, Arc<AnnotationTileLayerData>>,
}

impl AnnotationTileData {
    /// `addLayer(name)`: only constructs a new layer if it doesn't yet exist,
    /// otherwise, we'll use the existing one. Returns the layer for adding
    /// features (C++ returns an `AnnotationTileLayer` sharing the data).
    pub fn add_layer(&mut self, name: &str) -> &mut AnnotationTileLayerData {
        let layer = self
            .layers
            .entry(name.to_owned())
            .or_insert_with(|| Arc::new(AnnotationTileLayerData::new(name.to_owned())));
        Arc::make_mut(layer)
    }
}

impl GeometryTileData for AnnotationTileData {
    fn clone_data(&self) -> Box<dyn GeometryTileData> {
        Box::new(self.clone())
    }

    fn get_layer(&self, name: &str) -> Option<Box<dyn GeometryTileLayer + '_>> {
        self.layers
            .get(name)
            .map(|layer| Box::new(AnnotationTileLayer::new(layer.clone())) as Box<dyn GeometryTileLayer>)
    }
}
