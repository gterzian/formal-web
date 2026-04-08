use boa_engine::{Context, JsResult, JsValue, object::JsObject};

use crate::boa::{JsExecutionContext, platform_objects};
use crate::webidl::{ExceptionBehavior, invoke_callback_function};

/// <https://html.spec.whatwg.org/#dom-animationframeprovider-requestanimationframe>
pub(crate) fn request_animation_frame(context: &Context, callback: JsObject) -> JsResult<u32> {
    // Step 1: "If this is not supported, then throw a \"NotSupportedError\" DOMException."
    // Note: The content runtime exposes `AnimationFrameProvider` only on active `Window` globals, so the provider is always supported when this helper is reached.

    // Step 2: "Let target be this's target object."
    // Note: The active `Window`'s associated `Document` is modeled by the environment-local animation frame callback state in `RuntimeData`.

    // Step 3: "Increment target's animation frame callback identifier by one, and let handle be the result."
    let handle = platform_objects::next_animation_frame_callback_handle(context)?;

    // Step 4: "Let callbacks be target's map of animation frame callbacks."
    // Step 5: "Set callbacks[handle] to callback."
    platform_objects::store_animation_frame_callback(context, handle, callback)?;

    // Step 6: "Return handle."
    Ok(handle)
}

/// <https://html.spec.whatwg.org/#animationframeprovider-cancelanimationframe>
pub(crate) fn cancel_animation_frame(context: &Context, handle: u32) -> JsResult<()> {
    // Step 1: "If this is not supported, then throw a \"NotSupportedError\" DOMException."
    // Note: The content runtime exposes `AnimationFrameProvider` only on active `Window` globals, so the provider is always supported when this helper is reached.

    // Step 2: "Let callbacks be this's target object's map of animation frame callbacks."

    // Step 3: "Remove callbacks[handle]."
    platform_objects::remove_animation_frame_callback(context, handle)
}

/// <https://html.spec.whatwg.org/#run-the-animation-frame-callbacks>
pub(crate) fn run_animation_frame_callbacks(
    execution_context: &mut JsExecutionContext,
    now: f64,
) -> Result<(), String> {
    // Step 1: "Let callbacks be target's map of animation frame callbacks."

    // Step 2: "Let callbackHandles be the result of getting the keys of callbacks."
    let callback_handles =
        platform_objects::animation_frame_callback_handles(&execution_context.context)
            .map_err(|error| error.to_string())?;

    // Step 3: "For each handle in callbackHandles, if handle exists in callbacks:"
    for handle in callback_handles {
        let Some(callback) =
            platform_objects::take_animation_frame_callback(&execution_context.context, handle)
                .map_err(|error| error.to_string())?
        else {
            continue;
        };

        // Step 3.3: "Invoke callback with « now » and \"report\"."
        invoke_callback_function(
            execution_context,
            &callback,
            &[JsValue::from(now)],
            ExceptionBehavior::Report,
            None,
        )
        .map_err(|error| error.to_string())?;
    }

    Ok(())
}
