use std::{mem, ptr};

use crate::fetch::abort_fetch;
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
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

    /// The abort steps the fetch() method adds to its request's signal
    /// (step 11): they abort the pending fetch with this id.
    Fetch {
        #[ignore_trace]
        fetch_id: u64,
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
                event_target.remove_event_listener_by_id(*listener_id, ec);
            }
            Self::ReadableStreamPipeTo { state } => {
                state.run_abort_algorithm(ec)?;
            }
            Self::Fetch { fetch_id } => {
                // The fetch() method's step 11.4: abort the fetch() call with
                // p, request, responseObject, and requestObject's signal's
                // abort reason.
                let fetch_id = *fetch_id;
                with_global_scope(ec, |global_scope, ec| {
                    let Some(pending) = global_scope.take_fetch(fetch_id, ec) else {
                        return Ok(());
                    };
                    let reason = pending.request_object.signal().reason_value(ec);
                    abort_fetch(pending, reason, ec)
                })?;
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
            (Self::Fetch { fetch_id: left }, Self::Fetch { fetch_id: right }) => left == right,
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
    fn new(aborted: bool, abort_reason: JsValue, ec: &mut dyn ExecutionContext<Types>) -> Self {
        Self {
            reflector: None,
            event_target: EventTarget::new(ec),
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
    fn get_event_target(&self, ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.shared.borrow(ec).event_target.clone()
    }
}

impl AbortSignal {
    pub(crate) fn new(ec: &mut dyn ExecutionContext<Types>) -> Self {
        Self {
            shared: gc_cell_new(AbortSignalState::new(false, ec.value_undefined(), ec), ec),
        }
    }

    pub(crate) fn aborted_with_reason(
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            shared: gc_cell_new(AbortSignalState::new(true, reason, ec), ec),
        }
    }

    pub(crate) fn set_reflector(&self, reflector: JsObject, ec: &mut dyn ExecutionContext<Types>) {
        // `store_js_object` allocates (it converts the stored handle into a
        // cppgc edge), so the reflector is stored into a local slot first and
        // written into the cell under a borrow held only for the assignment —
        // no engine call runs while the cell is borrowed.
        let mut reflector_slot = self.shared.borrow(ec).reflector.clone();
        ec.store_js_object(&mut reflector_slot, reflector);
        self.shared.borrow_mut(ec).reflector = reflector_slot;
    }

    pub(crate) fn object(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<JsObject> {
        self.shared.borrow(ec).reflector.clone()
    }

    pub(crate) fn with_event_target_mut<R>(
        &self,
        f: impl FnOnce(&mut EventTarget, &mut dyn ExecutionContext<Types>) -> R,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> R {
        // The closure may call the engine (listener registration, reflector
        // storage — both allocate), so the event target is cloned out of the
        // cell and no borrow is held across the closure; the result is
        // written back under a borrow held only for the assignment.
        let mut event_target = self.shared.borrow(ec).event_target.clone();
        let result = f(&mut event_target, ec);
        self.shared.borrow_mut(ec).event_target = event_target;
        result
    }

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-aborted>
    pub(crate) fn aborted_value(&self, ec: &mut dyn ExecutionContext<Types>) -> bool {
        self.shared.borrow(ec).aborted
    }

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-reason>
    pub(crate) fn reason_value(&self, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
        self.shared.borrow(ec).abort_reason.clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-onabort>
    pub(crate) fn onabort_value(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<Callback> {
        self.shared.borrow(ec).onabort.clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-onabort>
    pub(crate) fn replace_onabort(
        &self,
        callback: Option<Callback>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<Callback> {
        let mut state = self.shared.borrow_mut(ec);
        mem::replace(&mut state.onabort, callback)
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-add>
    pub(crate) fn add_abort_algorithm(
        &self,
        algorithm: AbortAlgorithm,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        self.shared.borrow_mut(ec).abort_algorithms.push(algorithm);
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-remove>
    pub(crate) fn remove_abort_algorithm(
        &self,
        algorithm: &AbortAlgorithm,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        self.shared
            .borrow_mut(ec)
            .abort_algorithms
            .retain(|candidate| !candidate.matches_entry(algorithm));
    }

    // Note: Not yet wired to any JS binding; kept for spec completeness.
    #[allow(dead_code)]
    pub(crate) fn add_native_abort_algorithm(
        &self,
        callback: fn() -> Completion<(), Types>,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        self.add_abort_algorithm(AbortAlgorithm::Native { callback }, ec);
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-signal-abort>
    pub(crate) fn begin_abort(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<Vec<AbortSignal>> {
        let dependent_signals = {
            let mut state = self.shared.borrow_mut(ec);
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
                if dependent_signal.begin_dependent_abort(reason.clone(), ec) {
                    Some(dependent_signal)
                } else {
                    None
                }
            })
            .collect();

        Some(dependent_signals_to_abort)
    }

    fn begin_dependent_abort(&self, reason: JsValue, ec: &mut dyn ExecutionContext<Types>) -> bool {
        let mut state = self.shared.borrow_mut(ec);
        if state.aborted {
            return false;
        }

        state.aborted = true;
        state.abort_reason = reason;
        true
    }

    /// <https://dom.spec.whatwg.org/#run-the-abort-steps>
    pub(crate) fn take_abort_algorithms(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<AbortAlgorithm> {
        let mut state = self.shared.borrow_mut(ec);
        mem::take(&mut state.abort_algorithms)
    }

    /// <https://dom.spec.whatwg.org/#create-a-dependent-abort-signal>
    pub(crate) fn set_dependent(&self, dependent: bool, ec: &mut dyn ExecutionContext<Types>) {
        self.shared.borrow_mut(ec).dependent = dependent;
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-dependent>
    pub(crate) fn dependent_value(&self, ec: &mut dyn ExecutionContext<Types>) -> bool {
        self.shared.borrow(ec).dependent
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-source-signals>
    pub(crate) fn source_signals_value(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<AbortSignal> {
        self.shared.borrow(ec).source_signals.clone()
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-source-signals>
    pub(crate) fn append_source_signal(
        &self,
        signal: &AbortSignal,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let mut state = self.shared.borrow_mut(ec);
        append_unique_signal(&mut state.source_signals, signal);
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-dependent-signals>
    pub(crate) fn append_dependent_signal(
        &self,
        signal: &AbortSignal,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let mut state = self.shared.borrow_mut(ec);
        append_unique_signal(&mut state.dependent_signals, signal);
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-abort-reason>
    pub(crate) fn set_aborted_reason(&self, reason: JsValue, ec: &mut dyn ExecutionContext<Types>) {
        let mut state = self.shared.borrow_mut(ec);
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
    pub(crate) fn signal_object(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<JsObject> {
        self.signal.object(ec)
    }
}

/// <https://dom.spec.whatwg.org/#abortsignal>
pub(crate) fn create_abort_signal(
    signal: AbortSignal,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<AbortSignal, Types> {
    let signal_object = create_interface_instance::<Types, AbortSignal>(signal.clone(), ec)?;
    signal.set_reflector(signal_object, ec);
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

    let Some(dependent_signals_to_abort) = signal.begin_abort(reason, ec) else {
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
    let algorithms = signal.take_abort_algorithms(ec);
    for algorithm in algorithms {
        algorithm.run(ec)?;
    }

    fire_event(ec, signal, "abort", 0.0, false)?;
    Ok(())
}

/// <https://dom.spec.whatwg.org/#create-a-dependent-abort-signal>
pub(crate) fn initialize_dependent_abort_signal(
    result_signal: &AbortSignal,
    signals: &[AbortSignal],
    ec: &mut dyn ExecutionContext<Types>,
) {
    // Step 2: "For each signal of signals: if signal is aborted, then set resultSignal's abort reason to signal's abort reason and return resultSignal."
    for signal in signals {
        if signal.aborted_value(ec) {
            result_signal.set_aborted_reason(signal.reason_value(ec), ec);
            return;
        }
    }

    // Step 3: "Set resultSignal's dependent to true."
    result_signal.set_dependent(true, ec);

    // Step 4: "For each signal of signals:"
    for signal in signals {
        // Step 4.1: "If signal's dependent is false:"
        if !signal.dependent_value(ec) {
            // Step 4.1.1: "Append signal to resultSignal's source signals."
            result_signal.append_source_signal(signal, ec);

            // Step 4.1.2: "Append resultSignal to signal's dependent signals."
            signal.append_dependent_signal(result_signal, ec);
            continue;
        }

        // Step 4.2: "Otherwise, for each sourceSignal of signal's source signals:"
        for source_signal in signal.source_signals_value(ec) {
            // Step 4.2.2: "Append sourceSignal to resultSignal's source signals."
            result_signal.append_source_signal(&source_signal, ec);

            // Step 4.2.3: "Append resultSignal to sourceSignal's dependent signals."
            source_signal.append_dependent_signal(result_signal, ec);
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
