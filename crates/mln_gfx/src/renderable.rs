// Ports include/mln/gfx/renderable.hpp

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mln_util::size::Size;

/// `gfx::RenderableResource`: the backend-specific render target behind a
/// `Renderable`. Backends downcast through `as_any`.
pub trait RenderableResource: Any {
    fn bind(&self);
    fn as_any(&self) -> &dyn Any;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapBehaviour {
    NoFlush,
    Flush,
}

/// The data members of `gfx::Renderable` (`size`, `resource`). Interior
/// mutability: renderables are reached through `&dyn RendererBackend` and
/// create their resource lazily (`getDefaultRenderable`).
pub struct RenderableBase {
    size: Cell<Size>,
    resource: RefCell<Option<Rc<dyn RenderableResource>>>,
}

impl RenderableBase {
    pub fn new(size: Size, resource: Option<Rc<dyn RenderableResource>>) -> Self {
        RenderableBase { size: Cell::new(size), resource: RefCell::new(resource) }
    }
}

/// `gfx::Renderable`
pub trait Renderable {
    fn renderable_base(&self) -> &RenderableBase;

    fn get_size(&self) -> Size {
        self.renderable_base().size.get()
    }

    /// `getResource<T>()` without the cast: the caller downcasts.
    fn get_resource(&self) -> Option<Rc<dyn RenderableResource>> {
        self.renderable_base().resource.borrow().clone()
    }

    fn has_resource(&self) -> bool {
        self.renderable_base().resource.borrow().is_some()
    }

    fn set_resource(&self, resource: Option<Rc<dyn RenderableResource>>) {
        *self.renderable_base().resource.borrow_mut() = resource;
    }

    fn wait(&self) {}

    /// `setRenderableSize` (protected in C++).
    fn set_renderable_size(&self, size: Size) {
        self.renderable_base().size.set(size);
    }

    /// `operator!=`: compares the resource pointers.
    fn is_same_renderable(&self, other: &dyn Renderable) -> bool {
        let a = self.renderable_base().resource.borrow();
        let b = other.renderable_base().resource.borrow();
        match (a.as_ref(), b.as_ref()) {
            (Some(a), Some(b)) => std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b)),
            (None, None) => true,
            _ => false,
        }
    }
}
