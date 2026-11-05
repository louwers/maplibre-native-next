// Ports include/mln/renderer/renderer.hpp and src/mln/renderer/renderer.cpp.
//
// C++ keeps a `gfx::RendererBackend&`; the frontend owns the backend, so the
// renderer shares it through an `Rc`.

use crate::renderer_impl::RendererImpl;
use crate::renderer_observer::RendererObserver;
use crate::update_parameters::UpdateParameters;
use mln_gfx::{BackendScope, RendererBackend, ScopeType};
use std::rc::{Rc, Weak};
use std::sync::Arc;

pub struct Renderer {
    /// Always `Some` until `drop`, which destroys the impl inside a
    /// BackendScope as C++ does.
    impl_: Option<Box<RendererImpl>>,
}

impl Renderer {
    /// `Renderer(gfx::RendererBackend&, float pixelRatio, const
    /// std::optional<std::string>& localFontFamily = std::nullopt)`.
    // TODO(port): localFontFamily (glyph rasterization, M5).
    pub fn new(backend: Rc<dyn RendererBackend>, pixel_ratio: f32) -> Self {
        Renderer {
            impl_: Some(Box::new(RendererImpl::new(backend, pixel_ratio))),
        }
    }

    fn impl_ref(&self) -> &RendererImpl {
        self.impl_.as_deref().expect("renderer impl")
    }

    fn impl_mut(&mut self) -> &mut RendererImpl {
        self.impl_.as_deref_mut().expect("renderer impl")
    }

    pub fn mark_context_lost(&mut self) {
        self.impl_mut().orchestrator.mark_context_lost();
    }

    pub fn set_observer(&mut self, observer: Option<Weak<dyn RendererObserver>>) {
        let impl_ = self.impl_mut();
        impl_.set_observer(observer.clone());
        impl_.orchestrator.set_observer(observer);
    }

    pub fn render(&mut self, update_parameters: &Arc<UpdateParameters>) {
        // TODO(port): ErrorScope (SymbolErrorObserver routing).

        let impl_ = self.impl_mut();
        let style_changed = impl_.style_loaded && !update_parameters.style_loaded;
        impl_.style_loaded = update_parameters.style_loaded;
        if style_changed {
            impl_.orchestrator.reset_dynamic_texture_atlas();
        }
        if let Some(mut render_tree) = impl_.orchestrator.create_render_tree(update_parameters) {
            render_tree.prepare();
            impl_.render(&render_tree, update_parameters);
        }
    }

    // query
    /// `queryRenderedFeatures(const ScreenLineString&, const RenderedQueryOptions&)`.
    pub fn query_rendered_features(
        &self,
        geometry: &[mln_util::geo::ScreenCoordinate],
        options: &crate::query::RenderedQueryOptions,
    ) -> Vec<mln_util::feature::Feature> {
        self.impl_ref().orchestrator.query_rendered_features(geometry, options)
    }

    /// `queryRenderedFeatures(const ScreenCoordinate&, const RenderedQueryOptions&)`.
    pub fn query_rendered_features_at_point(
        &self,
        point: &mln_util::geo::ScreenCoordinate,
        options: &crate::query::RenderedQueryOptions,
    ) -> Vec<mln_util::feature::Feature> {
        self.impl_ref().orchestrator.query_rendered_features(&[*point], options)
    }

    /// `queryRenderedFeatures(const ScreenBox&, const RenderedQueryOptions&)`.
    pub fn query_rendered_features_in_box(
        &self,
        min: &mln_util::geo::ScreenCoordinate,
        max: &mln_util::geo::ScreenCoordinate,
        options: &crate::query::RenderedQueryOptions,
    ) -> Vec<mln_util::feature::Feature> {
        use mln_util::geo::ScreenCoordinate;
        self.impl_ref().orchestrator.query_rendered_features(
            &[*min, ScreenCoordinate::new(max.x, min.y), *max, ScreenCoordinate::new(min.x, max.y), *min],
            options,
        )
    }

    /// `queryPointAnnotations(const ScreenBox&)` (the box as its corners,
    /// as for `query_rendered_features_in_box`).
    pub fn query_point_annotations(
        &self,
        min: &mln_util::geo::ScreenCoordinate,
        max: &mln_util::geo::ScreenCoordinate,
    ) -> crate::annotation::AnnotationIDs {
        let options = crate::query::RenderedQueryOptions::new(
            Some(vec![crate::annotation::annotation_manager::POINT_LAYER_ID.to_owned()]),
            None,
        );
        let features = self.query_rendered_features_in_box(min, max, &options);
        self.get_annotation_ids(&features)
    }

