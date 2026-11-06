// Ports src/mln/renderer/sources/render_image_source.hpp and
// src/mln/renderer/sources/render_image_source.cpp (conformance).
//
// C++ moves the `ImageSourceRenderData` into the render tree in
// `createRenderItem` and the raster layers keep a raw pointer to it from
// `prepare`; here it is an `Rc` both hold.
//
// TODO(port): queryRenderedFeatures / querySourceFeatures (both empty in
// C++).

use crate::bucket::Bucket;
use crate::buckets::raster_bucket::RasterBucket;
use crate::render_pass::RenderPass;
use crate::render_source::{RenderSource, RenderSourceBase, SourcePrepareParameters};
use crate::render_tree::RenderItem;
use crate::tile_parameters::TileParameters;
use crate::util::tile_cover::{TileCoverParameters, tile_cover, tile_cover_bounds};
use geometry::Point;
use mln_gfx::UploadPass;
use mln_gfx::shaders::segment::SegmentBase;
use mln_map_types::TileCoordinate;
use mln_map_types::tile_coordinate::TileCoordinatePoint;
use mln_style::layer_properties::LayerProperties;
use mln_style::source::{self, static_immutable_cast};
use mln_style::sources::image_source;
use mln_tile_id::{GeometryCoordinate, UnwrappedTileID};
use mln_util::constants;
use mln_util::geo::LatLngBounds;
use mln_util::immutable::Immutable;
use mln_util::logging::{Event, Log};
use mln_util::mat4::{Mat4, matrix};
use mln_util::range::Range;
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;

/// `ImageSourceRenderData`.
pub struct ImageSourceRenderData {
    pub bucket: Option<Arc<RasterBucket>>,
    pub matrices: Vec<Mat4>,
    name: String,
}

impl ImageSourceRenderData {
    pub fn new(bucket: Option<Arc<RasterBucket>>, matrices: Vec<Mat4>, name: String) -> Self {
        ImageSourceRenderData { bucket, matrices, name }
    }
}

impl RenderItem for ImageSourceRenderData {
    fn upload(&self, upload_pass: &dyn UploadPass) {
        if let Some(bucket) = &self.bucket
            && bucket.needs_upload()
        {
            bucket.upload(upload_pass);
        }
    }

    fn has_render_pass(&self, _pass: RenderPass) -> bool {
        false
    }

    fn get_name(&self) -> &str {
        &self.name
    }
}

/// The render tree's handle on the source's render data.
struct ImageSourceRenderItem(Rc<ImageSourceRenderData>);

impl RenderItem for ImageSourceRenderItem {
    fn upload(&self, upload_pass: &dyn UploadPass) {
        self.0.upload(upload_pass);
    }

    fn has_render_pass(&self, pass: RenderPass) -> bool {
        self.0.has_render_pass(pass)
    }

    fn get_name(&self) -> &str {
        self.0.get_name()
    }
}

/// `RenderImageSource`.
pub struct RenderImageSource {
    base: RenderSourceBase,
    bucket: Option<Arc<RasterBucket>>,
    render_data: Option<Rc<ImageSourceRenderData>>,
    tile_ids: Vec<UnwrappedTileID>,
}

impl RenderImageSource {
    pub fn new(impl_: Arc<image_source::Impl>) -> Self {
        let impl_: Immutable<dyn source::Impl> = impl_;
        RenderImageSource { base: RenderSourceBase::new(impl_), bucket: None, render_data: None, tile_ids: Vec::new() }
    }

    fn impl_(&self) -> Arc<image_source::Impl> {
        static_immutable_cast::<image_source::Impl>(&self.base.base_impl)
    }
}

