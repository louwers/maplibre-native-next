// Ports platform/default/include/mln/map/map_snapshotter.hpp and
// platform/default/src/mln/map/map_snapshotter.cpp.
//
// Differences from C++ (DEVIATIONS.md [server-tests] MapSnapshotter):
// - C++ renders on a "Snapshotter" thread (`util::Thread<SnapshotterRenderer>`)
//   and sends the renderer's observer calls back to the map's thread through
//   a `ForwardingRendererObserver`. The port renders on the map's thread:
//   the `UpdateParameters` hold the style's layer and source Impls, which are
//   not `Send`. `SnapshotterRenderer` observes the HeadlessFrontend's renderer
//   and calls the map's observer directly, as a map's own HeadlessFrontend
//   does. The snapshot callback still goes through an actor on the caller's
//   scheduler, so it runs from the RunLoop after the frame, as in C++.
// - `MapSnapshotter`'s methods take `&self` (state in `RefCell`s), so an
//   observer callback can reach the snapshotter through an `Rc`/`Weak`, as
//   the C++ lambdas capture it by reference. `getStyle()` is
//   `with_style(|style| ...)`.
// - The MapObserver half of `MapSnapshotter::Impl` is `SnapshotterMapObserver`
//   (the Map owns its observer as an `Rc`; the Impl owns the Map).
//
// TODO(port): addAnnotationImage/addAnnotation (annotations are not ported),
// localFontFamily, getThreadPool.

use crate::headless_frontend::HeadlessFrontend;
use mln_actor::{ActorRef, Mailbox, scheduler};
use mln_gfx::{ContextMode, SwapBehaviour};
use mln_map::transform::Transform;
use mln_map::{CameraOptions, Map, MapLoadError, MapMode, MapObserver, MapOptions, TransformState};
use mln_renderer::{ExceptionPtr, RenderMode, RendererFrontend, RendererObserver, UpdateParameters};
use mln_storage::{ClientOptions, ResourceOptions};
use mln_style::Style;
use mln_util::exception::MisuseException;
use mln_util::geo::{EdgeInsets, LatLng, LatLngBounds, ScreenCoordinate, WrapMode};
use mln_util::image::PremultipliedImage;
use mln_util::size::Size;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

/// `MapSnapshotterObserver`.
pub trait MapSnapshotterObserver {
    fn on_did_fail_loading_style(&self, _error: &str) {}
    fn on_did_finish_loading_style(&self) {}
    fn on_style_image_missing(&self, _id: &str) {}
}

/// `MapSnapshotterObserver::nullObserver()`.
pub struct NullMapSnapshotterObserver;

impl MapSnapshotterObserver for NullMapSnapshotterObserver {}

/// `MapSnapshotter::PointForFn`.
pub type PointForFn = Box<dyn Fn(&LatLng) -> ScreenCoordinate + Send>;
/// `MapSnapshotter::LatLngForFn`.
pub type LatLngForFn = Box<dyn Fn(&ScreenCoordinate) -> LatLng + Send>;
/// `MapSnapshotter::Attributions`.
pub type Attributions = Vec<String>;
/// `MapSnapshotter::Callback`: the error (if any), the image, the sources'
/// attributions and the coordinate conversions of the snapshot's camera.
pub type Callback = Box<dyn FnMut(Option<ExceptionPtr>, PremultipliedImage, Attributions, PointForFn, LatLngForFn)>;

fn empty_image() -> PremultipliedImage {
    PremultipliedImage::new(Size::default())
}

/// `SnapshotterRenderer`: the HeadlessFrontend that renders the snapshots,
/// and the observer of its renderer (C++: on the "Snapshotter" thread).
struct SnapshotterRenderer {
    still_image: RefCell<PremultipliedImage>,
    has_pending_still_image_request: Cell<bool>,
    renderer_observer: RefCell<Option<Weak<dyn RendererObserver>>>,
    frontend: HeadlessFrontend,
}

