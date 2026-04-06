use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::{JsData, object::JsObject};
use boa_gc::{Finalize, GcRefCell, Trace};

/// <https://html.spec.whatwg.org/#global-object>
#[derive(Trace, Finalize)]
pub struct CachedNodeObject {
    /// <https://dom.spec.whatwg.org/#interface-node>
    #[unsafe_ignore_trace]
    pub node_id: usize,

    /// <https://webidl.spec.whatwg.org/#dfn-platform-object>
    pub object: JsObject,
}

/// <https://html.spec.whatwg.org/#environment>
#[derive(Trace, Finalize, JsData)]
pub struct RuntimeData {
    /// <https://dom.spec.whatwg.org/#concept-node-document>
    #[unsafe_ignore_trace]
    pub document: Rc<RefCell<BaseDocument>>,

    /// <https://html.spec.whatwg.org/#concept-document-window>
    pub document_object: GcRefCell<Option<JsObject>>,

    /// <https://webidl.spec.whatwg.org/#dfn-platform-object>
    pub node_objects: GcRefCell<Vec<CachedNodeObject>>,
}

impl RuntimeData {
    pub fn new(document: Rc<RefCell<BaseDocument>>) -> Self {
        Self {
            document,
            document_object: GcRefCell::new(None),
            node_objects: GcRefCell::new(Vec::new()),
        }
    }
}