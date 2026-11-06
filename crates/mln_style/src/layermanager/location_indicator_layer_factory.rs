// Ports include/mln/layermanager/location_indicator_layer_factory.hpp +
// src/mln/layermanager/location_indicator_layer_factory.cpp (style half).

use super::layer_factory::LayerFactory;
use crate::layer::{Layer, LayerTypeInfo};
use crate::layers::location_indicator_layer::{Impl, LocationIndicatorLayer};
use mln_conversion::Convertible;

pub struct LocationIndicatorLayerFactory;

impl LayerFactory for LocationIndicatorLayerFactory {
    fn get_type_info(&self) -> &'static LayerTypeInfo {
        Impl::static_type_info()
    }

    fn create_layer(&self, id: &str, _value: &Convertible<'_>) -> Option<Box<dyn Layer>> {
        Some(Box::new(LocationIndicatorLayer::new(id)))
    }
}