impl SnapshotterRenderer {
    fn new(size: Size, pixel_ratio: f32) -> Self {
        SnapshotterRenderer {
            still_image: RefCell::new(empty_image()),
            has_pending_still_image_request: Cell::new(false),
            renderer_observer: RefCell::new(None),
            frontend: HeadlessFrontend::with_options(
                size,
                pixel_ratio,
                SwapBehaviour::NoFlush,
                ContextMode::Unique,
                true,
            ),
        }
    }

    fn renderer_observer(&self) -> Option<Rc<dyn RendererObserver>> {
        self.renderer_observer.borrow().as_ref().and_then(Weak::upgrade)
    }

    fn reset(&self) {
        self.has_pending_still_image_request.set(false);
        self.frontend.reset();
    }

    fn set_observer(self: &Rc<Self>, observer: Weak<dyn RendererObserver>) {
        *self.renderer_observer.borrow_mut() = Some(observer);
        let this: Rc<dyn RendererObserver> = self.clone();
        self.frontend.set_observer(Rc::downgrade(&this));
    }

    fn update(&self, params: Arc<UpdateParameters>) {
        self.has_pending_still_image_request.set(params.still_image_request);
        self.frontend.update(params);
    }

    fn set_size(&self, size: Size) {
        self.frontend.clone().set_size(size);
    }

    fn take_image(&self) -> PremultipliedImage {
        debug_assert!(self.still_image.borrow().valid());
        std::mem::replace(&mut *self.still_image.borrow_mut(), empty_image())
    }
}

impl RendererObserver for SnapshotterRenderer {
    fn on_invalidate(&self) {
        if let Some(observer) = self.renderer_observer() {
            observer.on_invalidate();
        }
    }

    fn on_resource_error(&self, error: ExceptionPtr) {
        self.has_pending_still_image_request.set(false);
        if let Some(observer) = self.renderer_observer() {
            observer.on_resource_error(error);
        }
    }

    fn on_did_finish_rendering_frame(&self, mode: RenderMode, repaint_needed: bool, placement_changed: bool) {
        if mode == RenderMode::Full && self.has_pending_still_image_request.get() {
            *self.still_image.borrow_mut() = self.frontend.read_still_image();
        }
        if let Some(observer) = self.renderer_observer() {
            observer.on_did_finish_rendering_frame(mode, repaint_needed, placement_changed);
        }
    }

    fn on_style_image_missing(&self, id: &str, done: &mut dyn FnMut()) {
        match self.renderer_observer() {
            Some(observer) => observer.on_style_image_missing(id, done),
            None => done(),
        }
    }

    // C++ forwards only the calls above (and onSymbolError, which the port's
    // RendererObserver does not have yet); the others keep the base class's
    // empty implementations.
}

struct FrontendShared {
    update_parameters: RefCell<Option<Arc<UpdateParameters>>>,
    renderer: Rc<SnapshotterRenderer>,
}

/// `SnapshotterRendererFrontend`: the map's RendererFrontend. A handle;
/// `Clone` shares it (the Map keeps a clone, C++ a reference).
#[derive(Clone)]
struct SnapshotterRendererFrontend {
    shared: Rc<FrontendShared>,
}

impl SnapshotterRendererFrontend {
    fn new(size: Size, pixel_ratio: f32) -> Self {
        SnapshotterRendererFrontend {
            shared: Rc::new(FrontendShared {
                update_parameters: RefCell::new(None),
                renderer: Rc::new(SnapshotterRenderer::new(size, pixel_ratio)),
            }),
        }
    }

    fn set_size(&self, size: Size) {
        self.shared.renderer.set_size(size);
    }

    /// `getTransformState()`: the last update's, or a default one.
    fn get_transform_state(&self) -> TransformState {
        let update_parameters = self.shared.update_parameters.borrow();
        debug_assert!(update_parameters.is_some());
        match update_parameters.as_ref() {
            Some(parameters) => parameters.transform_state.clone(),
            None => TransformState::default(),
        }
    }

    fn take_image(&self) -> PremultipliedImage {
        self.shared.renderer.take_image()
    }
}

