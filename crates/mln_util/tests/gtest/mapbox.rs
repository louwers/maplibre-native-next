// Ports test/util/mapbox.test.cpp.
//
// mln_util::mapbox ports the normalize* half of util/mapbox.cpp. The
// canonicalize* half (`canonicalizeTileURL`, `canonicalizeTileset`,
// `canonicalize{Source,Sprite,Glyph}URL`) is a TODO(port) there: it needs
// std::regex (ECMAScript) matching of the URL templates and
// `style::SourceType`, which lives above mln_util. The tests that call it
// are #[ignore]d; their canonicalize assertions are kept as comments, the
// normalize ones as code.

use mln_util::mapbox::{
    normalize_glyphs_url, normalize_source_url, normalize_sprite_url, normalize_style_url, normalize_tile_url,
};
use mln_util::tile_server_options::TileServerOptions;

fn mapbox_tile_server_options() -> TileServerOptions {
    TileServerOptions::mapbox_configuration()
}

fn map_libre_tile_server_options() -> TileServerOptions {
    TileServerOptions::maplibre_configuration()
}

fn map_tiler_tile_server_options() -> TileServerOptions {
    TileServerOptions::map_tiler_configuration()
}

// gtest: Mapbox.SourceURL
#[test]
fn source_url() {
    let mapbox = mapbox_tile_server_options();
    assert_eq!(
        "https://api.mapbox.com/v4/user.map.json?access_token=key&secure",
        normalize_source_url(&mapbox, "mapbox://user.map", "key").unwrap()
    );
    assert_eq!(
        "https://api.example.com/v4/user.map.json?access_token=key&secure",
        normalize_source_url(
            &mapbox_tile_server_options().with_base_url("https://api.example.com"),
            "mapbox://user.map",
            "key"
        )
        .unwrap()
    );
    assert_eq!(
        "https://api.mapbox.com/v4/user.map.json?access_token=key&secure&style=mapbox://styles/mapbox/streets-v9@0",
        normalize_source_url(&mapbox, "mapbox://user.map?style=mapbox://styles/mapbox/streets-v9@0", "key").unwrap()
    );
    assert_eq!(
        "https://api.mapbox.com/v4/user.map.json?access_token=key&secure",
        normalize_source_url(&mapbox, "mapbox://user.map?", "key").unwrap()
    );
    assert_eq!("http://path", normalize_source_url(&mapbox, "http://path", "key").unwrap());
    // EXPECT_THROW(..., std::runtime_error)
    assert!(normalize_source_url(&mapbox, "mapbox://user.map", "").is_err());
}

// gtest: Mapbox.GlyphsURL
#[test]
fn glyphs_url() {
    let mapbox = mapbox_tile_server_options();
    assert_eq!(
        "https://api.mapbox.com/fonts/v1/boxmap/Comic%20Sans/0-255.pbf?access_token=key",
        normalize_glyphs_url(&mapbox, "mapbox://fonts/boxmap/Comic%20Sans/0-255.pbf", "key")
    );
    assert_eq!(
        "https://api.example.com/fonts/v1/boxmap/Comic%20Sans/0-255.pbf?access_token=key",
        normalize_glyphs_url(
            &mapbox_tile_server_options().with_base_url("https://api.example.com"),
            "mapbox://fonts/boxmap/Comic%20Sans/0-255.pbf",
            "key"
        )
    );
    assert_eq!(
        "https://api.mapbox.com/fonts/v1/boxmap/{fontstack}/{range}.pbf?access_token=key",
        normalize_glyphs_url(&mapbox, "mapbox://fonts/boxmap/{fontstack}/{range}.pbf", "key")
    );
    assert_eq!("http://path", normalize_glyphs_url(&mapbox, "http://path", "key"));
    assert_eq!("mapbox://path", normalize_glyphs_url(&mapbox, "mapbox://path", "key"));
}

