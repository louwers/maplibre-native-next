// Ports test/renderer/renderable.test.cpp.

use mln_gfx::renderable::{Renderable, RenderableBase};
use mln_util::size::Size;

struct TestRenderable {
    base: RenderableBase,
}

impl TestRenderable {
    fn new(size: Size) -> Self {
        TestRenderable { base: RenderableBase::new(size, None) }
    }

    fn resize(&self, size: Size) {
        self.set_renderable_size(size);
    }
}

impl Renderable for TestRenderable {
    fn renderable_base(&self) -> &RenderableBase {
        &self.base
    }
}

// gtest: Renderable.MaintainsCanonicalPhysicalSize
#[test]
fn maintains_canonical_physical_size() {
    let renderable = TestRenderable::new(Size::new(256, 128));
    assert_eq!(renderable.get_size(), Size::new(256, 128));

    renderable.resize(Size::new(512, 384));
    assert_eq!(renderable.get_size(), Size::new(512, 384));
}
