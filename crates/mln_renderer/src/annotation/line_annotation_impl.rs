// Ports src/mln/annotation/line_annotation_impl.hpp and
// src/mln/annotation/line_annotation_impl.cpp.

use crate::annotation::annotation::{AnnotationID, LineAnnotation, ShapeAnnotationGeometry};
use crate::annotation::annotation_manager::{POINT_LAYER_ID, SOURCE_ID};
use crate::annotation::shape_annotation_impl::{ShapeAnnotationImpl, ShapeAnnotationImplBase, close_shape_annotation};
use mln_style::layers::LineLayer;
use mln_style::style_impl::Impl as StyleImpl;
use mln_style::{Layer, PropertyValue};
use mln_style_types::LineJoinType;
use mln_util::logging::{Event, Log};

/// `LineAnnotationImpl`.
pub struct LineAnnotationImpl {
    base: ShapeAnnotationImplBase,
    annotation: LineAnnotation,
}

impl LineAnnotationImpl {
    /// `LineAnnotationImpl(AnnotationID, LineAnnotation)`.
    pub fn new(id: AnnotationID, annotation: &LineAnnotation) -> Self {
        LineAnnotationImpl {
            base: ShapeAnnotationImplBase::new(id),
            annotation: LineAnnotation {
                geometry: close_shape_annotation(&annotation.geometry),
                opacity: annotation.opacity.clone(),
                width: annotation.width.clone(),
                color: annotation.color.clone(),
            },
        }
    }
}

impl ShapeAnnotationImpl for LineAnnotationImpl {
    fn base(&self) -> &ShapeAnnotationImplBase {
        &self.base
    }

    fn base_mut_and_geometry(&mut self) -> (&mut ShapeAnnotationImplBase, &ShapeAnnotationGeometry) {
        (&mut self.base, &self.annotation.geometry)
    }

    fn update_style(&self, style: &mut StyleImpl) {
        let layer_id = &self.base.layer_id;
        if style.get_layer(layer_id).is_none() {
            let mut new_layer = LineLayer::new(layer_id, SOURCE_ID);
            new_layer.set_source_layer(layer_id);
            new_layer.set_line_join(&PropertyValue::Constant(LineJoinType::Round));
            // C++ throws for a layer the source does not support (the
            // annotation source supports line layers).
            if let Err(e) = style.add_layer(Box::new(new_layer), Some(POINT_LAYER_ID)) {
                Log::error(Event::General, e.to_string());
                return;
            }
        }

        let Some(mut layer) = style.get_layer_mut(layer_id) else {
            return;
        };
        let layer: &mut dyn Layer = &mut *layer;
        // C++ static_casts; the layer is the LineLayer created above.
        let Some(line_layer) = layer.as_any_mut().downcast_mut::<LineLayer>() else {
            return;
        };
        line_layer.set_line_opacity(&self.annotation.opacity);
        line_layer.set_line_width(&self.annotation.width);
        line_layer.set_line_color(&self.annotation.color);
    }

    fn geometry(&self) -> &ShapeAnnotationGeometry {
        &self.annotation.geometry
    }
}