// gtest: Mapbox.StyleURL
#[test]
fn style_url() {
    let mapbox = mapbox_tile_server_options();
    assert_eq!("mapbox://foo", normalize_style_url(&mapbox, "mapbox://foo", "key"));
    assert_eq!(
        "https://api.mapbox.com/styles/v1/user/style?access_token=key",
        normalize_style_url(&mapbox, "mapbox://styles/user/style", "key")
    );
    assert_eq!(
        "https://api.example.com/styles/v1/user/style?access_token=key",
        normalize_style_url(
            &mapbox_tile_server_options().with_base_url("https://api.example.com"),
            "mapbox://styles/user/style",
            "key"
        )
    );
    assert_eq!(
        "https://api.mapbox.com/styles/v1/user/style/draft?access_token=key",
        normalize_style_url(&mapbox, "mapbox://styles/user/style/draft", "key")
    );
    assert_eq!(
        "https://api.mapbox.com/styles/v1/user/style?access_token=key&shave=true",
        normalize_style_url(&mapbox, "mapbox://styles/user/style?shave=true", "key")
    );
    assert_eq!(
        "https://api.mapbox.com/styles/v1/user/style?access_token=key",
        normalize_style_url(&mapbox, "mapbox://styles/user/style?", "key")
    );
    assert_eq!("http://path", normalize_style_url(&mapbox, "http://path", "key"));
}

// gtest: Mapbox.SpriteURL
#[test]
fn sprite_url() {
    let mapbox = mapbox_tile_server_options();
    assert_eq!("map/box/sprites@2x.json", normalize_sprite_url(&mapbox, "map/box/sprites@2x.json", "key"));
    assert_eq!("mapbox://foo", normalize_sprite_url(&mapbox, "mapbox://foo", "key"));
    assert_eq!(
        "https://api.mapbox.com/styles/v1/mapbox/streets-v8/sprite.json?access_token=key",
        normalize_sprite_url(&mapbox, "mapbox://sprites/mapbox/streets-v8.json", "key")
    );
    assert_eq!(
        "https://api.example.com/styles/v1/mapbox/streets-v8/sprite.json?access_token=key",
        normalize_sprite_url(
            &mapbox_tile_server_options().with_base_url("https://api.example.com"),
            "mapbox://sprites/mapbox/streets-v8.json",
            "key"
        )
    );
    assert_eq!(
        "https://api.mapbox.com/styles/v1/mapbox/streets-v8/sprite@2x.png?access_token=key",
        normalize_sprite_url(&mapbox, "mapbox://sprites/mapbox/streets-v8@2x.png", "key")
    );
    assert_eq!(
        "https://api.mapbox.com/styles/v1/mapbox/streets-v8/draft/sprite@2x.png?access_token=key",
        normalize_sprite_url(&mapbox, "mapbox://sprites/mapbox/streets-v8/draft@2x.png", "key")
    );
    assert_eq!(
        "https://api.mapbox.com/styles/v1/mapbox/streets-v11/sprite?access_token=key&fresh=true.png",
        normalize_sprite_url(&mapbox, "mapbox://sprites/mapbox/streets-v11?fresh=true.png", "key")
    );
    assert_eq!("mapbox://////", normalize_sprite_url(&mapbox, "mapbox://////", "key"));
}

// gtest: Mapbox.TileURL
#[test]
fn tile_url() {
    let mapbox = mapbox_tile_server_options();
    assert_eq!(
        "https://api.mapbox.com/v4/a.b/0/0/0.pbf?access_token=key",
        normalize_tile_url(&mapbox, "mapbox://tiles/a.b/0/0/0.pbf", "key")
    );
    assert_eq!(
        "https://api.mapbox.com/v4/a.b/0/0/0.pbf?access_token=key&style=mapbox://styles/mapbox/streets-v9@0",
        normalize_tile_url(&mapbox, "mapbox://tiles/a.b/0/0/0.pbf?style=mapbox://styles/mapbox/streets-v9@0", "key")
    );
    assert_eq!(
        "https://api.mapbox.com/v4/a.b/0/0/0.pbf?access_token=key",
        normalize_tile_url(&mapbox, "mapbox://tiles/a.b/0/0/0.pbf?", "key")
    );
    assert_eq!(
        "https://api.mapbox.com/v4/a.b/0/0/0.png?access_token=key",
        normalize_tile_url(&mapbox, "mapbox://tiles/a.b/0/0/0.png", "key")
    );
    assert_eq!(
        "https://api.example.com/v4/a.b/0/0/0.png?access_token=key",
        normalize_tile_url(
            &mapbox_tile_server_options().with_base_url("https://api.example.com"),
            "mapbox://tiles/a.b/0/0/0.png",
            "key"
        )
    );
    assert_eq!(
        "https://api.mapbox.com/v4/a.b/0/0/0@2x.png?access_token=key",
        normalize_tile_url(&mapbox, "mapbox://tiles/a.b/0/0/0@2x.png", "key")
    );
    assert_eq!(
        "https://api.mapbox.com/v4/a.b,c.d/0/0/0.pbf?access_token=key",
        normalize_tile_url(&mapbox, "mapbox://tiles/a.b,c.d/0/0/0.pbf", "key")
    );
    assert_eq!("http://path", normalize_sprite_url(&mapbox, "http://path", "key"));
}

