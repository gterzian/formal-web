use js_engine::{Completion, ExecutionContext};

use crate::dom::event::Event;
use crate::dom::{dispatch_event, simple_path};
use crate::html::Window;
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;

/// <https://html.spec.whatwg.org/#steps-to-fire-beforeunload>
pub(crate) fn steps_to_fire_beforeunload(
    ec: &mut dyn ExecutionContext<Types>,
    event_type: &str,
    cancelable: bool,
    time_millis: f64,
) -> Completion<bool, Types> {
    // Step 1: Let unloadPromptCanceled be false.
    //         (Handled by caller — the return value indicates cancelation.)

    // Step 2: Increase the document's unload counter by 1.
    // TODO: Not yet implemented.

    // Step 3: Increase document's relevant agent's event loop's termination
    //         nesting level by 1.
    // TODO: Not yet implemented.

    // Step 4: Let eventFiringResult be the result of firing an event named
    //         beforeunload at document's relevant global object, using
    //         BeforeUnloadEvent, with the cancelable attribute initialized to true.
    // Note: The spec mandates BeforeUnloadEvent and a hardcoded "beforeunload"
    // type. This implementation uses a generic Event and accepts an event_type
    // parameter. The caller always passes "beforeunload".
    //
    //         <https://dom.spec.whatwg.org/#concept-event-fire>
    //         Sub-algorithm: fire an event.
    let target_object = ec.global_object();
    let event_target = ec
        .with_object_any(&target_object)
        .and_then(|data| data.downcast_ref::<Window>())
        .map(|window| window.event_target.clone())
        .unwrap_or_default();

    // Step 4, sub-step 2: Let event be the result of creating an event given
    //                     eventConstructor, in the relevant realm of target.
    // Step 4, sub-step 3: Initialize event's type attribute to e.
    // Step 4, sub-step 4: Initialize any other IDL attributes of event as
    //                     described (cancelable).
    let event = Event::new(
        event_type.into(),
        false,
        cancelable,
        false,
        true,
        time_millis,
    );
    let event_object = create_interface_instance::<Types, Event>(event, ec)?;
    let event: Event = ec
        .with_object_any(&event_object)
        .and_then(|data| data.downcast_ref::<Event>().cloned())
        .ok_or_else(|| ec.new_type_error("event_object is not an Event"))?;

    // Step 4, sub-step 5: Return the result of dispatching event at target.
    let path = simple_path(&event_target);
    dispatch_event(ec, &path, &event)

    // Step 5: Decrease document's relevant agent's event loop's termination
    //         nesting level by 1.
    // TODO: Not yet implemented.

    // Step 6: Show beforeunload prompt to user if conditions are met.
    //         (Handled by the caller in main.rs.)

    // Step 7: Decrease document's unload counter by 1.
    // TODO: Not yet implemented.

    // Step 8: Return (unloadPromptShown, unloadPromptCanceled).
    //         (Handled by caller — caller reads the dispatch result.)
}
