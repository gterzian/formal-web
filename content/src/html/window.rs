use std::mem;

use boa_engine::JsData;
use boa_gc::{Finalize, Trace};

use crate::dom::event::EventTarget;
use crate::webidl::Callback;

use super::GlobalScope;

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