// gtest: Mapbox.CanonicalURL
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeTileURL"]
fn canonical_url() {
    // test/util/mapbox.test.cpp:163: 22 canonicalizeTileURL cases (mapbox
    // configuration; vector .pbf with and without access_token and custom
    // parameters, raster .jpg/.jpg70/.png at tile sizes 256 ({ratio}) and
    // 512 (@2x), and the pass-through inputs "", "http://path",
    // "http://api.mapbox.com/v4/", ".../{y}." and ".../{y}/.").
}

// gtest: Mapbox.CanonicalizeRasterTileset
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeTileset"]
fn canonicalize_raster_tileset() {
    // Tileset { tiles: ["http://a.tiles.mapbox.com/v4/mapbox.satellite/{z}/{x}/{y}.png?access_token=key"] },
    // canonicalizeTileset(mapbox, tileset, "mapbox://mapbox.satellite", SourceType::Raster, 256)
    // -> tiles[0] == "mapbox://tiles/mapbox.satellite/{z}/{x}/{y}{ratio}.png"
}

// gtest: Mapbox.CanonicalizeVectorTileset
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeTileset"]
fn canonicalize_vector_tileset() {
    // Tileset { tiles: ["http://a.tiles.mapbox.com/v4/mapbox.streets/{z}/{x}/{y}.vector.pbf?access_token=key"] },
    // canonicalizeTileset(mapbox, tileset, "mapbox://mapbox.streets", SourceType::Vector, 512)
    // -> tiles[0] == "mapbox://tiles/mapbox.streets/{z}/{x}/{y}.vector.pbf"
}

// MapLibre tests

// gtest: MapLibre.CanonicalURL
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeTileURL"]
fn maplibre_canonical_url() {
    let maplibre = map_libre_tile_server_options();
    assert_eq!(
        "https://demotiles.maplibre.org/style.json",
        normalize_style_url(&maplibre, "maplibre://maps/style", "")
    );
    assert_eq!(
        "https://demotiles.maplibre.org/tiles/tiles.json",
        normalize_source_url(&maplibre, "maplibre://tiles/tiles", "").unwrap()
    );
    assert_eq!(
        "https://demotiles.maplibre.org/font/{fontstack}/{start}-{end}.pbf",
        normalize_glyphs_url(&maplibre, "maplibre://fonts/{fontstack}/{start}-{end}.pbf", "")
    );
    // EXPECT_EQ("maplibre://tiles/tiles/{z}/{x}/{y}.pbf",
    //           canonicalizeTileURL(maplibre, "https://demotiles.maplibre.org/tiles/{z}/{x}/{y}.pbf",
    //                               SourceType::Vector, 512));
}

// MapTiler tests

// gtest: MapTiler.StyleURL
#[test]
fn maptiler_style_url() {
    let maptiler = map_tiler_tile_server_options();
    // Styles
    assert_eq!(
        "https://api.maptiler.com/maps/basic/style.json",
        normalize_style_url(&maptiler, "maptiler://maps/basic", "")
    );
    assert_eq!(
        "https://api.maptiler.com/maps/basic/style.json?key=abcdef",
        normalize_style_url(&maptiler, "maptiler://maps/basic", "abcdef")
    );
    assert_eq!("", normalize_style_url(&maptiler, "", ""));
    assert_eq!("https://dummy", normalize_source_url(&maptiler, "https://dummy", "").unwrap());
}

// Sources

