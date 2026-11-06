// Ports test/renderer/dynamic_texture_atlas.test.cpp.
//
// The C++ fixture creates a HeadlessBackend and a BackendScope only to hand
// the atlas a `gfx::Context`. The port's DynamicTextureAtlas takes no
// context (its dynamic textures create their GPU texture in the upload
// pass, see dynamic_texture.rs), so the fixture has no backend.

use mln_renderer::gfx::dynamic_texture_atlas::{DynamicTextureAtlas, ImageAtlas, ImageVersionMap};
use mln_style_types::image::{ImageMap, Impl as ImageImpl};
use mln_util::image::PremultipliedImage;
use mln_util::immutable::Immutable;
use mln_util::size::Size;
use std::sync::Arc;

/// `makeImage(id, sdf, size = {16, 16})`.
fn make_image(id: &str, sdf: bool) -> Immutable<ImageImpl> {
    let size = Size::new(16, 16);
    Arc::new(
        ImageImpl::new(
            id.to_owned(),
            PremultipliedImage::new(size),
            1.0,
            sdf,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        )
        .expect("image"),
    )
}

/// `{{image->id, image}}`.
fn image_map(image: &Immutable<ImageImpl>) -> ImageMap {
    ImageMap::from([(image.id.clone(), image.clone())])
}

/// `DynamicTextureAtlasTest`.
struct DynamicTextureAtlasTest {
    atlas: DynamicTextureAtlas,
    atlases: Vec<ImageAtlas>,
}

impl DynamicTextureAtlasTest {
    fn new() -> Self {
        DynamicTextureAtlasTest { atlas: DynamicTextureAtlas::new(), atlases: Vec::new() }
    }

    fn upload_pattern(&mut self, image: &Immutable<ImageImpl>) -> &ImageAtlas {
        let image_atlas =
            self.atlas.upload_icons_and_patterns(&ImageMap::new(), &image_map(image), &ImageVersionMap::new());
        self.atlases.push(image_atlas);
        self.atlases.last().unwrap()
    }
}

impl Drop for DynamicTextureAtlasTest {
    /// `TearDown()`.
    fn drop(&mut self) {
        for image_atlas in &self.atlases {
            self.atlas.remove_textures(&image_atlas.texture_handles, &image_atlas.dynamic_texture);
        }
    }
}

// gtest: DynamicTextureAtlasTest.SharesAllocationWithinImageRevision
#[test]
fn shares_allocation_within_image_revision() {
    let mut test = DynamicTextureAtlasTest::new();
    let image = make_image("pattern", false);

    let first = test.upload_pattern(&image).texture_handles[0];
    let second = test.upload_pattern(&image).texture_handles[0];

    assert_eq!(first.get_id(), second.get_id());
    assert_eq!(first.get_rectangle(), second.get_rectangle());
}

// gtest: DynamicTextureAtlasTest.ReplacedSameSizeImageGetsFreshAllocation
#[test]
fn replaced_same_size_image_gets_fresh_allocation() {
    let mut test = DynamicTextureAtlasTest::new();
    // Each revision keeps its layout alive, as a tile still rendering an older layout would.
    let original = test.upload_pattern(&make_image("pattern", false)).texture_handles[0];
    let sdf = test.upload_pattern(&make_image("pattern", true)).texture_handles[0];
    let toggled_back = test.upload_pattern(&make_image("pattern", false)).texture_handles[0];

    assert_ne!(original.get_rectangle(), sdf.get_rectangle());
    assert_ne!(original.get_rectangle(), toggled_back.get_rectangle());
    assert_ne!(sdf.get_rectangle(), toggled_back.get_rectangle());
}

// gtest: DynamicTextureAtlasTest.IconAndPatternUsageDoNotShareAllocation
#[test]
fn icon_and_pattern_usage_do_not_share_allocation() {
    let mut test = DynamicTextureAtlasTest::new();
    let image = make_image("image", false);

    let image_atlas =
        test.atlas.upload_icons_and_patterns(&image_map(&image), &image_map(&image), &ImageVersionMap::new());
    test.atlases.push(image_atlas);
    let image_atlas = test.atlases.last().unwrap();

    assert_eq!(2, image_atlas.texture_handles.len());
    assert_ne!(image_atlas.texture_handles[0].get_rectangle(), image_atlas.texture_handles[1].get_rectangle());
}

// gtest: DynamicTextureAtlasTest.ReleasedRevisionGetsFreshAllocation
#[test]
fn released_revision_gets_fresh_allocation() {
    let mut test = DynamicTextureAtlasTest::new();
    let image = make_image("pattern", false);
    let released = test.atlas.upload_icons_and_patterns(&ImageMap::new(), &image_map(&image), &ImageVersionMap::new());
    let released_id = released.texture_handles[0].get_id();
    test.atlas.remove_textures(&released.texture_handles, &released.dynamic_texture);

    // The revision was forgotten on its last release, so it is packed and uploaded again under a new id.
    assert_ne!(released_id, test.upload_pattern(&image).texture_handles[0].get_id());
}
