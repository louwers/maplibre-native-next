// Ports include/mln/storage/resource_transform.hpp and
// src/mln/storage/resource_transform.cpp.

use crate::resource::Kind;
use std::sync::Arc;

/// `ResourceTransform::FinishedCallback` (`std::function<void(const std::string&)>`).
pub type FinishedCallback = Box<dyn FnOnce(String) + Send>;

/// `ResourceTransform::TransformCallback`. It runs on the online file
/// source's thread, hence `Send + Sync`.
pub type TransformCallback = Arc<dyn Fn(Kind, &str, FinishedCallback) + Send + Sync>;

/// `ResourceTransform`: rewrites request URLs (asynchronously) before the
/// online file source requests them.
#[derive(Clone, Default)]
pub struct ResourceTransform {
    transform_callback: Option<TransformCallback>,
}

impl ResourceTransform {
    /// `ResourceTransform(TransformCallback = {})`.
    pub fn new(callback: Option<TransformCallback>) -> Self {
        ResourceTransform { transform_callback: callback }
    }

    /// `transform(Resource::Kind, const std::string& url, FinishedCallback)`.
    pub fn transform(&self, kind: Kind, url: &str, finished: FinishedCallback) {
        let callback = self.transform_callback.as_ref().expect("transformCallback");
        callback(kind, url, finished);
    }

    /// `explicit operator bool()`.
    pub fn is_set(&self) -> bool {
        self.transform_callback.is_some()
    }
}