// gtest: MapTiler.SourceURL
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeSourceURL"]
fn maptiler_source_url() {
    let maptiler = map_tiler_tile_server_options();
    assert_eq!(
        "https://api.maptiler.com/tiles/v3/tiles.json?key=abcdef",
        normalize_source_url(&maptiler, "maptiler://sources/v3", "abcdef").unwrap()
    );
    assert_eq!(
        "https://api.maptiler.com/tiles/outdoor/tiles.json?key=abcdef",
        normalize_source_url(&maptiler, "maptiler://sources/outdoor", "abcdef").unwrap()
    );
    assert_eq!(
        "https://api.maptiler.com/tiles/7ac429c7-c96e-46dd-8c3e-13d48988986a/tiles.json?key=abcdef",
        normalize_source_url(&maptiler, "maptiler://sources/7ac429c7-c96e-46dd-8c3e-13d48988986a", "abcdef").unwrap()
    );
    // canonicalizeSourceURL(maptiler, "https://api.maptiler.com/tiles/v3/tiles.json?key=abcdef")
    //   == "maptiler://sources/v3"
    // canonicalizeSourceURL(maptiler, "https://api.maptiler.com/tiles/outdoor/tiles.json?key=abcdef")
    //   == "maptiler://sources/outdoor"
    // canonicalizeSourceURL(maptiler, "https://api.maptiler.com/tiles/7ac429c7-c96e-46dd-8c3e-13d48988986a/tiles.json?key=abcdef")
    //   == "maptiler://sources/7ac429c7-c96e-46dd-8c3e-13d48988986a"
}

// gtest: MapTiler.SourceURLPassThrough
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeSourceURL"]
fn maptiler_source_url_pass_through() {
    let maptiler = map_tiler_tile_server_options();
    assert_eq!("", normalize_source_url(&maptiler, "", "").unwrap());
    assert_eq!("http://dummy", normalize_source_url(&maptiler, "http://dummy", "").unwrap());
    assert_eq!(
        "https://api.tileserver.com/map?key=1234",
        normalize_source_url(&maptiler, "https://api.tileserver.com/map?key=1234", "").unwrap()
    );
    // canonicalizeSourceURL(maptiler, "http://dummy") == "http://dummy"
    // canonicalizeSourceURL(maptiler, "https://api.tileserver.com/map?key=1234") == same
    // canonicalizeSourceURL(maptiler, "") == ""
}

// gtest: MapTiler.GlyphsURL
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeGlyphURL"]
fn maptiler_glyphs_url() {
    let maptiler = map_tiler_tile_server_options();
    // Glyphs
    assert_eq!(
        "https://api.maptiler.com/fonts/{fontstack}/{range}.pbf",
        normalize_glyphs_url(&maptiler, "maptiler://fonts/{fontstack}/{range}.pbf", "")
    );
    assert_eq!(
        "https://api.maptiler.com/fonts/{fontstack}/{range}.pbf?key=abcdef",
        normalize_glyphs_url(&maptiler, "maptiler://fonts/{fontstack}/{range}.pbf", "abcdef")
    );
    // canonicalizeGlyphURL(maptiler, "https://api.maptiler.com/fonts/{fontstack}/{range}.pbf")
    //   == "maptiler://fonts/{fontstack}/{range}.pbf"
    // canonicalizeGlyphURL(maptiler, "https://api.maptiler.com/fonts/{fontstack}/{range}.pbf?key=abcdef")
    //   == "maptiler://fonts/{fontstack}/{range}.pbf"
}

// gtest: MapTiler.GlyphsURLPassThrough
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeGlyphURL"]
fn maptiler_glyphs_url_pass_through() {
    let maptiler = map_tiler_tile_server_options();
    assert_eq!("", normalize_glyphs_url(&maptiler, "", ""));
    assert_eq!("http://dummy", normalize_glyphs_url(&maptiler, "http://dummy", ""));
    assert_eq!(
        "https://api.tileserver.com/map?key=1234",
        normalize_glyphs_url(&maptiler, "https://api.tileserver.com/map?key=1234", "")
    );
    // canonicalizeGlyphURL(maptiler, "http://dummy") == "http://dummy"
    // canonicalizeGlyphURL(maptiler, "https://api.tileserver.com/map?key=1234") == same
    // canonicalizeGlyphURL(maptiler, "") == ""
}

