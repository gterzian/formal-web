use std::mem;

use boa_engine::{JsData, JsNativeError, JsResult, JsValue, object::JsObject};
use boa_gc::{Finalize, Trace};

use super::{EventDispatchHost, EventTarget, fire_event, with_event_target_mut};

/// <https://dom.spec.whatwg.org/#abortsignal-add>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum AbortAlgorithm {
    RemoveEventListener {
        /// <https://dom.spec.whatwg.org/#eventtarget>
        event_target: JsObject,

        #[unsafe_ignore_trace]
        listener_id: u64,
    },
}

impl AbortAlgorithm {
    /// <https://dom.spec.whatwg.org/#abortsignal-add>
    pub(crate) fn run(&self) -> JsResult<()> {
        match self {
            Self::RemoveEventListener {
                event_target,
                listener_id,
            } => {
                with_event_target_mut(&JsValue::from(event_target.clone()), |target| {
                    target.remove_event_listener_by_id(*listener_id);
                })?;
            }
        }

        Ok(())
    }
}

/// <https://dom.spec.whatwg.org/#abortcontroller>
#[derive(Trace, Finalize, JsData)]
pub struct AbortController {
    /// <https://dom.spec.whatwg.org/#abortcontroller-signal>
    signal: JsObject,
}

impl AbortController {
    pub(crate) fn new(signal: JsObject) -> Self {
        Self { signal }
    }

    /// <https://dom.spec.whatwg.org/#dom-abortcontroller-signal>
    pub(crate) fn signal_value(&self) -> JsObject {
        self.signal.clone()
    }
}

/// <https://dom.spec.whatwg.org/#abortsignal>
#[derive(Trace, Finalize, JsData)]
pub struct AbortSignal {
    /// <https://dom.spec.whatwg.org/#eventtarget>
    pub event_target: EventTarget,

    /// <https://dom.spec.whatwg.org/#abortsignal-aborted>
    #[unsafe_ignore_trace]
    aborted: bool,

    /// <https://dom.spec.whatwg.org/#abortsignal-abort-reason>
    abort_reason: JsValue,

    /// <https://dom.spec.whatwg.org/#abortsignal-abort-algorithms>
    abort_algorithms: Vec<AbortAlgorithm>,

    /// <https://dom.spec.whatwg.org/#abortsignal-dependent>
    #[unsafe_ignore_trace]
    dependent: bool,

    /// <https://dom.spec.whatwg.org/#abortsignal-source-signals>
    source_signals: Vec<JsObject>,

    /// <https://dom.spec.whatwg.org/#abortsignal-dependent-signals>
    dependent_signals: Vec<JsObject>,

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-onabort>
    onabort: Option<JsObject>,
}

impl AbortSignal {
    pub(crate) fn new() -> Self {
        Self {
            event_target: EventTarget::default(),
            aborted: false,
            abort_reason: JsValue::undefined(),
            abort_algorithms: Vec::new(),
            dependent: false,
            source_signals: Vec::new(),
            dependent_signals: Vec::new(),
            onabort: None,
        }
    }

