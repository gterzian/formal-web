use std::collections::BTreeMap;
use std::mem;

use boa_engine::JsData;
use boa_gc::{Finalize, Trace};

use crate::dom::Element;
use crate::dom::event::EventTarget;
use crate::webidl::Callback;

use super::GlobalScope;
use super::resolved_style_properties_for_element;

/// <https://html.spec.whatwg.org/#window>
#[derive(Trace, Finalize, JsData)]
pub struct Window {
    /// <https://dom.spec.whatwg.org/#interface-eventtarget>
    pub event_target: EventTarget,

    /// <https://html.spec.whatwg.org/#global-object>
    pub global_scope: GlobalScope,

    /// <https://html.spec.whatwg.org/#handler-onload>
    onload: Option<Callback>,
}

impl Window {
    pub(crate) fn new(global_scope: GlobalScope) -> Self {
        Self {
            event_target: EventTarget::default(),
            global_scope,
            onload: None,
        }
    }

    /// <https://html.spec.whatwg.org/#handler-onload>
    pub(crate) fn onload_value(&self) -> Option<Callback> {
        self.onload.clone()
    }

    /// <https://html.spec.whatwg.org/#handler-onload>
    pub(crate) fn replace_onload(&mut self, callback: Option<Callback>) -> Option<Callback> {
        mem::replace(&mut self.onload, callback)
    }
}

/// <https://drafts.csswg.org/cssom/#dom-window-getcomputedstyle>
pub(crate) fn window_computed_style_properties_for_element(
    elt: &Element,
    pseudo_elt: Option<&str>,
) -> BTreeMap<String, String> {
    // Step 1: "Let doc be elt's node document."
    // Note: The style resolution helper reads elt's node document through the DOM carrier.

    // Step 2: "Let obj be elt."
    let mut obj = Some(elt);

    // Step 3: "If pseudoElt is provided, is not the empty string, and starts with a colon..."
    if let Some(pseudo_elt) = pseudo_elt.map(str::trim).filter(|value| !value.is_empty()) {
        if pseudo_elt.starts_with(':') {
            // Step 3.1: Parse pseudoElt as a <pseudo-element-selector>.
            // Step 3.2 / 3.3: Map invalid, ::slotted(), ::part(), or supported pseudo-element
            // requests to the corresponding pseudo-element object.
            //
            // Note: The current runtime does not yet expose pseudo-element carriers, so any
            // pseudo-element request leaves `obj` null and therefore produces an empty declaration
            // list below.
            obj = None;
        }
    }

    // Step 4: "Let decls be an empty list of CSS declarations."
    let mut decls = BTreeMap::new();

    // Step 5: "If obj is not null, and elt is connected, part of the flat tree, and its
    // shadow-including root has a browsing context ... being rendered, set decls ..."
    //
    // Note: The current runtime represents the connected predicate, but it does not yet model flat
    // tree membership, pseudo-elements, or the browsing-context-container rendering gate. The
    // populated branch therefore uses the connected element carrier that exists today.
    if let Some(obj) = obj.filter(|element| element.is_connected()) {
        decls = resolved_style_properties_for_element(obj);
    }

    // Step 6: "Return a live CSSStyleProperties object ... declarations decls ... owner node obj."
    // Note: The binding layer currently wraps this declaration snapshot in a plain JS object while
    // native CSSStyleProperties liveness is still pending.
    decls
}
