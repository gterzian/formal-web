use boa_engine::JsData;
use boa_gc::{Finalize, Trace};

use super::{event::EventTarget, global_scope::GlobalScope};

/// <https://html.spec.whatwg.org/#window>
#[derive(Trace, Finalize, JsData)]
pub struct Window {
    /// <https://dom.spec.whatwg.org/#interface-eventtarget>
    pub event_target: EventTarget,

    /// <https://html.spec.whatwg.org/#global-object>
    pub global_scope: GlobalScope,
}