impl RendererFrontend for SnapshotterRendererFrontend {
    fn reset(&self) {
        self.shared.renderer.reset();
    }

    fn set_observer(&self, observer: Weak<dyn RendererObserver>) {
        self.shared.renderer.set_observer(observer);
    }

    fn update(&self, parameters: Arc<UpdateParameters>) {
        *self.shared.update_parameters.borrow_mut() = Some(parameters.clone());
        self.shared.renderer.update(parameters);
    }
}

/// The MapObserver overrides of `MapSnapshotter::Impl`.
struct SnapshotterMapObserver {
    observer: Rc<dyn MapSnapshotterObserver>,
}

impl MapObserver for SnapshotterMapObserver {
    fn on_did_fail_loading_map(&self, _error: MapLoadError, what: &str) {
        self.observer.on_did_fail_loading_style(what);
    }

    fn on_did_finish_loading_style(&self) {
        self.observer.on_did_finish_loading_style();
    }

    fn on_style_image_missing(&self, id: &str) {
        self.observer.on_style_image_missing(id);
    }
}

/// The target of `renderStillCallback` (`Actor<MapSnapshotter::Callback>`).
struct RenderStillCallback {
    callback: RefCell<Callback>,
    impl_: Weak<Impl>,
}

impl RenderStillCallback {
    fn call(
        &self,
        error: Option<ExceptionPtr>,
        image: PremultipliedImage,
        attributions: Attributions,
        point_for_fn: PointForFn,
        lat_lng_for_fn: LatLngForFn,
    ) {
        (self.callback.borrow_mut())(error, image, attributions, point_for_fn, lat_lng_for_fn);
        // C++ resets `renderStillCallback` here, whichever snapshot it
        // belongs to by now (a snapshot started from the callback loses its
        // actor, as in C++).
        if let Some(impl_) = self.impl_.upgrade() {
            impl_.cancel();
        }
    }
}

/// `MapSnapshotter::Impl`. Fields drop in declaration order, which is the
/// C++ destruction order (the map before the frontend).
struct Impl {
    map: RefCell<Map>,
    frontend: SnapshotterRendererFrontend,
    region: RefCell<LatLngBounds>,
    region_insets: Cell<EdgeInsets>,
    render_still_callback: RefCell<Option<Arc<Mailbox>>>,
}

impl Impl {
    fn new(
        size: Size,
        pixel_ratio: f32,
        resource_options: &ResourceOptions,
        client_options: &ClientOptions,
        observer: Rc<dyn MapSnapshotterObserver>,
    ) -> Self {
        let frontend = SnapshotterRendererFrontend::new(size, pixel_ratio);
        let map = Map::with_resource_options(
            &frontend,
            Rc::new(SnapshotterMapObserver { observer }),
            MapOptions::new()
                .with_map_mode(MapMode::Static)
                .with_size(size)
                .with_pixel_ratio(pixel_ratio),
            resource_options,
            client_options,
        );
        Impl {
            map: RefCell::new(map),
            frontend,
            region: RefCell::new(LatLngBounds::empty()),
            region_insets: Cell::new(EdgeInsets::default()),
            render_still_callback: RefCell::new(None),
        }
    }

    fn set_region(&self, region: &LatLngBounds) {
        *self.region.borrow_mut() = *region;
        let lat_lngs = [region.southwest(), region.northeast()];
        let mut map = self.map.borrow_mut();
        let camera = map.camera_for_lat_lngs(&lat_lngs, &self.region_insets.get(), None, None);
        map.jump_to(&camera);
    }

    fn set_padding(&self, insets: &EdgeInsets) {
        self.region_insets.set(*insets);
        let region = *self.region.borrow();
        if !region.is_empty() {
            let lat_lngs = [region.southwest(), region.northeast()];
            let mut map = self.map.borrow_mut();
            let camera = map.camera_for_lat_lngs(&lat_lngs, &self.region_insets.get(), None, None);
            map.jump_to(&camera);
        }
    }

    fn get_padding(&self) -> EdgeInsets {
        self.region_insets.get()
    }

