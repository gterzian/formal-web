use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use blitz_dom::BaseDocument;
use boa_engine::object::JsObject;
use boa_gc::{Finalize, GcRefCell, Trace};

/// <https://html.spec.whatwg.org/#global-object>
#[derive(Debug, Clone, Copy)]
pub enum GlobalScopeKind {
    Window,
}

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

/// <https://html.spec.whatwg.org/#global-object>
#[derive(Trace, Finalize)]
pub struct GlobalScope {
    /// <https://html.spec.whatwg.org/#global-object>
    #[unsafe_ignore_trace]
    pub kind: GlobalScopeKind,

    /// <https://html.spec.whatwg.org/#concept-document-window>
    #[unsafe_ignore_trace]
    document: Rc<RefCell<BaseDocument>>,

    /// <https://dom.spec.whatwg.org/#interface-document>
    document_object: GcRefCell<Option<JsObject>>,

    /// <https://webidl.spec.whatwg.org/#dfn-platform-object>
    node_objects: GcRefCell<Vec<CachedNodeObject>>,

    /// <https://html.spec.whatwg.org/#animation-frame-callback-identifier>
    #[unsafe_ignore_trace]
    animation_frame_callback_identifier: Cell<u32>,

    /// <https://html.spec.whatwg.org/#list-of-animation-frame-callbacks>
    animation_frame_callbacks: GcRefCell<Vec<AnimationFrameCallback>>,
}

impl GlobalScope {
    pub fn new(kind: GlobalScopeKind, document: Rc<RefCell<BaseDocument>>) -> Self {
        Self {
            kind,
            document,
            document_object: GcRefCell::new(None),
            node_objects: GcRefCell::new(Vec::new()),
            animation_frame_callback_identifier: Cell::new(0),
            animation_frame_callbacks: GcRefCell::new(Vec::new()),
        }
    }

    pub(crate) fn document(&self) -> Rc<RefCell<BaseDocument>> {
        Rc::clone(&self.document)
    }

    pub(crate) fn document_object(&self) -> Option<JsObject> {
        self.document_object.borrow().clone()
    }

    pub(crate) fn store_document_object(&self, object: JsObject) {
        self.document_object.borrow_mut().replace(object);
    }

    pub(crate) fn cached_node_object(&self, node_id: usize) -> Option<JsObject> {
        self.node_objects
            .borrow()
            .iter()
            .find(|entry| entry.node_id == node_id)
            .map(|entry| entry.object.clone())
    }

    pub(crate) fn cache_node_object(&self, node_id: usize, object: JsObject) {
        self.node_objects
            .borrow_mut()
            .push(CachedNodeObject { node_id, object });
    }

    /// <https://html.spec.whatwg.org/#dom-animationframeprovider-requestanimationframe>
    pub(crate) fn request_animation_frame(&self, callback: JsObject) -> u32 {
        // Step 3: "Increment target's animation frame callback identifier by one, and let handle be the result."
        let callbacks = self.animation_frame_callbacks.borrow();
        let mut handle = self.animation_frame_callback_identifier.get();

        loop {
            handle = handle.wrapping_add(1);
            if handle == 0 {
                continue;
            }
            if callbacks.iter().all(|entry| entry.handle != handle) {
                break;
            }
        }

        drop(callbacks);
        self.animation_frame_callback_identifier.set(handle);

        // Step 4: "Let callbacks be target's map of animation frame callbacks."
        // Step 5: "Set callbacks[handle] to callback."
        self.animation_frame_callbacks
            .borrow_mut()
            .push(AnimationFrameCallback { handle, callback });

        // Step 6: "Return handle."
        handle
    }

    /// <https://html.spec.whatwg.org/#animationframeprovider-cancelanimationframe>
    pub(crate) fn cancel_animation_frame(&self, handle: u32) {
        // Step 2: "Let callbacks be this's target object's map of animation frame callbacks."

        // Step 3: "Remove callbacks[handle]."
        self.animation_frame_callbacks
            .borrow_mut()
            .retain(|entry| entry.handle != handle);
    }

    /// <https://html.spec.whatwg.org/#run-the-animation-frame-callbacks>
    pub(crate) fn take_animation_frame_callbacks(&self) -> Vec<JsObject> {
        // Step 1: "Let callbacks be target's map of animation frame callbacks."

        // Step 2: "Let callbackHandles be the result of getting the keys of callbacks."
        let callback_handles: Vec<u32> = self
            .animation_frame_callbacks
            .borrow()
            .iter()
            .map(|entry| entry.handle)
            .collect();

        // Step 3: "For each handle in callbackHandles, if handle exists in callbacks:"
        let mut callbacks = self.animation_frame_callbacks.borrow_mut();
        let mut taken = Vec::with_capacity(callback_handles.len());
        for handle in callback_handles {
            let Some(index) = callbacks.iter().position(|entry| entry.handle == handle) else {
                continue;
            };
            taken.push(callbacks.remove(index).callback.clone());
        }
        taken
    }
}
