// Ports src/mln/annotation/annotation_source.hpp and
// src/mln/annotation/annotation_source.cpp.

use crate::annotation::annotation_manager::SOURCE_ID;
use mln_storage::FileSource;
use mln_style::layer::LayerTypeInfo;
use mln_style::source::{Source, SourceBase, static_immutable_cast};
use mln_style::source_impl::{self, ImplBase};
use mln_style_types::SourceType;
use mln_util::immutable::Immutable;
use std::any::Any;
use std::sync::Arc;

/// `AnnotationSource::Impl`.
#[derive(Clone, Debug)]
pub struct Impl {
    base: ImplBase,
}

impl Impl {
    /// `Impl()`.
    pub fn new() -> Self {
        Impl { base: ImplBase::new(SourceType::Annotations, SOURCE_ID.to_owned()) }
    }
}

impl Default for Impl {
    fn default() -> Self {
        Self::new()
    }
}

impl source_impl::Impl for Impl {
    fn base(&self) -> &ImplBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut ImplBase {
        &mut self.base
    }

    fn get_attribution(&self) -> Option<String> {
        None
    }
}

/// `AnnotationSource`.
#[derive(Debug)]
pub struct AnnotationSource {
    base: SourceBase,
}

impl AnnotationSource {
    /// `AnnotationSource()`.
    pub fn new() -> Self {
        let impl_: Immutable<dyn source_impl::Impl> = Arc::new(Impl::new());
        AnnotationSource { base: SourceBase::new(impl_) }
    }

    /// `impl()`.
    pub fn impl_(&self) -> Arc<Impl> {
        static_immutable_cast::<Impl>(&self.base.base_impl())
    }
}

impl Default for AnnotationSource {
    fn default() -> Self {
        Self::new()
    }
}

impl Source for AnnotationSource {
    fn base(&self) -> &SourceBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn load_description(&self, _file_source: &dyn FileSource) {
        self.base.set_loaded(true);
    }

    fn supports_layer_type(&self, info: &LayerTypeInfo) -> bool {
        info.type_ == "line" || info.type_ == "symbol" || info.type_ == "fill"
    }

    fn create_mutable(&self) -> Box<dyn source_impl::Impl> {
        Box::new((*self.impl_()).clone())
    }
}