    /// `queryShapeAnnotations(const ScreenBox&)`.
    pub fn query_shape_annotations(
        &self,
        min: &mln_util::geo::ScreenCoordinate,
        max: &mln_util::geo::ScreenCoordinate,
    ) -> crate::annotation::AnnotationIDs {
        use mln_util::geo::ScreenCoordinate;
        let features = self.impl_ref().orchestrator.query_shape_annotations(&[
            *min,
            ScreenCoordinate::new(max.x, min.y),
            *max,
            ScreenCoordinate::new(min.x, max.y),
            *min,
        ]);
        self.get_annotation_ids(&features)
    }

    /// `getAnnotationIDs(const std::vector<Feature>&)`: the features' ids,
    /// sorted and without duplicates.
    pub fn get_annotation_ids(&self, features: &[mln_util::feature::Feature]) -> crate::annotation::AnnotationIDs {
        let mut set = std::collections::BTreeSet::new();
        for feature in features {
            let maplibre_native_base::Identifier::Uint(id) = feature.id else {
                debug_assert!(false, "annotation feature id is not a uint64_t");
                continue;
            };
            set.insert(id);
        }
        set.into_iter().collect()
    }

    /// `querySourceFeatures(sourceID, options)`.
    pub fn query_source_features(
        &self,
        source_id: &str,
        options: &crate::query::SourceQueryOptions,
    ) -> Vec<mln_util::feature::Feature> {
        self.impl_ref().orchestrator.query_source_features(source_id, options)
    }

    /// `getPlacedSymbolsData()`.
    pub fn get_placed_symbols_data(&self) -> Vec<crate::text::placement::PlacedSymbolData> {
        self.impl_ref().orchestrator.get_placed_symbols_data()
    }

    // TODO(port): queryPointAnnotations, queryShapeAnnotations,
    // queryFeatureExtensions, dumpDebugLogs.

    // layers: feature state
    /// `setFeatureState(sourceID, sourceLayerID, featureID, state)`.
    pub fn set_feature_state(
        &mut self,
        source_id: &str,
        source_layer_id: Option<&str>,
        feature_id: &str,
        state: &mln_style::expression::expression::FeatureState,
    ) {
        self.impl_mut().orchestrator.set_feature_state(source_id, source_layer_id, feature_id, state);
    }

    /// `getFeatureState(sourceID, sourceLayerID, featureID)`.
    pub fn get_feature_state(
        &self,
        source_id: &str,
        source_layer_id: Option<&str>,
        feature_id: &str,
    ) -> mln_style::expression::expression::FeatureState {
        self.impl_ref().orchestrator.get_feature_state(source_id, source_layer_id, feature_id)
    }

    /// `removeFeatureState(sourceID, sourceLayerID, featureID, stateKey)`.
    pub fn remove_feature_state(
        &mut self,
        source_id: &str,
        source_layer_id: Option<&str>,
        feature_id: Option<&str>,
        state_key: Option<&str>,
    ) {
        self.impl_mut()
            .orchestrator
            .remove_feature_state(source_id, source_layer_id, feature_id, state_key);
    }

    pub fn collect_placed_symbol_data(&mut self, enable: bool) {
        self.impl_mut().orchestrator.collect_placed_symbol_data(enable);
    }

    pub fn set_tile_cache_enabled(&mut self, enable: bool) {
        self.impl_mut().orchestrator.set_tile_cache_enabled(enable);
    }

    pub fn get_tile_cache_enabled(&self) -> bool {
        self.impl_ref().orchestrator.get_tile_cache_enabled()
    }

    pub fn reduce_memory_use(&mut self) {
        let backend = self.impl_ref().backend.clone();
        let _guard = BackendScope::new(&*backend, ScopeType::Explicit);
        self.impl_mut().reduce_memory_use();
        self.impl_mut().orchestrator.reduce_memory_use();
    }

    pub fn clear_data(&mut self) {
        self.impl_mut().orchestrator.clear_data();
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // `gfx::BackendScope guard{impl->backend}; impl.reset();`
        if let Some(impl_) = self.impl_.take() {
            let backend = impl_.backend.clone();
            let _guard = BackendScope::new(&*backend, ScopeType::Explicit);
            drop(impl_);
        }
    }
}
