use js_engine::gc_struct;

/// <https://dom.spec.whatwg.org/#domimplementation>
#[gc_struct]
pub(crate) struct DOMImplementation {}

impl DOMImplementation {
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// <https://dom.spec.whatwg.org/#dom-domimplementation-hasfeature>
    pub(crate) fn has_feature(&self) -> bool {
        // The hasFeature() method steps are to return true.
        true
    }
}
