// Ports src/mln/annotation/annotation_manager.hpp and
// src/mln/annotation/annotation_manager.cpp.
//
// See mod.rs for the style parameter and the tile slots. The
// `LayerManager::annotationsEnabled` checks are left out: the port's layer
// manager always has the line, fill and symbol layers.
//
// The symbol tree: C++ indexes the symbol annotations in a boost R*-tree
// (`rstar<16, 4>`) and adds the symbols of a tile to its point layer in the
// order the tree's query returns them. The port keeps them in a vector,
// appended on insertion and removed by moving the last element into the
// hole, and scans it with the same intersection test: that is exactly the
// tree's single leaf while it holds at most 16 annotations. With more, the
// tree splits and its query order differs from the vector's
// (DEVIATIONS.md [annotations] Symbol tree order).

use crate::annotation::annotation::{Annotation, AnnotationID, FillAnnotation, LineAnnotation, SymbolAnnotation};
use crate::annotation::annotation_source::AnnotationSource;
use crate::annotation::annotation_tile::{AnnotationTileData, AnnotationTileDataPtr, AnnotationTileSlot};
use crate::annotation::fill_annotation_impl::FillAnnotationImpl;
use crate::annotation::line_annotation_impl::LineAnnotationImpl;
use crate::annotation::shape_annotation_impl::ShapeAnnotationImpl;
use crate::annotation::symbol_annotation_impl::SymbolAnnotationImpl;
use mln_style::Layer;
use mln_style::PropertyValue;
use mln_style::expression::dsl::{concat, get_str, image, literal_str, to_string};
use mln_style::image::Image;
use mln_style::layers::SymbolLayer;
use mln_style::property_expression::PropertyExpression;
use mln_style::style_impl::Impl as StyleImpl;
use mln_tile_id::CanonicalTileID;
use mln_util::geo::{LatLng, LatLngBounds};
use mln_util::logging::{Event, Log};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

/// `AnnotationManager::SourceID`.
pub const SOURCE_ID: &str = "org.maplibre.annotations";
/// `AnnotationManager::PointLayerID`.
pub const POINT_LAYER_ID: &str = "org.maplibre.annotations.points";
/// `AnnotationManager::ShapeLayerID`.
pub const SHAPE_LAYER_ID: &str = "org.maplibre.annotations.shape.";

/// The members `std::mutex mutex` guards.
#[derive(Default)]
struct State {
    dirty: bool,

    next_id: AnnotationID,

    /// `SymbolAnnotationTree symbolTree` (see the file comment).
    symbol_tree: Vec<Arc<SymbolAnnotationImpl>>,
    /// Unlike std::unordered_map, std::map is guaranteed to sort by
    /// AnnotationID, ensuring that older annotations are below newer
    /// annotations. <https://github.com/mapbox/mapbox-gl-native/issues/5691>
    symbol_annotations: BTreeMap<AnnotationID, Arc<SymbolAnnotationImpl>>,
    shape_annotations: BTreeMap<AnnotationID, Box<dyn ShapeAnnotationImpl>>,
    /// `std::unordered_map<std::string, style::Image>`.
    images: HashMap<String, Image>,

    /// `std::unordered_set<AnnotationTile*> tiles`.
    tiles: Vec<Arc<AnnotationTileSlot>>,
}

/// `AnnotationManager`.
#[derive(Default)]
pub struct AnnotationManager {
    state: Mutex<State>,
}

/// To ensure that annotation images do not collide with images from the
/// style, we prefix input image IDs with "org.maplibre.annotations".
fn prefixed_image_id(id: &str) -> String {
    format!("{SOURCE_ID}.{id}")
}

impl AnnotationManager {
    /// `AnnotationManager(style::Style&)`; the style is passed to the
    /// methods that change it.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `onStyleLoaded()`.
    pub fn on_style_loaded(&self, style: &mut StyleImpl) {
        self.update_style(style);
    }

