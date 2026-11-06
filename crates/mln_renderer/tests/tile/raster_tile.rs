// Ports test/tile/raster_tile.test.cpp.

use std::sync::Arc;

use mln_renderer::buckets::raster_bucket::RasterBucket;
use mln_renderer::tile::raster_tile::RasterTile;
use mln_renderer::tile::tile::Tile;
use mln_renderer::tile::tile_loader::LoaderTile;
use mln_tile_id::OverscaledTileID;
use mln_util::exception::RuntimeError;
use mln_util::image::PremultipliedImage;
use mln_util::size::Size;

use super::RasterTileTest;

fn tile(test: &RasterTileTest) -> RasterTile {
    RasterTile::new(
        OverscaledTileID::from_zxy(0, 0, 0),
        "testSource".to_owned(),
        &test.tile_parameters(),
        &test.tileset,
        None,
    )
}

// gtest: RasterTile.setError
#[test]
fn set_error() {
    let test = RasterTileTest::new();
    let tile = tile(&test);
    LoaderTile::set_error(&**tile.inner(), RuntimeError::ptr("test"));
    assert!(!tile.is_renderable());
    assert!(tile.is_loaded());
    assert!(tile.is_complete());
}

// gtest: RasterTile.onError
#[test]
fn on_error() {
    let test = RasterTileTest::new();
    let tile = tile(&test);
    tile.inner().on_error(RuntimeError::ptr("test"), 0);
    assert!(!tile.is_renderable());
    assert!(tile.is_loaded());
    assert!(tile.is_complete());
}

// gtest: RasterTile.onParsed
#[test]
fn on_parsed() {
    let test = RasterTileTest::new();
    let tile = tile(&test);
    tile.inner().on_parsed(Some(Arc::new(RasterBucket::new(PremultipliedImage::new(Size::new(0, 0))))), 0);
    assert!(tile.is_renderable());
    assert!(tile.is_loaded());
    assert!(tile.is_complete());

    // Make sure that once we've had a renderable tile and then receive
    // erroneous data, we retain the previously rendered data and keep the tile
    // renderable.
    LoaderTile::set_error(&**tile.inner(), RuntimeError::ptr("Connection offline"));
    assert!(tile.is_renderable());
    assert!(tile.is_loaded());
    assert!(tile.is_complete());

    // Then simulate a parsing failure and make sure that we keep it renderable
    // in this situation as well.
    tile.inner().on_error(RuntimeError::ptr("Parse error"), 0);
    assert!(tile.is_renderable());
    assert!(tile.is_loaded());
    assert!(tile.is_complete());
}

// gtest: RasterTile.onParsedEmpty
#[test]
fn on_parsed_empty() {
    let test = RasterTileTest::new();
    let tile = tile(&test);
    tile.inner().on_parsed(None, 0);
    assert!(!tile.is_renderable());
    assert!(tile.is_loaded());
    assert!(tile.is_complete());
}
