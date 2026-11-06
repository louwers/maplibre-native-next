// Ports src/mln/renderer/buckets/debug_bucket.hpp and
// src/mln/renderer/buckets/debug_bucket.cpp.
//
// TODO(port): the tile-id / timestamp text (`addText` over the simplex font
// of geometry/debug_font_data.hpp) for MapDebugOptions::ParseStatus and
// Timestamps, and the DebugShader drawables that draw it: no render test
// sets those options (the runner maps `debug` to TileBorders only). The
// bucket is still created as in C++, because the tile borders are drawn
// for tiles that have one.

use mln_gfx::UploadPass;
use mln_gfx::shaders::segment::SegmentBase;
use mln_map_types::MapDebugOptions;
use mln_tile_id::OverscaledTileID;
use mln_util::chrono::Timestamp;

use super::fill_bucket::FillLayoutVertex;

/// `DebugBucket`.
pub struct DebugBucket {
    pub renderable: bool,
    pub complete: bool,
    pub modified: Option<Timestamp>,
    pub expires: Option<Timestamp>,
    pub debug_mode: MapDebugOptions,
    pub source_name: String,

    pub vertices: Vec<FillLayoutVertex>,
    /// `gfx::IndexVector<gfx::Lines>`.
    pub indices: Vec<u16>,

    pub segments: Vec<SegmentBase>,
    pub tile_border_segments: Vec<SegmentBase>,
}

impl DebugBucket {
    pub fn new(
        _id: &OverscaledTileID,
        renderable: bool,
        complete: bool,
        modified: Option<Timestamp>,
        expires: Option<Timestamp>,
        debug_mode: MapDebugOptions,
        source_name: String,
    ) -> Self {
        let vertices: Vec<FillLayoutVertex> = Vec::new();
        let indices: Vec<u16> = Vec::new();
        // TODO(port): addText for ParseStatus ("<tile id> - complete|
        // renderable|pending <source>") and Timestamps ("modified: ...",
        // "expires: ..."), see the file comment.
        let segments = vec![SegmentBase::with_lengths(0, 0, vertices.len(), indices.len())];
        DebugBucket {
            renderable,
            complete,
            modified,
            expires,
            debug_mode,
            source_name,
            vertices,
            indices,
            segments,
            tile_border_segments: Vec::new(),
        }
    }

    pub fn upload(&self, _upload_pass: &dyn UploadPass) {}
}