    /// `addAnnotation(const Annotation&)`. C++ throws the `LatLng`
    /// constructor's `std::domain_error` for a symbol annotation with an
    /// invalid latitude or longitude; that is the error here.
    pub fn add_annotation(&self, style: &mut StyleImpl, annotation: &Annotation) -> Result<AnnotationID, String> {
        let mut state = self.lock();
        let id = state.next_id;
        state.next_id += 1;
        match annotation {
            Annotation::Symbol(annotation) => state.add_symbol(id, annotation)?,
            Annotation::Line(annotation) => state.add_line(style, id, annotation),
            Annotation::Fill(annotation) => state.add_fill(style, id, annotation),
        }
        state.dirty = true;
        Ok(id)
    }

    /// `updateAnnotation(const AnnotationID&, const Annotation&)`: whether
    /// the annotations changed (the `dirty` flag). Errors as for
    /// `add_annotation`.
    pub fn update_annotation(
        &self,
        style: &mut StyleImpl,
        id: AnnotationID,
        annotation: &Annotation,
    ) -> Result<bool, String> {
        let mut state = self.lock();
        match annotation {
            Annotation::Symbol(annotation) => state.update_symbol(style, id, annotation)?,
            Annotation::Line(annotation) => state.update_line(style, id, annotation),
            Annotation::Fill(annotation) => state.update_fill(style, id, annotation),
        }
        Ok(state.dirty)
    }

    /// `removeAnnotation(const AnnotationID&)`.
    pub fn remove_annotation(&self, style: &mut StyleImpl, id: AnnotationID) {
        let mut state = self.lock();
        state.remove(style, id);
        state.dirty = true;
    }