// gtest: MapTiler.Sprites
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeSpriteURL"]
fn maptiler_sprites() {
    let maptiler = map_tiler_tile_server_options();
    // Sprites
    assert_eq!(
        "https://api.maptiler.com/maps/streets/sprite",
        normalize_sprite_url(&maptiler, "maptiler://sprites/streets/sprite", "")
    );
    // canonicalizeSpriteURL(maptiler, "https://api.maptiler.com/maps/streets/sprite")
    //   == "maptiler://sprites/streets/sprite"
}

// gtest: MapTiler.SpritesURLPassThrough
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeSpriteURL"]
fn maptiler_sprites_url_pass_through() {
    let maptiler = map_tiler_tile_server_options();
    assert_eq!("", normalize_sprite_url(&maptiler, "", ""));
    assert_eq!("http://dummy", normalize_sprite_url(&maptiler, "http://dummy", ""));
    assert_eq!(
        "https://api.tileserver.com/map?key=1234",
        normalize_sprite_url(&maptiler, "https://api.tileserver.com/map?key=1234", "")
    );
    // canonicalizeSpriteURL(maptiler, "http://dummy") == "http://dummy"
    // canonicalizeSpriteURL(maptiler, "https://api.tileserver.com/map?key=1234") == same
    // canonicalizeSpriteURL(maptiler, "") == ""
}

// gtest: MapTiler.Tiles
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeTileURL"]
fn maptiler_tiles() {
    let maptiler = map_tiler_tile_server_options();
    // Tiles
    assert_eq!(
        "https://api.maptiler.com/tiles/contours/{z}/{x}/{y}.pbf?key=abcdef",
        normalize_tile_url(&maptiler, "maptiler://tiles/tiles/contours/{z}/{x}/{y}.pbf", "abcdef")
    );
    assert_eq!(
        "https://api.maptiler.com/tiles/v3/{z}/{x}/{y}.pbf?key=abcdef",
        normalize_tile_url(&maptiler, "maptiler://tiles/tiles/v3/{z}/{x}/{y}.pbf", "abcdef")
    );
    // canonicalizeTileURL(maptiler, "https://api.maptiler.com/tiles/contours/{z}/{x}/{y}.pbf?key=abcdef",
    //                     SourceType::Vector, 512) == "maptiler://tiles/tiles/contours/{z}/{x}/{y}.pbf"
}

// gtest: MapTiler.TilesURLPassThrough
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeTileURL"]
fn maptiler_tiles_url_pass_through() {
    let maptiler = map_tiler_tile_server_options();
    assert_eq!("", normalize_tile_url(&maptiler, "", ""));
    assert_eq!("http://dummy", normalize_tile_url(&maptiler, "http://dummy", ""));
    assert_eq!(
        "https://api.tileserver.com/map?key=1234",
        normalize_tile_url(&maptiler, "https://api.tileserver.com/map?key=1234", "")
    );
    // canonicalizeTileURL(maptiler, "http://dummy", Vector, 512) == "http://dummy"
    // canonicalizeTileURL(maptiler, "https://api.tileserver.com/map?key=1234", Vector, 512) == same
    // canonicalizeTileURL(maptiler, "", Vector, 512) == ""
}

// gtest: MapTiler.CanonicalizeRasterTileset
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeTileset"]
fn maptiler_canonicalize_raster_tileset() {
    // Tileset { tiles: ["https://api.maptiler.com/tiles/satellite/{z}/{x}/{y}.jpg?key=abcdef"] },
    // canonicalizeTileset(maptiler, tileset, "maptiler://tiles/satellite", SourceType::Raster, 256)
    // -> tiles[0] == "maptiler://tiles/tiles/satellite/{z}/{x}/{y}{ratio}.jpg"
}

// gtest: MapTiler.CanonicalizeVectorTileset
#[test]
#[ignore = "TODO(port): util::mapbox canonicalizeTileset"]
fn maptiler_canonicalize_vector_tileset() {
    // Tileset { tiles: ["https://api.maptiler.com/tiles/v3/{z}/{x}/{y}.pbf?key=abcdef"] },
    // canonicalizeTileset(maptiler, tileset, "maptiler://tiles/streets", SourceType::Vector, 512)
    // -> tiles[0] == "maptiler://tiles/tiles/v3/{z}/{x}/{y}.pbf"
}
