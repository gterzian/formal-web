use js_engine::{Completion, ExecutionContext, JsTypes};
use log::error;

use super::observer::ResizeObserverBoxOptions;
use super::resize_observer_entry::ResizeObserverEntry;
use super::resize_observer_size::Size;
use crate::dom::{Document, Element};
use crate::js::Types;
use crate::webidl::{ExceptionBehavior, invoke_callback_function};

/// <https://drafts.csswg.org/resize-observer/#gather-active-observations-at-depth>
pub(crate) fn gather_active_observations_at_depth(
    document: &Document,
    depth: usize,
    ec: &mut dyn ExecutionContext<Types>,
) {
    // Step 1: "Let depth be the depth passed in."
    // Step 2: "For each observer in [[resizeObservers]] run these steps:"
    let observers = document.resize_observers.borrow(ec).clone();
    for observer in observers {
        // Step 2.1: "Clear observer's [[activeTargets]], and [[skippedTargets]]."
        observer.active_targets.borrow_mut(ec).clear();
        observer.skipped_targets.borrow_mut(ec).clear();

        // Step 2.2: "For each observation in observer.[[observationTargets]] run this step:"
        let observations = observer.observation_targets.borrow(ec).clone();
        for observation in observations {
            // Step 2.2.1: "If observation.isActive() is true"
            if !observation.is_active() {
                continue;
            }

            // Step 2.2.1.1: "Let targetDepth be result of calculate depth for node for observation.target."
            let target_depth = calculate_depth_for_node(&observation.target);

            // Step 2.2.1.2: "If targetDepth is greater than depth then add observation to [[activeTargets]]."
            if target_depth > depth {
                observer.active_targets.borrow_mut(ec).push(observation);
            // Step 2.2.1.3: "Else add observation to [[skippedTargets]]."
            } else {
                observer.skipped_targets.borrow_mut(ec).push(observation);
            }
        }
    }
}

/// <https://drafts.csswg.org/resize-observer/#has-active-observations>
pub(crate) fn has_active_observations(
    document: &Document,
    ec: &mut dyn ExecutionContext<Types>,
) -> bool {
    // Step 1: "For each observer in [[resizeObservers]] run this step:"
    let observers = document.resize_observers.borrow(ec).clone();
    for observer in observers {
        // Step 1.1: "If observer.[[activeTargets]] is not empty, return true."
        if !observer.active_targets.borrow(ec).is_empty() {
            return true;
        }
    }

    // Step 2: "return false."
    false
}

/// <https://drafts.csswg.org/resize-observer/#has-skipped-observations>
pub(crate) fn has_skipped_observations(
    document: &Document,
    ec: &mut dyn ExecutionContext<Types>,
) -> bool {
    // Step 1: "For each observer in [[resizeObservers]] run this step:"
    let observers = document.resize_observers.borrow(ec).clone();
    for observer in observers {
        // Step 1.1: "If observer.[[skippedTargets]] is not empty, return true."
        if !observer.skipped_targets.borrow(ec).is_empty() {
            return true;
        }
    }

    // Step 2: "return false."
    false
}