    pub(crate) fn aborted_with_reason(reason: JsValue) -> Self {
        Self {
            event_target: EventTarget::default(),
            aborted: true,
            abort_reason: reason,
            abort_algorithms: Vec::new(),
            dependent: false,
            source_signals: Vec::new(),
            dependent_signals: Vec::new(),
            onabort: None,
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-aborted>
    pub(crate) fn aborted_value(&self) -> bool {
        self.aborted
    }

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-reason>
    pub(crate) fn reason_value(&self) -> JsValue {
        self.abort_reason.clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-onabort>
    pub(crate) fn onabort_value(&self) -> Option<JsObject> {
        self.onabort.clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-abortsignal-onabort>
    pub(crate) fn replace_onabort(&mut self, callback: Option<JsObject>) -> Option<JsObject> {
        mem::replace(&mut self.onabort, callback)
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-add>
    pub(crate) fn add_abort_algorithm(&mut self, algorithm: AbortAlgorithm) {
        self.abort_algorithms.push(algorithm);
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-signal-abort>
    /// Note: This helper continues the signal-abort algorithm through populating `dependentSignalsToAbort`.
    pub(crate) fn begin_abort(&mut self, reason: JsValue) -> JsResult<Option<Vec<JsObject>>> {
        // Step 1: "If signal is aborted, then return."
        if self.aborted {
            return Ok(None);
        }

        // Step 2: "Set signal's abort reason to reason if it is given; otherwise to a new \"AbortError\" DOMException."
        // Note: The caller normalizes the omitted-reason case before invoking this helper.
        // Note: The content runtime stores the aborted state explicitly, so setting the final abort reason also flips `aborted`.
        self.aborted = true;
        self.abort_reason = reason.clone();

        // Step 3: "Let dependentSignalsToAbort be a new list."
        let mut dependent_signals_to_abort = Vec::new();

        // Step 4: "For each dependentSignal of signal's dependent signals:"
        for dependent_signal in self.dependent_signals.clone() {
            // Step 4.1: "If dependentSignal is not aborted:"
            let should_abort = with_abort_signal_mut(&JsValue::from(dependent_signal.clone()), |signal| {
                if signal.aborted {
                    return false;
                }

                // Step 4.1.1: "Set dependentSignal's abort reason to signal's abort reason."
                // Note: The content runtime stores the aborted state explicitly, so this is also where the dependent becomes aborted.
                signal.aborted = true;
                signal.abort_reason = reason.clone();
                true
            })?;

            if should_abort {
                // Step 4.1.2: "Append dependentSignal to dependentSignalsToAbort."
                dependent_signals_to_abort.push(dependent_signal);
            }
        }

        Ok(Some(dependent_signals_to_abort))
    }

    /// <https://dom.spec.whatwg.org/#run-the-abort-steps>
    pub(crate) fn take_abort_algorithms(&mut self) -> Vec<AbortAlgorithm> {
        mem::take(&mut self.abort_algorithms)
    }

    /// <https://dom.spec.whatwg.org/#create-a-dependent-abort-signal>
    pub(crate) fn set_dependent(&mut self, dependent: bool) {
        self.dependent = dependent;
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-dependent>
    pub(crate) fn dependent_value(&self) -> bool {
        self.dependent
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-source-signals>
    pub(crate) fn source_signals_value(&self) -> Vec<JsObject> {
        self.source_signals.clone()
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-source-signals>
    pub(crate) fn append_source_signal(&mut self, signal: &JsObject) {
        append_unique_signal(&mut self.source_signals, signal);
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-dependent-signals>
    pub(crate) fn append_dependent_signal(&mut self, signal: &JsObject) {
        append_unique_signal(&mut self.dependent_signals, signal);
    }

    /// <https://dom.spec.whatwg.org/#abortsignal-abort-reason>
    pub(crate) fn set_aborted_reason(&mut self, reason: JsValue) {
        self.aborted = true;
        self.abort_reason = reason;
    }
}

/// <https://dom.spec.whatwg.org/#abortsignal-signal-abort>
pub(crate) fn signal_abort(
    host: &mut impl EventDispatchHost,
    signal: &JsObject,
    reason: JsValue,
) -> JsResult<()> {
    // Step 1: "If signal is aborted, then return."
    let Some(dependent_signals_to_abort) =
        with_abort_signal_mut(&JsValue::from(signal.clone()), |signal| signal.begin_abort(reason))??
    else {
        return Ok(());
    };

    // Steps 2-4 are implemented by `AbortSignal::begin_abort`.

    // Step 5: "Run the abort steps for signal."
    run_abort_steps(host, signal)?;

    // Step 6: "For each dependentSignal of dependentSignalsToAbort, run the abort steps for dependentSignal."
    for dependent_signal in dependent_signals_to_abort {
        run_abort_steps(host, &dependent_signal)?;
    }

    Ok(())
}

/// <https://dom.spec.whatwg.org/#run-the-abort-steps>
pub(crate) fn run_abort_steps(
    host: &mut impl EventDispatchHost,
    signal: &JsObject,
) -> JsResult<()> {
    // Step 1: "For each algorithm of signal's abort algorithms: run algorithm."
    let algorithms = with_abort_signal_mut(&JsValue::from(signal.clone()), |signal| {
        signal.take_abort_algorithms()
    })?;
    for algorithm in algorithms {
        algorithm.run()?;
    }

    // Step 2: "Empty signal's abort algorithms."
    // Note: `take_abort_algorithms()` empties the list before the loop above runs.

    // Step 3: "Fire an event named abort at signal."
    let _ = fire_event(host, signal, "abort", false)?;
    Ok(())
}

/// <https://dom.spec.whatwg.org/#create-a-dependent-abort-signal>
pub(crate) fn initialize_dependent_abort_signal(
    result_signal: &JsObject,
    signals: &[JsObject],
) -> JsResult<()> {
    // Step 2: "For each signal of signals: if signal is aborted, then set resultSignal's abort reason to signal's abort reason and return resultSignal."
    for signal in signals {
        if with_abort_signal_ref(signal, |signal| signal.aborted_value())? {
            let reason = with_abort_signal_ref(signal, |signal| signal.reason_value())?;
            with_abort_signal_mut(&JsValue::from(result_signal.clone()), |signal| {
                signal.set_aborted_reason(reason);
            })?;
            return Ok(());
        }
    }

    // Step 3: "Set resultSignal's dependent to true."
    with_abort_signal_mut(&JsValue::from(result_signal.clone()), |signal| {
        signal.set_dependent(true);
    })?;

    // Step 4: "For each signal of signals:"
    for signal in signals {
        // Step 4.1: "If signal's dependent is false:"
        if !with_abort_signal_ref(signal, |signal| signal.dependent_value())? {
            // Step 4.1.1: "Append signal to resultSignal's source signals."
            with_abort_signal_mut(&JsValue::from(result_signal.clone()), |result| {
                result.append_source_signal(signal);
            })?;

            // Step 4.1.2: "Append resultSignal to signal's dependent signals."
            with_abort_signal_mut(&JsValue::from(signal.clone()), |source| {
                source.append_dependent_signal(result_signal);
            })?;
            continue;
        }

        // Step 4.2: "Otherwise, for each sourceSignal of signal's source signals:"
        let source_signals = with_abort_signal_ref(signal, |signal| signal.source_signals_value())?;
        for source_signal in source_signals {
            // Step 4.2.2: "Append sourceSignal to resultSignal's source signals."
            with_abort_signal_mut(&JsValue::from(result_signal.clone()), |result| {
                result.append_source_signal(&source_signal);
            })?;

            // Step 4.2.3: "Append resultSignal to sourceSignal's dependent signals."
            with_abort_signal_mut(&JsValue::from(source_signal.clone()), |source| {
                source.append_dependent_signal(result_signal);
            })?;
        }
    }

    Ok(())
}

fn append_unique_signal(signals: &mut Vec<JsObject>, signal: &JsObject) {
    if signals
        .iter()
        .any(|existing| JsObject::equals(existing, signal))
    {
        return;
    }

    signals.push(signal.clone());
}

pub(crate) fn with_abort_controller_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&AbortController) -> R,
) -> JsResult<R> {
    let controller = object.downcast_ref::<AbortController>().ok_or_else(|| {
        JsNativeError::typ().with_message("object is not an AbortController")
    })?;
    Ok(f(&controller))
}

pub(crate) fn with_abort_signal_mut<R>(
    this: &JsValue,
    f: impl FnOnce(&mut AbortSignal) -> R,
) -> JsResult<R> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("abort signal receiver is not an object"))?;
    let Some(mut signal) = object.downcast_mut::<AbortSignal>() else {
        return Err(JsNativeError::typ()
            .with_message("receiver is not an AbortSignal")
            .into());
    };
    Ok(f(&mut signal))
}

pub(crate) fn with_abort_signal_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&AbortSignal) -> R,
) -> JsResult<R> {
    let signal = object.downcast_ref::<AbortSignal>().ok_or_else(|| {
        JsNativeError::typ().with_message("object is not an AbortSignal")
    })?;
    Ok(f(&signal))
}

pub(crate) fn is_abort_signal_object(object: &JsObject) -> bool {
    object.downcast_ref::<AbortSignal>().is_some()
}