    /// `addImage(std::unique_ptr<style::Image>)`.
    pub fn add_image(&self, style: &mut StyleImpl, image: Image) {
        let mut state = self.lock();
        let id = prefixed_image_id(&image.get_id());
        state.images.remove(&id);
        let inserted = Image::new(
            id.clone(),
            image.get_image().clone(),
            image.get_pixel_ratio(),
            image.is_sdf(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        )
        // The image was valid as `image`, and keeps its size and pixel ratio.
        .expect("annotation image");
        style.add_image(inserted.base_impl.clone());
        state.images.insert(id, inserted);
    }

    /// `removeImage(const std::string&)`.
    pub fn remove_image(&self, style: &mut StyleImpl, id: &str) {
        let mut state = self.lock();
        let id = prefixed_image_id(id);
        state.images.remove(&id);
        style.remove_image(&id);
    }

    /// `getTopOffsetPixelsForImage(const std::string&)`.
    pub fn get_top_offset_pixels_for_image(&self, id: &str) -> f64 {
        let state = self.lock();
        let id = prefixed_image_id(id);
        match state.images.get(&id) {
            // `-(uint32_t height / float pixelRatio) / 2`, in float.
            Some(image) => f64::from(-(image.get_image().size.height as f32 / image.get_pixel_ratio()) / 2.0),
            None => 0.0,
        }
    }

    /// `updateStyle()`.
    fn update_style(&self, style: &mut StyleImpl) {
        // Create annotation source, point layer, and point bucket. We do
        // everything via Style::Impl because we don't want annotation
        // mutations to trigger Style::Impl::styleMutated to be set.
        if style.get_source(SOURCE_ID).is_none() {
            if let Err(e) = style.add_source(Box::new(AnnotationSource::new())) {
                Log::error(Event::General, e.to_string());
            }

            let mut layer = SymbolLayer::new(POINT_LAYER_ID, SOURCE_ID);

            layer.set_source_layer(POINT_LAYER_ID);
            layer.set_icon_image(&PropertyValue::Expression(PropertyExpression::from_box(
                image(concat(vec![
                    literal_str(&format!("{SOURCE_ID}.")),
                    to_string(get_str("sprite"), None),
                ])),
                None,
            )));
            layer.set_icon_allow_overlap(&PropertyValue::Constant(true));
            layer.set_icon_ignore_placement(&PropertyValue::Constant(true));

            // C++ throws if the style already has a layer with this id.
            if let Err(e) = style.add_layer(Box::new(layer), None) {
                Log::error(Event::General, e.to_string());
            }
        }

        let state = self.lock();

        for shape in state.shape_annotations.values() {
            shape.update_style(style);
        }

        for image in state.images.values() {
            // Call addImage even for images we may have previously added,
            // because we must support addAnnotationImage being used to update
            // an existing image. Creating a new image is relatively cheap, as
            // it copies only the Immutable reference. (We can't keep track of
            // which images need to be added because we don't know if the
            // style is the same instance as in the last updateStyle call. If
            // it's a new style, we need to add all images.)
            style.add_image(image.base_impl.clone());
        }
    }

    /// `updateData()`.
    pub fn update_data(&self) {
        let mut state = self.lock();
        if state.dirty {
            let tiles = state.tiles.clone();
            for tile in &tiles {
                let data = state.get_tile_data(&tile.id);
                tile.set_data(data);
            }
            state.dirty = false;
        }
    }

    /// `addTile(AnnotationTile&)`: registers the tile's slot and returns the
    /// data C++ sets on the tile (`tile.setData(getTileData(...))`).
    pub fn add_tile(&self, tile: &Arc<AnnotationTileSlot>) -> AnnotationTileDataPtr {
        let mut state = self.lock();
        state.tiles.push(tile.clone());
        state.get_tile_data(&tile.id)
    }

    /// `removeTile(AnnotationTile&)`.
    pub fn remove_tile(&self, tile: &Arc<AnnotationTileSlot>) {
        let mut state = self.lock();
        state.tiles.retain(|registered| !Arc::ptr_eq(registered, tile));
    }
}

impl State {
    /// `add(const AnnotationID&, const SymbolAnnotation&)`.
    fn add_symbol(&mut self, id: AnnotationID, annotation: &SymbolAnnotation) -> Result<(), String> {
        let impl_ = Arc::new(SymbolAnnotationImpl::new(id, annotation.clone())?);
        self.symbol_tree.push(impl_.clone());
        self.symbol_annotations.insert(id, impl_);
        Ok(())
    }

    /// `add(const AnnotationID&, const LineAnnotation&)`.
    fn add_line(&mut self, style: &mut StyleImpl, id: AnnotationID, annotation: &LineAnnotation) {
        let impl_ = self
            .shape_annotations
            .entry(id)
            .or_insert_with(|| Box::new(LineAnnotationImpl::new(id, annotation)));
        impl_.update_style(style);
    }

    /// `add(const AnnotationID&, const FillAnnotation&)`.
    fn add_fill(&mut self, style: &mut StyleImpl, id: AnnotationID, annotation: &FillAnnotation) {
        let impl_ = self
            .shape_annotations
            .entry(id)
            .or_insert_with(|| Box::new(FillAnnotationImpl::new(id, annotation)));
        impl_.update_style(style);
    }

    /// `update(const AnnotationID&, const SymbolAnnotation&)`.
    fn update_symbol(
        &mut self,
        style: &mut StyleImpl,
        id: AnnotationID,
        annotation: &SymbolAnnotation,
    ) -> Result<(), String> {
        let Some(existing) = self.symbol_annotations.get(&id) else {
            debug_assert!(false, "Attempt to update a non-existent symbol annotation");
            return Ok(());
        };

        let existing = &existing.annotation;
        if existing.geometry != annotation.geometry || existing.icon != annotation.icon {
            self.dirty = true;

            self.remove(style, id);
            self.add_symbol(id, annotation)?;
        }
        Ok(())
    }

