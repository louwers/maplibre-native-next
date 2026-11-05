// Ports src/mln/annotation/fill_annotation_impl.hpp and
// src/mln/annotation/fill_annotation_impl.cpp.

use crate::annotation::annotation::{AnnotationID, FillAnnotation, ShapeAnnotationGeometry};
use crate::annotation::annotation_manager::{POINT_LAYER_ID, SOURCE_ID};
use crate::annotation::shape_annotation_impl::{ShapeAnnotationImpl, ShapeAnnotationImplBase, close_shape_annotation};
use mln_style::Layer;
use mln_style::layers::FillLayer;
use mln_style::style_impl::Impl as StyleImpl;
use mln_util::logging::{Event, Log};

/// `FillAnnotationImpl`.
pub struct FillAnnotationImpl {
    base: ShapeAnnotationImplBase,
    annotation: FillAnnotation,
}

impl FillAnnotationImpl {
    /// `FillAnnotationImpl(AnnotationID, FillAnnotation)`.
    pub fn new(id: AnnotationID, annotation: &FillAnnotation) -> Self {
        FillAnnotationImpl {
            base: ShapeAnnotationImplBase::new(id),
            annotation: FillAnnotation {
                geometry: close_shape_annotation(&annotation.geometry),
                opacity: annotation.opacity.clone(),
                color: annotation.color.clone(),
                outline_color: annotation.outline_color.clone(),
            },
        }
    }
}

impl ShapeAnnotationImpl for FillAnnotationImpl {
    fn base(&self) -> &ShapeAnnotationImplBase {
        &self.base
    }

    fn base_mut_and_geometry(&mut self) -> (&mut ShapeAnnotationImplBase, &ShapeAnnotationGeometry) {
        (&mut self.base, &self.annotation.geometry)
    }

    fn update_style(&self, style: &mut StyleImpl) {
        let layer_id = &self.base.layer_id;
        if style.get_layer(layer_id).is_none() {
            let mut new_layer = FillLayer::new(layer_id, SOURCE_ID);
            new_layer.set_source_layer(layer_id);
            // C++ throws for a layer the source does not support (the
            // annotation source supports fill layers).
            if let Err(e) = style.add_layer(Box::new(new_layer), Some(POINT_LAYER_ID)) {
                Log::error(Event::General, e.to_string());
                return;
            }
        }

        let Some(mut layer) = style.get_layer_mut(layer_id) else {
            return;
        };
        let layer: &mut dyn Layer = &mut *layer;
        // C++ static_casts; the layer is the FillLayer created above.
        let Some(fill_layer) = layer.as_any_mut().downcast_mut::<FillLayer>() else {
            return;
        };
        fill_layer.set_fill_opacity(&self.annotation.opacity);
        fill_layer.set_fill_color(&self.annotation.color);
        fill_layer.set_fill_outline_color(&self.annotation.outline_color);
    }

    fn geometry(&self) -> &ShapeAnnotationGeometry {
        &self.annotation.geometry
    }
}
