use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

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

/// <https://html.spec.whatwg.org/#list-of-animation-frame-callbacks>
#[derive(Trace, Finalize)]
pub struct AnimationFrameCallback {
    /// <https://html.spec.whatwg.org/#animation-frame-callback-identifier>
    #[unsafe_ignore_trace]
    pub handle: u32,

    /// <https://webidl.spec.whatwg.org/#idl-callback-function>
    pub callback: JsObject,
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

    /// <https://html.spec.whatwg.org/#animation-frame-callback-identifier>
    #[unsafe_ignore_trace]
    pub animation_frame_callback_identifier: Cell<u32>,

    /// <https://html.spec.whatwg.org/#list-of-animation-frame-callbacks>
    pub animation_frame_callbacks: GcRefCell<Vec<AnimationFrameCallback>>,
}

impl RuntimeData {
    pub fn new(document: Rc<RefCell<BaseDocument>>) -> Self {
        Self {
            document,
            document_object: GcRefCell::new(None),
            node_objects: GcRefCell::new(Vec::new()),
            animation_frame_callback_identifier: Cell::new(0),
            animation_frame_callbacks: GcRefCell::new(Vec::new()),
        }
    }
}
