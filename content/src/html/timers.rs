//! The content process's map of active window timers: the per-timer records
//! and the algorithms that schedule, cancel, and reap them.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ipc_messages::content::{DocumentId, WindowTimerKey, WorkerId};

/// The realm a timer belongs to: a window's associated Document, or a worker
/// global scope.  The map of active timers is shared by every realm of the
/// content process's event loop, so the expiry reaper needs to know which
/// realm's task to queue.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TimerRealm {
    /// <https://html.spec.whatwg.org/#concept-document>
    Document(DocumentId),
    /// <https://html.spec.whatwg.org/#the-workerglobalscope-common-interface>
    Worker(WorkerId),
}

/// <https://html.spec.whatwg.org/#map-of-active-timers>
#[derive(Clone)]
pub(crate) struct ActiveTimer {
    /// <https://html.spec.whatwg.org/#map-of-active-timers>
    pub expiry_time: Instant,
    /// <https://html.spec.whatwg.org/#run-steps-after-a-timeout>
    /// Note: Step 4.2 orders invocations that started earlier ahead of later
    /// ones with an equal or larger `milliseconds`; this counter records the
    /// start order so equal expiry times keep it.
    pub start_order: u64,
    /// <https://html.spec.whatwg.org/#map-of-active-timers>
    pub realm: TimerRealm,
    /// <https://html.spec.whatwg.org/#map-of-active-timers>
    pub timer_key: WindowTimerKey,
    /// <https://html.spec.whatwg.org/#map-of-settimeout-and-setinterval-ids>
    pub timer_id: u32,
    /// <https://html.spec.whatwg.org/#timer-nesting-level>
    pub nesting_level: u32,
}

/// <https://html.spec.whatwg.org/#map-of-active-timers>
#[derive(Default)]
pub(crate) struct MapOfActiveTimers {
    entries: HashMap<WindowTimerKey, ActiveTimer>,
    next_start_order: u64,
}

impl MapOfActiveTimers {
    /// <https://html.spec.whatwg.org/#run-steps-after-a-timeout>
    pub(crate) fn run_steps_after_a_timeout(
        &mut self,
        realm: TimerRealm,
        timer_key: WindowTimerKey,
        milliseconds: u32,
        timer_id: u32,
        nesting_level: u32,
    ) {
        // Step 1: "Let timerKey be a new unique internal value."
        // Note: The timer initialization steps allocate timerKey before calling
        // this algorithm so they can record it in the global's map of setTimeout
        // and setInterval IDs, and pass it in.

        // Step 2: "Let startTime be the current high resolution time given global."
        let start_time = Instant::now();

        // Step 3: "Set global's map of active timers[timerKey] to startTime plus milliseconds."
        self.entries.insert(
            timer_key,
            ActiveTimer {
                expiry_time: start_time + Duration::from_millis(u64::from(milliseconds)),
                start_order: self.next_start_order,
                realm,
                timer_key,
                timer_id,
                nesting_level,
            },
        );
        self.next_start_order += 1;

        // Step 4: "Run the following steps in parallel:"
        // Step 5: "Return timerKey."
        // Note: Steps 4.1-4.5 run on the content process main loop, which waits
        // on `earliest_expiry_wait` and then takes the expired entries with
        // `take_expired_timers`.  timerKey is this algorithm's argument, so
        // there is nothing to return.
    }

    /// <https://html.spec.whatwg.org/#run-steps-after-a-timeout>
    pub(crate) fn remove_active_timer(&mut self, timer_key: WindowTimerKey) {
        // Step 4.5: "Remove global's map of active timers[timerKey]."
        // Note: The clearTimeout() and clearInterval() method steps only remove
        // the id from the map of setTimeout and setInterval IDs, leaving the
        // active timer to expire and abort at step 9.2 of the timer
        // initialization steps.  Removing the entry here instead keeps the
        // content main loop from waking for a timer that can no longer run.
        self.entries.remove(&timer_key);
    }

    /// <https://html.spec.whatwg.org/#run-steps-after-a-timeout>
    pub(crate) fn earliest_expiry_wait(&self) -> Option<Duration> {
        // Step 4.1: "If global is a Window object, wait until global's
        // associated Document has been fully active for a further milliseconds
        // milliseconds (not necessarily consecutively)."
        // Note: Full activity is not tracked, so the wait runs consecutively.
        self.entries
            .values()
            .map(|timer| timer.expiry_time)
            .min()
            .map(|expiry_time| expiry_time.saturating_duration_since(Instant::now()))
    }

    /// <https://html.spec.whatwg.org/#run-steps-after-a-timeout>
    pub(crate) fn take_expired_timers(&mut self) -> Vec<ActiveTimer> {
        let now = Instant::now();
        // Step 4.2: "Wait until any invocations of this algorithm that had the
        // same global and orderingIdentifier, that started before this one, and
        // whose milliseconds is less than or equal to this one's, have
        // completed."
        let mut expired = self
            .entries
            .iter()
            .filter(|(_, timer)| timer.expiry_time <= now)
            .map(|(timer_key, timer)| (*timer_key, timer.clone()))
            .collect::<Vec<_>>();
        expired.sort_by(|(_, lhs), (_, rhs)| {
            lhs.expiry_time
                .cmp(&rhs.expiry_time)
                .then(lhs.start_order.cmp(&rhs.start_order))
        });

        // Step 4.5: "Remove global's map of active timers[timerKey]."
        expired
            .into_iter()
            .map(|(timer_key, timer)| {
                self.entries.remove(&timer_key);
                timer
            })
            .collect()
    }
}