impl RenderSource for RenderImageSource {
    fn base(&self) -> &RenderSourceBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut RenderSourceBase {
        &mut self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn is_loaded(&self) -> bool {
        self.bucket.is_some()
    }

    fn update(
        &mut self,
        base_impl: Immutable<dyn source::Impl>,
        _layers: &[Immutable<dyn LayerProperties>],
        needs_rendering: bool,
        _needs_relayout: bool,
        parameters: &TileParameters<'_>,
    ) {
        self.base.enabled = needs_rendering;
        if !needs_rendering {
            return;
        }

        let transform_state = parameters.transform_state;
        self.base.base_impl = base_impl;

        let impl_ = self.impl_();
        let coords = impl_.get_coordinates();
        let image = impl_.get_image();

        let Some(image) = image.filter(|image| image.valid()) else {
            self.base.enabled = false;
            return;
        };

        // Compute the z0 tile coordinates for the given LatLngs
        let mut ne_point: TileCoordinatePoint = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut sw_point: TileCoordinatePoint = Point::new(f64::INFINITY, f64::INFINITY);
        let mut tile_coordinates: Vec<TileCoordinatePoint> = Vec::new();
        for lat_lng in &coords {
            let point = TileCoordinate::from_lat_lng(0.0, lat_lng).p;
            tile_coordinates.push(point);
            // std::min / std::max
            sw_point.x = if point.x < sw_point.x { point.x } else { sw_point.x };
            ne_point.x = if ne_point.x < point.x { point.x } else { ne_point.x };
            sw_point.y = if point.y < sw_point.y { point.y } else { sw_point.y };
            ne_point.y = if ne_point.y < point.y { point.y } else { ne_point.y };
        }

        // Calculate the optimum zoom level to determine the tile ids to use
        // for transforms
        let dx = ne_point.x - sw_point.x;
        let dy = ne_point.y - sw_point.y;
        let d_max = if dx < dy { dy } else { dx };
        let floor = (-mln_math::log2(d_max)).floor();
        let zoom = (if 0.0 < floor { floor } else { 0.0 }) as u8;

        // Only enable if the long side of the image is > 2 pixels. Resulting
        // in a display of at least 2 x 1 px image
        self.base.enabled = d_max * 2.0f64.powf(transform_state.get_zoom()) * constants::TILE_SIZE_D > 2.0;
        if !self.base.enabled {
            return;
        }

        let mut image_bounds = LatLngBounds::hull(coords[0], coords[1]);
        image_bounds.extend(&coords[2]);
        image_bounds.extend(&coords[3]);
        let tile_cover_ids = tile_cover_bounds(&image_bounds, zoom);
        self.tile_ids.clear();
        // The cover is empty, for example, when the image lies wholly outside
        // the latitudes a Web Mercator map can show (beyond about ±85.05°).
        if tile_cover_ids.is_empty() {
            self.base.enabled = false;
            return;
        }
        self.tile_ids.push(tile_cover_ids[0]);

        let mut has_visible_tile = false;
        // Add additional wrapped tile ids if necessary
        let zoom_range = Range::new(0u8, zoom);
        let ideal_tiles = tile_cover(
            &TileCoverParameters {
                transform_state: transform_state.clone(),
                tile_lod_min_radius: parameters.tile_lod_min_radius,
                tile_lod_scale: parameters.tile_lod_scale,
                tile_lod_pitch_threshold: parameters.tile_lod_pitch_threshold,
                tile_lod_mode: parameters.tile_lod_mode,
            },
            transform_state.get_zoom() as u8,
            zoom_range,
            None,
        );
        for tile in &ideal_tiles {
            for covering_tile in &tile_cover_ids {
                if covering_tile.canonical == tile.canonical
                    || covering_tile.canonical.is_child_of(&tile.canonical)
                    || tile.canonical.is_child_of(&covering_tile.canonical)
                {
                    has_visible_tile = true;
                    let wrapped_tile_id = UnwrappedTileID::with_wrap(tile.wrap, tile_cover_ids[0].canonical);
                    if tile.wrap != 0 && !self.tile_ids.contains(&wrapped_tile_id) {
                        self.tile_ids.push(wrapped_tile_id);
                    }
                    break;
                }
            }
        }

        self.base.enabled = has_visible_tile;
        if !self.base.enabled {
            return;
        }

        // Calculate Geometry Coordinates based on tile cover at ideal zoom
        let geom_coords: Vec<GeometryCoordinate> = tile_coordinates
            .iter()
            .map(|tile_coords| TileCoordinate::to_geometry_coordinate(&self.tile_ids[0], tile_coords))
            .collect();

        // C++ clears the bucket and swaps its image in place; a changed image
        // gets a new bucket here (buckets are shared across threads).
        let same_image =
            self.bucket.as_ref().is_some_and(|bucket| bucket.image.as_ref().is_some_and(|i| Arc::ptr_eq(i, &image)));
        if !same_image {
            self.bucket = Some(Arc::new(RasterBucket::with_shared_image(Some(image))));
        }
        let bucket = self.bucket.as_ref().expect("bucket");
        bucket.clear();

        // Set Bucket Vertices, Indices, and segments
        let e = constants::EXTENT as u16;
        let vertices = vec![
            RasterBucket::layout_vertex(Point::new(geom_coords[0].x, geom_coords[0].y), Point::new(0, 0)),
            RasterBucket::layout_vertex(Point::new(geom_coords[1].x, geom_coords[1].y), Point::new(e, 0)),
            RasterBucket::layout_vertex(Point::new(geom_coords[3].x, geom_coords[3].y), Point::new(0, e)),
            RasterBucket::layout_vertex(Point::new(geom_coords[2].x, geom_coords[2].y), Point::new(e, e)),
        ];
        let indices = vec![0, 1, 2, 1, 2, 3];
        let segments = vec![SegmentBase::with_lengths(0, 0, 4, 6)];
        bucket.set_geometry(vertices, indices, segments);
    }

    fn create_render_item(&mut self) -> Box<dyn RenderItem> {
        let render_data = self.render_data.take().expect("RenderImageSource::prepare");
        Box::new(ImageSourceRenderItem(render_data))
    }

    fn prepare(&mut self, parameters: &SourcePrepareParameters<'_>) {
        if !self.is_loaded() {
            self.render_data = Some(Rc::new(ImageSourceRenderData::new(
                self.bucket.clone(),
                Vec::new(),
                self.base.base_impl.id.clone(),
            )));
            return;
        }

        let transform_params = parameters.transform;
        let matrices: Vec<Mat4> = self
            .tile_ids
            .iter()
            .map(|tile_id| {
                let mut m: Mat4 = [0.0; 16];
                matrix::identity(&mut m);
                transform_params.state.matrix_for(&mut m, tile_id);
                m
            })
            .collect();
        self.render_data =
            Some(Rc::new(ImageSourceRenderData::new(self.bucket.clone(), matrices, self.base.base_impl.id.clone())));
    }

    fn get_image_render_data(&self) -> Option<Rc<ImageSourceRenderData>> {
        self.render_data.clone()
    }

    fn update_fading_tiles(&mut self) {}

    fn has_fading_tiles(&self) -> bool {
        false
    }

    fn reduce_memory_use(&mut self) {}

    fn dump_debug_logs(&self) {
        Log::info(Event::General, format!("RenderImageSource::id: {}", self.base.base_impl.id));
        Log::info(
            Event::General,
            format!("RenderImageSource::loaded: {}", if self.is_loaded() { "yes" } else { "no" }),
        );
    }
}
