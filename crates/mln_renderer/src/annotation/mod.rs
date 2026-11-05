//! Ports `src/mln/annotation` and `include/mln/annotation/annotation.hpp`
//! (re-exported by mln_map as `mln_map::annotation`).
//!
//! Layering (PLAN.md section 2 rule, measured from the include edges): the
//! annotation module includes nothing above the renderer (only style, tile,
//! renderer, layermanager, util and math headers), while the renderer
//! includes it from four files: render_source.cpp (RenderAnnotationSource),
//! render_orchestrator.cpp and renderer.cpp (AnnotationManager: updateData,
//! PointLayerID, ShapeLayerID) and renderer.hpp (AnnotationIDs). Placing the
//! module above mln_renderer would still move four of its nine files down
//! (annotation.hpp, annotation_source, annotation_tile,
//! render_annotation_source: the last two subclass GeometryTile and
//! RenderTileSource) and need a trait for the AnnotationManager that
//! TileParameters, AnnotationTile and the orchestrator reach. Putting the
//! whole module here needs no cut and moves no other file, as for the high
//! tile files (`crate::tile`).
//!
//! Structural differences from C++ (not observable):
//! - `AnnotationManager` holds no `Style&`: the methods that change the
//!   style take the map's `style::Impl` (C++ `style.get().impl`), because
//!   the map keeps its style in a `RefCell`. `setStyle` is not needed (the
//!   port has no `Map::setStyle`).
//! - The manager is shared with the renderer as `Arc`/`Weak` (C++ `WeakPtr`
//!   from a `WeakPtrFactory`) and keeps its `std::mutex`, so the renderer may
//!   run on another thread (UpdateParameters are `Send`). It cannot hold the
//!   renderer's tiles (`AnnotationTile*` in C++), which are not `Send`: each
//!   tile registers an `AnnotationTileSlot`, `updateData` puts the new data
//!   into the slots, and the tiles take it in `RenderAnnotationSource::update`
//!   of the same frame (C++ `tile->setData(...)` inside `updateData`).

#[allow(clippy::module_inception)]
pub mod annotation;
pub mod annotation_manager;
pub mod annotation_source;
pub mod annotation_tile;
pub mod fill_annotation_impl;
pub mod line_annotation_impl;
pub mod render_annotation_source;
pub mod shape_annotation_impl;
pub mod symbol_annotation_impl;

pub use annotation::{
    Annotation, AnnotationID, AnnotationIDs, FillAnnotation, LineAnnotation, ShapeAnnotationGeometry,
    SymbolAnnotation,
};
pub use annotation_manager::AnnotationManager;
