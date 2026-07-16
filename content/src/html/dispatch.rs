use js_engine::{Completion, ExecutionContext};

use crate::dom::event::Event;
use crate::dom::{dispatch_event, simple_path};
use crate::html::Window;
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;

/// <https://html.spec.whatwg.org/multipage/#steps-to-fire-beforeunload>
/// <https://dom.spec.whatwg.org/#concept-event-fire>
pub(crate) fn fire_global_event(
    ec: &mut dyn ExecutionContext<Types>,
    event_type: &str,
    cancelable: bool,
    time_millis: f64,
) -> Completion<bool, Types> {
    let target_object = ec.global_object();
    let event_target = ec
        .with_object_any(&target_object)
        .and_then(|data| data.downcast_ref::<Window>())
        .map(|window| window.event_target.clone())
        .unwrap_or_default();
    let event = Event::new(event_type.into(), false, cancelable, false, true, time_millis);
    let event_object = create_interface_instance::<Types, Event>(event, ec)?;
    let event: Event = ec
        .with_object_any(&event_object)
        .and_then(|data| data.downcast_ref::<Event>().cloned())
        .ok_or_else(|| ec.new_type_error("event_object is not an Event"))?;
    let path = simple_path(&event_target);
    dispatch_event(ec, &path, &event)
}
