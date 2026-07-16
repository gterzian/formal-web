use std::{mem, ptr};

use crate::js::Types;
use crate::streams::PipeToState;
use crate::webidl::Callback;
use crate::webidl::bindings::create_interface_instance;
use js_engine::gc::{GcCell, gc_cell_new, gc_cell_ptr_eq};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::{DOMException, EventTarget, EventTargetAccess, fire_event};

type JsObject = <Types as JsTypes>::JsObject;
type JsValue = <Types as JsTypes>::JsValue;

/// <https://dom.spec.whatwg.org/#abortsignal-add>
#[gc_struct]
pub(crate) enum AbortAlgorithm {
    // Note: Not yet wired to any JS binding; kept for spec completeness.
    #[allow(dead_code)]
    Native {
        #[ignore_trace]
        callback: fn() -> Completion<(), Types>,
    },

    RemoveEventListener {
        event_target: EventTarget,

        #[ignore_trace]
        listener_id: u64,
    },

    ReadableStreamPipeTo {
        state: PipeToState,
    },
}

impl AbortAlgorithm {
    /// <https://dom.spec.whatwg.org/#abortsignal-add>
    pub(crate) fn run(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        match self {
            Self::Native { callback } => {
                if callback().is_err() {
                    let err = ec.new_type_error("abort algorithm native callback failed");
                    return Err(err);
                }
            }
            Self::RemoveEventListener {
                event_target,
                listener_id,
            } => {
                event_target.remove_event_listener_by_id(*listener_id);
            }
            Self::ReadableStreamPipeTo { state } => {
                state.run_abort_algorithm(ec)?;
            }
        }

        Ok(())
    }

    pub(crate) fn matches_entry(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Native { callback: left }, Self::Native { callback: right }) => {
                ptr::fn_addr_eq(*left, *right)
            }
            (
                Self::RemoveEventListener {
                    event_target: _,
                    listener_id: left_id,
                },
                Self::RemoveEventListener {
                    event_target: _,
                    listener_id: right_id,
                },
            ) => left_id == right_id,
            (
                Self::ReadableStreamPipeTo { state: left_state },
                Self::ReadableStreamPipeTo { state: right_state },
            ) => left_state.ptr_eq(right_state),
            _ => false,
        }
    }
}

#[gc_struct]
struct AbortSignalState {
    reflector: Option<JsObject>,
    event_target: EventTarget,

    #[ignore_trace]
    aborted: bool,

    abort_reason: JsValue,
    abort_algorithms: Vec<AbortAlgorithm>,

    #[ignore_trace]
    dependent: bool,

    source_signals: Vec<AbortSignal>,
    dependent_signals: Vec<AbortSignal>,

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-onabort>
    onabort: Option<Callback>,
}

impl AbortSignalState {
    fn new(aborted: bool, abort_reason: JsValue) -> Self {
        Self {
            reflector: None,
            event_target: EventTarget::default(),
            aborted,
            abort_reason,
            abort_algorithms: Vec::new(),
            dependent: false,
            source_signals: Vec::new(),
            dependent_signals: Vec::new(),
            onabort: None,
        }
    }
}

/// <https://dom.spec.whatwg.org/#abortsignal>
#[gc_struct]
pub struct AbortSignal {
    shared: GcCell<AbortSignalState>,
}

impl EventTargetAccess for AbortSignal {
    fn get_event_target(&self) -> EventTarget {
        self.shared.borrow().event_target.clone()
    }

    fn get_target_object(&self) -> Option<JsObject> {
        self.object()
    }
}

impl AbortSignal {
    pub(crate) fn new(ec: &mut dyn ExecutionContext<Types>) -> Self {
        Self {
            shared: gc_cell_new(AbortSignalState::new(false, ec.value_undefined())),
        }
    }

    pub(crate) fn aborted_with_reason(reason: JsValue) -> Self {
        Self {
            shared: gc_cell_new(AbortSignalState::new(true, reason)),
        }
    }

    pub(crate) fn set_reflector(&self, reflector: JsObject) {
        self.shared.borrow_mut().reflector = Some(reflector);
    }

    pub(crate) fn object(&self) -> Option<JsObject> {
        self.shared.borrow().reflector.clone()
    }