/// <https://drafts.csswg.org/resize-observer/#broadcast-active-observations>
pub(crate) fn broadcast_active_observations(
    document: &Document,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<usize, Types> {
    // Step 1: "Let shallowestTargetDepth be ∞."
    let mut shallowest_target_depth = usize::MAX;

    // Step 2: "For each observer in document.[[resizeObservers]] run these steps:"
    let observers = document.resize_observers.borrow(ec).clone();
    for observer in observers {
        // Step 2.1: "If observer.[[activeTargets]] slot is empty, continue."
        let active_targets = observer.active_targets.borrow(ec).clone();
        if active_targets.is_empty() {
            continue;
        }

        // Step 2.2: "Let entries be an empty list of ResizeObserverEntryies."
        let entries = ec.create_empty_array();

        // Step 2.3: "For each observation in [[activeTargets]] perform these steps:"
        for observation in active_targets {
            // Step 2.3.1: "Let entry be the result of running create new ResizeObserverEntry with observation.target."
            let entry = ResizeObserverEntry::new(observation.target.clone(), ec)?;

            // Step 2.3.2: "Add entry to entries."
            let entry_value = entry
                .reflector
                .clone()
                .map(<Types as JsTypes>::value_from_object)
                .ok_or_else(|| ec.new_type_error("ResizeObserverEntry has no reflector"))?;
            ec.array_push(&entries, entry_value)?;

            // Step 2.3.3: "Set observation.lastReportedSizes to matching entry sizes."
            // Step 2.3.3.1: "Matching sizes are entry.borderBoxSize if observation.observedBox is "border-box""
            // Step 2.3.3.2: "Matching sizes are entry.contentBoxSize if observation.observedBox is "content-box""
            // Step 2.3.3.3: "Matching sizes are entry.devicePixelContentBoxSize if observation.observedBox is "device-pixel-content-box""
            // Note: the entry's sizes are the box size calculation for each
            // box, so the matching size is recalculated for the observed box.
            let matching_size = calculate_box_size(&observation.target, observation.observed_box);
            *observation.last_reported_sizes.borrow_mut() = vec![matching_size];

            // Step 2.3.4: "Set targetDepth to the result of calculate depth for node for observation.target."
            let target_depth = calculate_depth_for_node(&observation.target);

            // Step 2.3.5: "Set shallowestTargetDepth to targetDepth if targetDepth < shallowestTargetDepth"
            if target_depth < shallowest_target_depth {
                shallowest_target_depth = target_depth;
            }
        }

        // Step 2.4: "Invoke observer.[[callback]] with entries."
        let entries_value = <Types as JsTypes>::value_from_object(entries);
        let observer_value = observer
            .reflector
            .borrow(ec)
            .clone()
            .map(<Types as JsTypes>::value_from_object)
            .unwrap_or_else(|| ec.value_undefined());
        invoke_callback_function(
            ec,
            &observer.callback,
            &[entries_value, observer_value],
            ExceptionBehavior::Report,
            None,
        )?;

        // Step 2.5: "Clear observer.[[activeTargets]]."
        observer.active_targets.borrow_mut(ec).clear();
    }

    // Step 3: "Return shallowestTargetDepth."
    Ok(shallowest_target_depth)
}

/// <https://drafts.csswg.org/resize-observer/#deliver-the-resize-loop-error-notification>
pub(crate) fn deliver_the_resize_loop_error_notification() {
    // Step 1: "Create a new ErrorEvent."
    // Step 2: "Initialize event's message slot to "ResizeObserver loop completed with undelivered notifications.""
    // Step 3: "Report the exception event."
    // Note: ErrorEvent and "report the exception" for an event are not
    // implemented; the message is logged.
    error!("ResizeObserver loop completed with undelivered notifications.");
}

/// <https://drafts.csswg.org/resize-observer/#calculate-depth-for-node>
pub(crate) fn calculate_depth_for_node(node: &Element) -> usize {
    // Step 1: "Let p be the parent-traversal path from node to a root Element of node's flattened DOM tree."
    // Step 2: "Return number of nodes in p."
    let document = node.node.document.borrow();
    let mut depth = 0;
    let mut current = document
        .get_node(node.node.node_id)
        .and_then(|blitz_node| blitz_node.parent);
    while let Some(node_id) = current {
        depth += 1;
        current = document
            .get_node(node_id)
            .and_then(|blitz_node| blitz_node.parent);
    }
    depth
}

/// <https://drafts.csswg.org/resize-observer/#calculate-box-size>
pub(crate) fn calculate_box_size(target: &Element, observed_box: ResizeObserverBoxOptions) -> Size {
    // Step 1: "Let computedSize be a new ResizeObserverSize object."
    // Step 2: "If target is an SVGGraphicsElement that does not have an associated CSS layout box:"
    // Note: every element here has a CSS layout box, so step 2 does not
    // apply; the horizontal writing mode maps inline to width and block to
    // height.
    // Step 3: "Otherwise:"
    let border_box = target.bounding_client_rect().unwrap_or_default();
    let metrics = target.box_metrics().unwrap_or_default();
    let content_inline = (border_box.width
        - metrics.border_left
        - metrics.border_right
        - metrics.padding_left
        - metrics.padding_right)
        .max(0.0);
    let content_block = (border_box.height
        - metrics.border_top
        - metrics.border_bottom
        - metrics.padding_top
        - metrics.padding_bottom)
        .max(0.0);
    match observed_box {
        // Step 3.1: "If observedBox is "border-box""
        // Step 3.1.1: "Set computedSize's inlineSize to target's border area inline length."
        // Step 3.1.2: "Set computedSize's blockSize to target's border area block length."
        ResizeObserverBoxOptions::BorderBox => Size {
            inline_size: border_box.width,
            block_size: border_box.height,
        },
        // Step 3.2: "If observedBox is "content-box""
        // Step 3.2.1: "Set computedSize's inlineSize to target's content area inline length."
        // Step 3.2.2: "Set computedSize's blockSize to target's content area block length."
        ResizeObserverBoxOptions::ContentBox => Size {
            inline_size: content_inline,
            block_size: content_block,
        },
        // Step 3.3: "If observedBox is "device-pixel-content-box""
        // Step 3.3.1: "Set computedSize's inlineSize to target's content area inline length, in integral device pixels."
        // Step 3.3.2: "Set computedSize's blockSize to target's content area block length, in integral device pixels."
        ResizeObserverBoxOptions::DevicePixelContentBox => {
            let scale = target.node.document.borrow().viewport().scale_f64();
            Size {
                inline_size: (content_inline * scale).round(),
                block_size: (content_block * scale).round(),
            }
        }
    }
    // Step 4: "Return computedSize."
}
