// Ports include/mln/webgpu/render_pass.hpp and src/mln/webgpu/render_pass.cpp
//
// The C++ pass keeps the descriptor (with a `Renderable&`); drawables read
// `getDescriptor().renderable` for its size (and pass it, unused, to
// `getRenderPipeline`). The port keeps the size and an identity of the
// renderable instead of the reference.

use std::any::Any;
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::rc::Rc;
use std::sync::OnceLock;

use mln_gfx::context::Context as _;
use mln_gfx::debug_group::DebugGroupTarget;
use mln_gfx::render_pass::{self, RenderPassDescriptor};
use mln_gfx::uniform_buffer::UniformBufferArray;
use mln_util::size::Size;

use crate::command_encoder::CommandEncoder;
use crate::context::Context;
use crate::renderable_resource::as_webgpu;

pub struct RenderPass {
    context: Rc<Context>,
    renderable_size: Size,
    /// Address of the renderable's resource (`clipMaskRenderable` compares it).
    renderable_id: usize,
    encoder: RefCell<Option<wgpu::RenderPass<'static>>>,
    global_uniform_buffers_bound: Cell<bool>,
    previous_color_format: Option<wgpu::TextureFormat>,
    previous_depth_stencil_format: Option<wgpu::TextureFormat>,
    color_format_updated: bool,
    depth_format_updated: bool,
}

fn debug_groups_enabled() -> bool {
    // Debug groups on render pass encoders can cause issues in some Dawn
    // versions; enable with an environment variable if needed.
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("WEBGPU_ENABLE_DEBUG_GROUPS").is_some())
}

impl RenderPass {
    pub fn new(command_encoder: &CommandEncoder, name: &str, descriptor: &RenderPassDescriptor<'_>) -> Self {
        let context = command_encoder.get_context().clone();
        let backend = context.get_backend().clone();
        let mut pass = RenderPass {
            context: context.clone(),
            renderable_size: descriptor.renderable.get_size(),
            renderable_id: 0,
            encoder: RefCell::new(None),
            global_uniform_buffers_bound: Cell::new(false),
            previous_color_format: backend.get_color_format(),
            previous_depth_stencil_format: backend.get_depth_stencil_format(),
            color_format_updated: false,
            depth_format_updated: false,
        };

        let mut encoder_guard = command_encoder.get_encoder();
        let Some(command_encoder) = encoder_guard.as_mut() else {
            eprintln!("WebGPU: Command encoder unavailable");
            return pass;
        };

        // Access the renderable resource associated with this pass
        let Some(resource) = descriptor.renderable.get_resource() else {
            eprintln!("WebGPU: No renderable resource available");
            return pass;
        };
        pass.renderable_id = Rc::as_ptr(&resource) as *const () as usize;
        let Some(renderable_resource) = as_webgpu(&*resource) else {
            eprintln!("WebGPU: No renderable resource available");
            return pass;
        };
        renderable_resource.bind();

        // Resolve the color attachment view, preferring the renderable's
        // texture view. TODO(port): fall back to backend.getCurrentTextureView().
        let Some(color_view) = renderable_resource.get_color_texture_view() else {
            eprintln!("WebGPU: Failed to acquire color attachment view");
            return pass;
        };
        if let Some(color_format) = renderable_resource.get_color_texture_format() {
            backend.set_color_format(Some(color_format));
            pass.color_format_updated = true;
        }

        let color_ops = wgpu::Operations {
            load: match descriptor.clear_color {
                Some(c) => wgpu::LoadOp::Clear(wgpu::Color {
                    r: f64::from(c.r),
                    g: f64::from(c.g),
                    b: f64::from(c.b),
                    a: f64::from(c.a),
                }),
                None => wgpu::LoadOp::Load,
            },
            store: wgpu::StoreOp::Store,
        };

        // TODO(port): fall back to backend.getDepthStencilView().
        let depth_view = renderable_resource.get_depth_stencil_texture_view();
        if depth_view.is_some()
            && let Some(depth_format) = renderable_resource.get_depth_stencil_texture_format()
        {
            backend.set_depth_stencil_format(Some(depth_format));
            pass.depth_format_updated = true;
        }
        let depth_stencil_attachment = depth_view.as_ref().map(|view| wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations {
                load: match descriptor.clear_depth {
                    Some(d) => wgpu::LoadOp::Clear(d),
                    None => wgpu::LoadOp::Load,
                },
                store: wgpu::StoreOp::Store,
            }),
            // C++ uses Undefined load/store ops without a stencil clear; in
            // wgpu `None` (read-only stencil aspect) is the equivalent.
            stencil_ops: descriptor
                .clear_stencil
                .map(|s| wgpu::Operations { load: wgpu::LoadOp::Clear(s as u32), store: wgpu::StoreOp::Store }),
        });

        let encoder = command_encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(name),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: color_ops,
                })],
                depth_stencil_attachment,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        drop(encoder_guard);

        let mut encoder = encoder;
        context.perform_cleanup();
        let size = pass.renderable_size;
        encoder.set_viewport(0.0, 0.0, size.width as f32, size.height as f32, 0.0, 1.0);
        encoder.set_scissor_rect(0, 0, size.width, size.height);
        *pass.encoder.borrow_mut() = Some(encoder);
        pass
    }

    /// `getEncoder()`: the wgpu render pass, `None` if it failed to begin.
    pub fn get_encoder(&self) -> RefMut<'_, Option<wgpu::RenderPass<'static>>> {
        self.encoder.borrow_mut()
    }

    pub fn has_encoder(&self) -> bool {
        self.encoder.borrow().is_some()
    }

    /// `getDescriptor().renderable.getSize()`
    pub fn get_renderable_size(&self) -> Size {
        self.renderable_size
    }

    pub fn get_renderable_id(&self) -> usize {
        self.renderable_id
    }

    pub fn get_context(&self) -> &Rc<Context> {
        &self.context
    }

    /// `setGlobalUniformBuffers(ptr)`: bound or not; the array is the
    /// context's (`Context::bindGlobalUniformBuffers`).
    pub fn set_global_uniform_buffers(&self, bound: bool) {
        self.global_uniform_buffers_bound.set(bound);
    }

    pub fn get_global_uniform_buffers(&self) -> Option<Ref<'_, UniformBufferArray>> {
        self.global_uniform_buffers_bound.get().then(|| self.context.get_global_uniform_buffers())
    }
}

impl Drop for RenderPass {
    fn drop(&mut self) {
        // End the render pass
        self.encoder.borrow_mut().take();
        let backend = self.context.get_backend();
        if self.color_format_updated {
            backend.set_color_format(self.previous_color_format);
        }
        if self.depth_format_updated {
            backend.set_depth_stencil_format(self.previous_depth_stencil_format);
        }
    }
}

impl DebugGroupTarget for RenderPass {
    fn push_debug_group(&self, name: &str) {
        if debug_groups_enabled()
            && let Some(encoder) = self.encoder.borrow_mut().as_mut()
        {
            encoder.push_debug_group(name);
        }
    }

    fn pop_debug_group(&self) {
        if debug_groups_enabled()
            && let Some(encoder) = self.encoder.borrow_mut().as_mut()
        {
            encoder.pop_debug_group();
        }
    }
}

impl render_pass::RenderPass for RenderPass {
    fn add_debug_signpost(&self, name: &str) {
        if let Some(encoder) = self.encoder.borrow_mut().as_mut() {
            encoder.insert_debug_marker(name);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
