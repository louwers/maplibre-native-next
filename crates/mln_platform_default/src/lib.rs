//! Ports `platform/default` (the headless frontend; the headless backend and
//! file sources arrive with gfx and storage).

pub mod headless_frontend;

pub use headless_frontend::{HeadlessFrontend, RenderResult};
pub mod map_snapshotter;

pub use map_snapshotter::{MapSnapshotter, MapSnapshotterObserver};