    fn snapshot(self: &Rc<Self>, mut callback: Callback) {
        // C++ logs "MapSnapshotter::Callback is not set" for an empty
        // std::function; a Rust callback can't be empty.
        if self.render_still_callback.borrow().is_some() {
            callback(
                Some(MisuseException::ptr("MapSnapshotter is currently rendering an image")),
                empty_image(),
                Vec::new(),
                Box::new(|_: &LatLng| ScreenCoordinate::default()),
                Box::new(|_: &ScreenCoordinate| LatLng::default()),
            );
            // C++ does not return here: it goes on with the same callback,
            // replacing the pending snapshot's actor.
        }

        let target = Rc::new(RenderStillCallback {
            callback: RefCell::new(callback),
            impl_: Rc::downgrade(self),
        });
        let mailbox = Mailbox::with_target(&scheduler::get_current(), target);
        let actor_ref = ActorRef::<RenderStillCallback>::new(&mailbox);
        let previous = self.render_still_callback.borrow_mut().replace(mailbox);
        if let Some(previous) = previous {
            previous.close();
        }

        // The still image callback runs from a later frame, or right away
        // (inside `render_still`, while this borrows the map) when the map
        // refuses the request; then it is finished below, once the borrow
        // has ended.
        let deferred: Rc<RefCell<Option<Option<ExceptionPtr>>>> = Rc::new(RefCell::new(None));
        let weak = Rc::downgrade(self);
        let (deferred_error, finish_ref) = (deferred.clone(), actor_ref.clone());
        self.map.borrow_mut().render_still(Box::new(move |error: Option<ExceptionPtr>| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            match this.map.try_borrow() {
                Ok(map) => this.finish_snapshot(&map, error, &finish_ref),
                Err(_) => *deferred_error.borrow_mut() = Some(error),
            }
        }));
        let deferred = deferred.borrow_mut().take();
        if let Some(error) = deferred {
            self.finish_snapshot(&self.map.borrow(), error, &actor_ref);
        }
    }

    /// The body of the `map.renderStill` lambda in `Impl::snapshot`.
    fn finish_snapshot(&self, map: &Map, error: Option<ExceptionPtr>, actor_ref: &ActorRef<RenderStillCallback>) {
        // Create lambda that captures the current transform state and can be
        // used to translate for geographic to screen coordinates
        let center = map.get_camera_options(None).center;
        debug_assert!(center.is_some());
        let center = center.unwrap_or_default();
        let transform_state = self.frontend.get_transform_state();
        let point_for_fn: PointForFn = Box::new(move |lat_lng: &LatLng| {
            let mut unwrapped_lat_lng = lat_lng.wrapped();
            unwrapped_lat_lng.unwrap_for_shortest_path(&center);
            let transform = Transform::from_state(transform_state.clone());
            transform.lat_lng_to_screen_coordinate(&unwrapped_lat_lng)
        });

        // Create lambda that captures the current transform state and can be
        // used to translate for geographic to screen coordinates
        let transform_state = self.frontend.get_transform_state();
        let lat_lng_for_fn: LatLngForFn = Box::new(move |screen_coordinate: &ScreenCoordinate| {
            let transform = Transform::from_state(transform_state.clone());
            transform.screen_coordinate_to_lat_lng(screen_coordinate, WrapMode::Wrapped)
        });

        // Collect all source attributions
        let attributions: Attributions = map
            .get_style()
            .get_sources()
            .iter()
            .filter_map(|source| source.get_attribution())
            .collect();

        // Invoke callback
        let image = if error.is_some() {
            empty_image()
        } else {
            self.frontend.take_image()
        };
        actor_ref.invoke(move |callback: &RenderStillCallback| {
            callback.call(error, image, attributions, point_for_fn, lat_lng_for_fn);
        });
    }

    /// `cancel()`: `renderStillCallback.reset()`.
    fn cancel(&self) {
        let callback = self.render_still_callback.borrow_mut().take();
        if let Some(callback) = callback {
            callback.close();
        }
    }
}

