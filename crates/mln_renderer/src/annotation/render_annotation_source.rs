// Ports src/mln/annotation/render_annotation_source.hpp and
// src/mln/annotation/render_annotation_source.cpp.

use crate::annotation::annotation_source;
use crate::annotation::annotation_tile::AnnotationTile;
use crate::geometry::feature_index::{QueryLayers, QueryResults};
use crate::render_source::{RenderSource, RenderSourceBase, RenderTiles, SourcePrepareParameters};
use crate::render_tile::RenderTile;
use crate::render_tree::RenderItem;
use crate::source_state::SourceFeatureState;
use crate::sources::render_tile_source::RenderTileSource;
use crate::tile::tile::Tile;
use crate::tile_parameters::TileParameters;
use mln_actor::TaggedScheduler;
use mln_style::expression::GlobalStateMap;
use mln_style::layer_properties::LayerProperties;
use mln_style::source::{self, static_immutable_cast};
use mln_util::constants;
use mln_util::immutable::Immutable;
use mln_util::range::Range;
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;

/// `RenderAnnotationSource`.
pub struct RenderAnnotationSource {
    tile_source: RenderTileSource,
}

impl RenderAnnotationSource {
    /// `RenderAnnotationSource(Immutable<AnnotationSource::Impl>, const
    /// TaggedScheduler&)`. The RenderTileSource constructor already makes
    /// the source the observer of its tile pyramid.
    pub fn new(impl_: Arc<annotation_source::Impl>, thread_pool: &TaggedScheduler) -> Self {
        let impl_: Immutable<dyn source::Impl> = impl_;
        RenderAnnotationSource {
            tile_source: RenderTileSource::new(impl_, thread_pool),
        }
    }

    /// `impl()`.
    pub fn impl_(&self) -> Arc<annotation_source::Impl> {
        static_immutable_cast::<annotation_source::Impl>(&self.tile_source.base.base_impl)
    }

    pub fn tile_source(&self) -> &RenderTileSource {
        &self.tile_source
    }
}

impl RenderSource for RenderAnnotationSource {
    fn base(&self) -> &RenderSourceBase {
        &self.tile_source.base
    }

    fn base_mut(&mut self) -> &mut RenderSourceBase {
        &mut self.tile_source.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn is_loaded(&self) -> bool {
        self.tile_source.is_loaded()
    }

    fn update(
        &mut self,
        base_impl: Immutable<dyn source::Impl>,
        layers: &[Immutable<dyn LayerProperties>],
        needs_rendering: bool,
        needs_relayout: bool,
        parameters: &TileParameters<'_>,
    ) {
        self.tile_source.base.base_impl = base_impl;

        self.tile_source.base.enabled = needs_rendering;

        // Port: the data AnnotationManager::updateData set for the tiles at
        // the start of this frame (C++ sets it on the tiles there; see
        // annotation_tile.rs).
        for tile in self.tile_source.tile_pyramid.get_tiles().values() {
            if let Some(tile) = tile.as_any().downcast_ref::<AnnotationTile>() {
                tile.take_pending_data();
            }
        }

        let impl_ = self.tile_source.base.base_impl.clone();
        self.tile_source.tile_pyramid.update(
            layers,
            needs_rendering,
            needs_relayout,
            parameters,
            &*impl_,
            constants::TILE_SIZE_I,
            // Zoom level 16 is typically sufficient for annotations.
            // See https://github.com/mapbox/mapbox-gl-native/issues/10197
            Range::new(0, 16),
            None,
            |tile_id, observer| -> Option<Box<dyn Tile>> {
                Some(Box::new(AnnotationTile::new(*tile_id, parameters, Some(observer))))
            },
        );
    }

    fn create_render_item(&mut self) -> Box<dyn RenderItem> {
        self.tile_source.create_render_item()
    }

    fn prepare(&mut self, parameters: &SourcePrepareParameters<'_>) {
        self.tile_source.prepare(parameters);
    }

    fn update_fading_tiles(&mut self) {
        self.tile_source.update_fading_tiles();
    }

    fn has_fading_tiles(&self) -> bool {
        self.tile_source.has_fading_tiles()
    }

    fn get_render_tiles(&self) -> Option<RenderTiles> {
        self.tile_source.get_render_tiles()
    }

    fn get_render_tiles_sorted_by_y_position(&self) -> Option<RenderTiles> {
        self.tile_source.get_render_tiles_sorted_by_y_position()
    }

    fn get_raw_render_tiles(&self) -> Rc<Vec<RenderTile>> {
        self.tile_source.get_raw_render_tiles()
    }

    /// `queryRenderedFeatures(...)`: the tile pyramid's, with no feature
    /// state (C++ passes `{}`).
    fn query_rendered_features(
        &self,
        geometry: &[mln_util::geo::ScreenCoordinate],
        transform_state: &mln_map_types::TransformState,
        layers: &QueryLayers<'_>,
        options: &crate::query::RenderedQueryOptions,
        global_state: Option<&GlobalStateMap>,
        proj_matrix: &mln_util::mat4::Mat4,
    ) -> QueryResults {
        self.tile_source.tile_pyramid.query_rendered_features(
            geometry,
            transform_state,
            layers,
            options,
            global_state,
            proj_matrix,
            &SourceFeatureState::new(),
        )
    }

    /// `querySourceFeatures(...)`: none.
    fn query_source_features(
        &self,
        _options: &crate::query::SourceQueryOptions,
        _global_state: Option<&GlobalStateMap>,
    ) -> Vec<mln_util::feature::Feature> {
        Vec::new()
    }

    fn set_feature_state(
        &mut self,
        source_layer_id: Option<&str>,
        feature_id: &str,
        state: &mln_style::expression::expression::FeatureState,
    ) {
        self.tile_source.set_feature_state(source_layer_id, feature_id, state);
    }

    fn get_feature_state(
        &self,
        source_layer_id: Option<&str>,
        feature_id: &str,
    ) -> mln_style::expression::expression::FeatureState {
        self.tile_source.get_feature_state(source_layer_id, feature_id)
    }

    fn remove_feature_state(&mut self, source_layer_id: Option<&str>, feature_id: Option<&str>, state_key: Option<&str>) {
        self.tile_source.remove_feature_state(source_layer_id, feature_id, state_key);
    }

    fn set_cache_enabled(&mut self, enable: bool) {
        self.tile_source.set_cache_enabled(enable);
    }

    fn reduce_memory_use(&mut self) {
        self.tile_source.reduce_memory_use();
    }

    fn dump_debug_logs(&self) {
        self.tile_source.dump_debug_logs();
    }
}