    /// `update(const AnnotationID&, const LineAnnotation&)`.
    fn update_line(&mut self, style: &mut StyleImpl, id: AnnotationID, annotation: &LineAnnotation) {
        if self.shape_annotations.remove(&id).is_none() {
            debug_assert!(false, "Attempt to update a non-existent shape annotation");
            return;
        }

        self.add_line(style, id, annotation);
        self.dirty = true;
    }

    /// `update(const AnnotationID&, const FillAnnotation&)`.
    fn update_fill(&mut self, style: &mut StyleImpl, id: AnnotationID, annotation: &FillAnnotation) {
        if self.shape_annotations.remove(&id).is_none() {
            debug_assert!(false, "Attempt to update a non-existent shape annotation");
            return;
        }

        self.add_fill(style, id, annotation);
        self.dirty = true;
    }

    /// `remove(const AnnotationID&)`.
    fn remove(&mut self, style: &mut StyleImpl, id: AnnotationID) {
        if let Some(impl_) = self.symbol_annotations.remove(&id) {
            // boost's R-tree removes from a leaf by moving its last element
            // into the hole.
            if let Some(index) = self.symbol_tree.iter().position(|value| Arc::ptr_eq(value, &impl_)) {
                self.symbol_tree.swap_remove(index);
            }
        } else if let Some(shape) = self.shape_annotations.remove(&id) {
            style.remove_layer(&shape.base().layer_id);
        } else {
            debug_assert!(false, "Should never happen");
        }
    }

    /// `getTileData(const CanonicalTileID&)`.
    fn get_tile_data(&mut self, tile_id: &CanonicalTileID) -> AnnotationTileDataPtr {
        if self.symbol_annotations.is_empty() && self.shape_annotations.is_empty() {
            return None;
        }

        let mut tile_data = AnnotationTileData::default();

        tile_data.add_layer(POINT_LAYER_ID);

        let mut tile_bounds = LatLngBounds::from_canonical_tile(tile_id.z, tile_id.x, tile_id.y);
        // Hack for https://github.com/mapbox/mapbox-gl-native/issues/12472
        // To handle precision issues, query a slightly larger area than the
        // tile bounds. Symbols at a border can be included in vector data for
        // both tiles. The rendering/querying logic will make sure the symbols
        // show up in only one of the tiles.
        let south_west = LatLng::new(tile_bounds.south() - 0.000000001, tile_bounds.west() - 0.000000001);
        let north_east = LatLng::new(tile_bounds.north() + 0.000000001, tile_bounds.east() + 0.000000001);
        // The tile's latitudes are within +-85.06, so these are valid.
        tile_bounds.extend(&south_west.expect("tile bounds"));
        tile_bounds.extend(&north_east.expect("tile bounds"));

        // `symbolTree.query(boost::geometry::index::intersects(tileBounds),
        // ...)`: a point intersects the box when it is inside or on its
        // boundary.
        for value in &self.symbol_tree {
            let lat_lng = value.lat_lng();
            let intersects = lat_lng.longitude() >= tile_bounds.west()
                && lat_lng.longitude() <= tile_bounds.east()
                && lat_lng.latitude() >= tile_bounds.south()
                && lat_lng.latitude() <= tile_bounds.north();
            if intersects {
                value.update_layer(tile_id, tile_data.add_layer(POINT_LAYER_ID));
            }
        }

        for shape in self.shape_annotations.values_mut() {
            shape.update_tile_data(tile_id, &mut tile_data);
        }

        Some(Arc::new(tile_data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The manager is shared with a renderer that may run on another thread
    /// (UpdateParameters are sent there).
    #[test]
    fn shared_with_the_render_thread() {
        fn assert_send_sync<T: Send + Sync>() {}
        fn assert_send<T: Send>() {}
        assert_send_sync::<AnnotationManager>();
        assert_send::<crate::UpdateParameters>();
    }

    #[test]
    fn prefixed_image_ids() {
        assert_eq!(prefixed_image_id("default_marker"), "org.maplibre.annotations.default_marker");
    }
}