impl Drop for Impl {
    fn drop(&mut self) {
        // `~Actor()` closes the mailbox.
        self.cancel();
    }
}

/// `MapSnapshotter`: renders still images of a style off screen.
pub struct MapSnapshotter {
    impl_: Rc<Impl>,
}

impl MapSnapshotter {
    /// `MapSnapshotter(Size, float pixelRatio, const ResourceOptions&, const
    /// ClientOptions&, MapSnapshotterObserver&, localFontFamily = nullopt)`.
    pub fn with_observer(
        size: Size,
        pixel_ratio: f32,
        resource_options: &ResourceOptions,
        client_options: &ClientOptions,
        observer: Rc<dyn MapSnapshotterObserver>,
    ) -> Self {
        MapSnapshotter {
            impl_: Rc::new(Impl::new(size, pixel_ratio, resource_options, client_options, observer)),
        }
    }

    /// `MapSnapshotter(Size, float pixelRatio, const ResourceOptions&, const
    /// ClientOptions& = ClientOptions())`: with the null observer.
    pub fn new(size: Size, pixel_ratio: f32, resource_options: &ResourceOptions, client_options: &ClientOptions) -> Self {
        Self::with_observer(
            size,
            pixel_ratio,
            resource_options,
            client_options,
            Rc::new(NullMapSnapshotterObserver),
        )
    }

    pub fn set_style_url(&self, style_url: &str) {
        self.impl_.map.borrow_mut().get_style_mut().load_url(style_url);
    }

    pub fn get_style_url(&self) -> String {
        self.impl_.map.borrow().get_style().get_url()
    }

    pub fn set_style_json(&self, style_json: &str) {
        self.impl_.map.borrow_mut().get_style_mut().load_json(style_json);
    }

    pub fn get_style_json(&self) -> String {
        self.impl_.map.borrow().get_style().get_json()
    }

    /// `setSize(const Size&)`. C++ throws from `Map::setSize` for an empty
    /// size; the port returns that error.
    pub fn set_size(&self, size: Size) -> Result<(), String> {
        self.impl_.map.borrow_mut().set_size(size)?;
        self.impl_.frontend.set_size(size);
        Ok(())
    }

    pub fn get_size(&self) -> Size {
        self.impl_.map.borrow().get_map_options().size()
    }

    pub fn set_camera_options(&self, options: &CameraOptions) {
        let mut options_with_padding = options.clone();
        if options.padding.is_none() {
            // Apply region padding if no padding is explicitly set in camera
            // options
            options_with_padding = options_with_padding.with_padding(Some(self.impl_.get_padding()));
        }
        self.impl_.map.borrow_mut().jump_to(&options_with_padding);
    }

    pub fn get_camera_options(&self) -> CameraOptions {
        self.impl_.map.borrow().get_camera_options(None)
    }

    pub fn set_region(&self, region: &LatLngBounds) {
        self.impl_.set_region(region);
    }

    pub fn get_region(&self) -> LatLngBounds {
        let map = self.impl_.map.borrow();
        let camera = map.get_camera_options(Some(self.get_padding()));
        map.lat_lng_bounds_for_camera(&camera)
    }

    pub fn set_padding(&self, insets: &EdgeInsets) {
        self.impl_.set_padding(insets);
    }

    pub fn get_padding(&self) -> EdgeInsets {
        self.impl_.get_padding()
    }

    // TODO(port): addAnnotationImage(std::unique_ptr<style::Image>) and
    // addAnnotation(const Annotation&) (Map::addAnnotation*; annotations are
    // not ported).

    /// `getStyle()`: runs `f` with the map's style.
    pub fn with_style<R>(&self, f: impl FnOnce(&mut Style) -> R) -> R {
        let mut map = self.impl_.map.borrow_mut();
        let mut style = map.get_style_mut();
        f(&mut style)
    }

    pub fn snapshot(&self, callback: Callback) {
        self.impl_.snapshot(callback);
    }

    pub fn cancel(&self) {
        self.impl_.cancel();
    }
}