    pub(crate) fn with_event_target_mut<R>(&self, f: impl FnOnce(&mut EventTarget) -> R) -> R {
        let mut state = self.shared.borrow_mut();
        f(&mut state.event_target)
    }

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-aborted>
    pub(crate) fn aborted_value(&self) -> bool {
        self.shared.borrow().aborted
    }

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-reason>
    pub(crate) fn reason_value(&self) -> JsValue {
        self.shared.borrow().abort_reason.clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-onabort>
    pub(crate) fn onabort_value(&self) -> Option<Callback> {
        self.shared.borrow().onabort.clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-onabort>
    pub(crate) fn replace_onabort(&self, callback: Option<Callback>) -> Option<Callback> {
        let mut state = self.shared.borrow_mut();
        mem::replace(&mut state.onabort, callback)
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-add>
    pub(crate) fn add_abort_algorithm(&self, algorithm: AbortAlgorithm) {
        self.shared.borrow_mut().abort_algorithms.push(algorithm);
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-remove>
    pub(crate) fn remove_abort_algorithm(&self, algorithm: &AbortAlgorithm) {
        self.shared
            .borrow_mut()
            .abort_algorithms
            .retain(|candidate| !candidate.matches_entry(algorithm));
    }

    // Note: Not yet wired to any JS binding; kept for spec completeness.
    #[allow(dead_code)]
    pub(crate) fn add_native_abort_algorithm(&self, callback: fn() -> Completion<(), Types>) {
        self.add_abort_algorithm(AbortAlgorithm::Native { callback });
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-signal-abort>
    pub(crate) fn begin_abort(&self, reason: JsValue) -> Option<Vec<AbortSignal>> {
        let dependent_signals = {
            let mut state = self.shared.borrow_mut();
            if state.aborted {
                return None;
            }

            // Step 2: "Set signal's abort reason to reason if it is given; otherwise to a new
            // \"AbortError\" DOMException."
            state.aborted = true;
            state.abort_reason = reason.clone();

            // Step 3: "Let dependentSignalsToAbort be a new list."
            state.dependent_signals.clone()
        };

        // Step 4: "For each dependentSignal of signal's dependent signals:"
        let dependent_signals_to_abort = dependent_signals
            .into_iter()
            .filter_map(|dependent_signal| {
                if dependent_signal.begin_dependent_abort(reason.clone()) {
                    Some(dependent_signal)
                } else {
                    None
                }
            })
            .collect();

        Some(dependent_signals_to_abort)
    }

    fn begin_dependent_abort(&self, reason: JsValue) -> bool {
        let mut state = self.shared.borrow_mut();
        if state.aborted {
            return false;
        }

        state.aborted = true;
        state.abort_reason = reason;
        true
    }

    /// <https://dom.spec.whatwg.org/#run-the-abort-steps>
    pub(crate) fn take_abort_algorithms(&self) -> Vec<AbortAlgorithm> {
        let mut state = self.shared.borrow_mut();
        mem::take(&mut state.abort_algorithms)
    }

    /// <https://dom.spec.whatwg.org/#create-a-dependent-abort-signal>
    pub(crate) fn set_dependent(&self, dependent: bool) {
        self.shared.borrow_mut().dependent = dependent;
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-dependent>
    pub(crate) fn dependent_value(&self) -> bool {
        self.shared.borrow().dependent
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-source-signals>
    pub(crate) fn source_signals_value(&self) -> Vec<AbortSignal> {
        self.shared.borrow().source_signals.clone()
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-source-signals>
    pub(crate) fn append_source_signal(&self, signal: &AbortSignal) {
        let mut state = self.shared.borrow_mut();
        append_unique_signal(&mut state.source_signals, signal);
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-dependent-signals>
    pub(crate) fn append_dependent_signal(&self, signal: &AbortSignal) {
        let mut state = self.shared.borrow_mut();
        append_unique_signal(&mut state.dependent_signals, signal);
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-abort-reason>
    pub(crate) fn set_aborted_reason(&self, reason: JsValue) {
        let mut state = self.shared.borrow_mut();
        state.aborted = true;
        state.abort_reason = reason;
    }
}

/// <https://dom.spec.whatwg.org/#abortcontroller>
#[gc_struct]
pub struct AbortController {
    /// <https://dom.spec.whatwg.org/#abortcontroller-signal>
    signal: AbortSignal,
}

impl AbortController {
    pub(crate) fn new(signal: AbortSignal) -> Self {
        Self { signal }
    }

    /// <https://dom.spec.whatwg.org/#dom-abortcontroller-signal>
    pub(crate) fn signal(&self) -> AbortSignal {
        self.signal.clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-abortcontroller-signal>
    pub(crate) fn signal_object(&self) -> Option<JsObject> {
        self.signal.object()
    }
}

/// <https://dom.spec.whatwg.org/#abortsignal>
pub(crate) fn create_abort_signal(
    signal: AbortSignal,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<AbortSignal, Types> {
    let signal_object = create_interface_instance::<Types, AbortSignal>(signal.clone(), ec)?;
    signal.set_reflector(signal_object);
    Ok(signal)
}

/// <https://dom.spec.whatwg.org/#abortsignal-signal-abort>
pub(crate) fn signal_abort(
    ec: &mut dyn ExecutionContext<Types>,
    signal: &AbortSignal,
    reason: JsValue,
) -> Completion<(), Types> {
    let reason = if <Types as JsTypes>::value_is_undefined(&reason) {
        <Types as JsTypes>::value_from_object(create_interface_instance::<Types, DOMException>(
            DOMException::abort_error(),
            ec,
        )?)
    } else {
        reason
    };

    let Some(dependent_signals_to_abort) = signal.begin_abort(reason) else {
        return Ok(());
    };

    run_abort_steps(ec, signal)?;

    for dependent_signal in dependent_signals_to_abort {
        run_abort_steps(ec, &dependent_signal)?;
    }

    Ok(())
}

fn run_abort_steps(
    ec: &mut dyn ExecutionContext<Types>,
    signal: &AbortSignal,
) -> Completion<(), Types> {
    let algorithms = signal.take_abort_algorithms();
    for algorithm in algorithms {
        algorithm.run(ec)?;
    }

    let event_target_clone = signal.get_event_target();
    let signal_object = signal.object().ok_or_else(|| {
        ec.new_type_error("AbortSignal is missing its JavaScript object")
    })?;

    fire_event(ec, &event_target_clone, &signal_object, "abort", 0.0, false)?;
    Ok(())
}

/// <https://dom.spec.whatwg.org/#create-a-dependent-abort-signal>
pub(crate) fn initialize_dependent_abort_signal(
    result_signal: &AbortSignal,
    signals: &[AbortSignal],
) {
    // Step 2: "For each signal of signals: if signal is aborted, then set resultSignal's abort reason to signal's abort reason and return resultSignal."
    for signal in signals {
        if signal.aborted_value() {
            result_signal.set_aborted_reason(signal.reason_value());
            return;
        }
    }

    // Step 3: "Set resultSignal's dependent to true."
    result_signal.set_dependent(true);

    // Step 4: "For each signal of signals:"
    for signal in signals {
        // Step 4.1: "If signal's dependent is false:"
        if !signal.dependent_value() {
            // Step 4.1.1: "Append signal to resultSignal's source signals."
            result_signal.append_source_signal(signal);

            // Step 4.1.2: "Append resultSignal to signal's dependent signals."
            signal.append_dependent_signal(result_signal);
            continue;
        }

        // Step 4.2: "Otherwise, for each sourceSignal of signal's source signals:"
        for source_signal in signal.source_signals_value() {
            // Step 4.2.2: "Append sourceSignal to resultSignal's source signals."
            result_signal.append_source_signal(&source_signal);

            // Step 4.2.3: "Append resultSignal to sourceSignal's dependent signals."
            source_signal.append_dependent_signal(result_signal);
        }
    }
}

fn append_unique_signal(signals: &mut Vec<AbortSignal>, signal: &AbortSignal) {
    if signals
        .iter()
        .any(|existing| gc_cell_ptr_eq(&existing.shared, &signal.shared))
    {
        return;
    }

    signals.push(signal.clone());
}
