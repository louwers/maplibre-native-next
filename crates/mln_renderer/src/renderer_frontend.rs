// Ports include/mln/renderer/renderer_frontend.hpp

use std::rc::Weak;
use std::sync::Arc;

use crate::renderer_observer::RendererObserver;
use crate::update_parameters::UpdateParameters;

/// The RenderFrontend is the bridge between the Map and platform used to
/// update and observe the Renderer.
///
/// It hides any threading specifics and always replies on the original
/// thread.
///
/// The methods take `&self`: the Map keeps a handle to its frontend (C++
/// keeps a reference) while the platform also uses it, so implementations
/// use interior mutability.
pub trait RendererFrontend {
    /// Must synchronously clean up the Renderer if set.
    fn reset(&self);

    /// Implementer must bind the renderer observer to the renderer in a
    /// appropriate manner so that the callbacks occur on the main thread.
    /// C++ passes a reference that must outlive the renderer; the port passes
    /// a non-owning `Weak`.
    fn set_observer(&self, observer: Weak<dyn RendererObserver>);

    /// Coalescing updates is up to the implementer.
    fn update(&self, parameters: Arc<UpdateParameters>);

    // TODO(port): `getThreadPool()` (the TaggedScheduler the style and
    // render sources use), with the thread pool.
}
