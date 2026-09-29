use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::{ExecutionContext, JsTypes, gc_struct};

use super::resize_observation::ResizeObservation;
use crate::dom::{Document, Element};
use crate::js::Types;
use crate::webidl::Callback;

type JsObject = <Types as JsTypes>::JsObject;

/// <https://drafts.csswg.org/resize-observer/#enumdef-resizeobserverboxoptions>
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeObserverBoxOptions {
    BorderBox,
    ContentBox,
    DevicePixelContentBox,
}

impl ResizeObserverBoxOptions {
    pub(crate) fn from_idl(value: &str) -> Option<Self> {
        match value {
            "border-box" => Some(Self::BorderBox),
            "content-box" => Some(Self::ContentBox),
            "device-pixel-content-box" => Some(Self::DevicePixelContentBox),
            _ => None,
        }
    }
}

/// <https://drafts.csswg.org/resize-observer/#resizeobserver>
#[gc_struct]
pub struct ResizeObserver {
    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserver-callback-slot>
    pub(crate) callback: Callback,

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserver-observationtargets-slot>
    pub(crate) observation_targets: GcCell<Vec<ResizeObservation>>,

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserver-activetargets-slot>
    pub(crate) active_targets: GcCell<Vec<ResizeObservation>>,

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserver-skippedtargets-slot>
    pub(crate) skipped_targets: GcCell<Vec<ResizeObservation>>,

    // Note: the reflector is a shared cell because the document's
    // [[resizeObservers]] list holds a clone made by the constructor before
    // the wrapper exists; the reflector hook writes it after creation.
    pub(crate) reflector: GcCell<Option<JsObject>>,
}

impl ResizeObserver {
    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserver-resizeobserver>
    pub(crate) fn constructor(
        callback: Callback,
        document: &Document,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        // Step 1: "Let this be a new ResizeObserver object."
        // Step 2: "Set this internal [[callback]] slot to callback."
        // Step 3: "Set this internal [[observationTargets]] slot to an empty list."
        // Step 4: "Set this internal [[activeTargets]] slot to an empty list."
        // Step 5: "Set this internal [[skippedTargets]] slot to an empty list."
        let observer = Self {
            callback,
            observation_targets: gc_cell_new(Vec::new(), ec),
            active_targets: gc_cell_new(Vec::new(), ec),
            skipped_targets: gc_cell_new(Vec::new(), ec),
            reflector: gc_cell_new(None, ec),
        };

        // Step 6: "Add this to Document's [[resizeObservers]] slot."
        document
            .resize_observers
            .borrow_mut(ec)
            .push(observer.clone());
        observer
    }

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserver-observe>
    pub(crate) fn observe(
        &self,
        target: Element,
        observed_box: ResizeObserverBoxOptions,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // Step 1: "If target is in [[observationTargets]] slot, call unobserve() with argument target."
        let observed = self
            .observation_targets
            .borrow(ec)
            .iter()
            .any(|observation| observation.target.is_same_element(&target));
        if observed {
            self.unobserve(&target, ec);
        }

        // Step 2: "Let observedBox be the value of options.box"
        // Step 3: "Let resizeObservation be new ResizeObservation(target, observedBox)."
        let resize_observation = ResizeObservation::new(target, observed_box);

        // Step 4: "Add the resizeObservation to the [[observationTargets]] slot."
        self.observation_targets
            .borrow_mut(ec)
            .push(resize_observation);
    }

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserver-unobserve>
    pub(crate) fn unobserve(&self, target: &Element, ec: &mut dyn ExecutionContext<Types>) {
        // Step 1: "Let observation be ResizeObservation in [[observationTargets]] whose target slot is target."
        let position = self
            .observation_targets
            .borrow(ec)
            .iter()
            .position(|observation| observation.target.is_same_element(target));

        // Step 2: "If observation is not found, return."
        let Some(position) = position else {
            return;
        };

        // Step 3: "Remove observation from [[observationTargets]]."
        self.observation_targets.borrow_mut(ec).remove(position);
    }

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserver-disconnect>
    pub(crate) fn disconnect(&self, ec: &mut dyn ExecutionContext<Types>) {
        // Step 1: "Clear the [[observationTargets]] list."
        self.observation_targets.borrow_mut(ec).clear();

        // Step 2: "Clear the [[activeTargets]] list."
        self.active_targets.borrow_mut(ec).clear();
    }
}
