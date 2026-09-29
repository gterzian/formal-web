use std::cell::RefCell;
use std::rc::Rc;

use js_engine::gc_struct;

use super::observer::ResizeObserverBoxOptions;
use super::processing_model::calculate_box_size;
use super::resize_observer_size::Size;
use crate::dom::Element;

/// <https://drafts.csswg.org/resize-observer/#resizeobservation>
#[gc_struct]
pub(crate) struct ResizeObservation {
    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobservation-target>
    pub(crate) target: Element,

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobservation-observedbox>
    #[ignore_trace]
    pub(crate) observed_box: ResizeObserverBoxOptions,

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobservation-lastreportedsizes>
    #[ignore_trace]
    pub(crate) last_reported_sizes: Rc<RefCell<Vec<Size>>>,
}

impl ResizeObservation {
    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobservation-resizeobservation>
    pub(crate) fn new(target: Element, observed_box: ResizeObserverBoxOptions) -> Self {
        // Step 1: "Let this be a new ResizeObservation object."
        // Step 2: "Set this internal [[target]] slot to target."
        // Step 3: "Set this internal [[observedBox]] slot to observedBox."
        // Step 4: "Set this internal [[lastReportedSizes]] slot to [(0,0)]."
        Self {
            target,
            observed_box,
            last_reported_sizes: Rc::new(RefCell::new(vec![Size {
                inline_size: 0.0,
                block_size: 0.0,
            }])),
        }
    }

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobservation-isactive>
    pub(crate) fn is_active(&self) -> bool {
        // Step 1: "Set currentSize by calculate box size given target and observedBox."
        let current_size = calculate_box_size(&self.target, self.observed_box);

        // Step 2: "If currentSize is not equal to this.lastReportedSizes[0] return true."
        if self.last_reported_sizes.borrow().first() != Some(&current_size) {
            return true;
        }

        // Step 3: "Return false."
        false
    }
}
