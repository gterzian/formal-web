use std::cell::RefCell;
use std::rc::Rc;

use crossbeam_channel::{Receiver, Sender, unbounded};
use ipc_messages::content::{
    BeforeUnloadCheckId, DispatchEventEntry, DocumentId, NavigableId, NavigationId, PortId,
    PortTaskKind, WindowTimerKey, WorkerId,
};
use ipc_messages::safe_passing_of_structured_data::PostMessageRequest;
use log::error;

use crate::html::structured_data::safe_passing_of_structured_data::SerializeWithTransferResult;

use super::timers::{MapOfActiveTimers, TimerRealm};

/// <https://html.spec.whatwg.org/#concept-task>
pub(crate) enum Task {
    /// The task that runs one expired window timer's steps (step 9 of the
    /// <https://html.spec.whatwg.org/#timer-initialisation-steps>), queued on
    /// the <https://html.spec.whatwg.org/#timer-task-source> when the timer's
    /// expiry time in the global's map of active timers passes.
    RunWindowTimer {
        document_id: DocumentId,
        timer_id: u32,
        timer_key: WindowTimerKey,
        nesting_level: u32,
    },

    /// The task that runs one expired worker timer's steps, queued on the
    /// dedicated worker agent's own task queue when its event loop reaps an
    /// expired entry from the worker's own map of active timers (see
    /// `workers/dedicated_worker_agent.rs`).  The worker realm is identified by worker
    /// id.
    /// <https://html.spec.whatwg.org/#timer-initialisation-steps>
    RunWorkerTimer {
        worker_id: WorkerId,
        timer_id: u32,
        timer_key: WindowTimerKey,
        nesting_level: u32,
    },

    /// The message task of one message a dedicated worker's owner posted to
    /// it: fires a message event at the worker global scope (its role as the
    /// inside port's message event target).  Only ever queued on the
    /// dedicated worker agent's own task queue.
    /// <https://html.spec.whatwg.org/#message-event-target>
    RunWorkerInboundMessage {
        worker_id: WorkerId,
        payload: SerializeWithTransferResult,
    },

    /// The message task of one message a dedicated worker posted back to its
    /// owner: fires a message event at the worker's Worker platform object.
    /// Queued on the owner's event loop.
    /// <https://html.spec.whatwg.org/#message-event-target>
    RunWorkerOutboundMessage {
        worker_id: WorkerId,
        payload: SerializeWithTransferResult,
    },

    /// The message task for one message on a port whose message queue is
    /// enabled.  Each queued message fires in its own task, so the event loop
    /// can interleave other tasks between messages.
    /// <https://html.spec.whatwg.org/#port-message-queue>
    RunPortMessage { port: PortId },

    /// The task the user agent's port message routing hands to the port's
    /// owning event loop: `NewTask` runs the message task for a routed
    /// "Single" item, `Buffer` appends the messages that were buffered while
    /// the port was in transit.
    /// <https://html.spec.whatwg.org/#message-port-post-message-steps>
    PortRouting { port: PortId, kind: PortTaskKind },

    /// The task that fires the message event at the target window (step 8 and
    /// its substeps), queued once the source content process has run steps
    /// 1-7 and the user agent has routed the message here.
    /// <https://html.spec.whatwg.org/#window-post-message-steps>
    PostMessage(PostMessageRequest),

    /// <https://html.spec.whatwg.org/#update-the-rendering>
    UpdateTheRendering {
        traversable_id: NavigableId,
        document_id: DocumentId,
        /// Milliseconds since the Unix epoch on the browser-wide monotonic
        /// clock, captured when the user agent noted the rendering
        /// opportunity (the HTML event loop's "last render opportunity
        /// time").
        frame_timestamp_epoch_ms: f64,
    },

    /// The script a WebDriver or CDP client asked to evaluate in the
    /// traversable's active document.
    EvaluateScript {
        traversable_id: NavigableId,
        request_id: u64,
        source: String,
    },

    /// The click a WebDriver or CDP client asked to perform on the element
    /// matching `selector`.
    ClickElement {
        traversable_id: NavigableId,
        request_id: u64,
        selector: String,
    },

    /// The input events the embedder delivered for this document, dispatched
    /// as trusted events.
    DispatchEvent { events: Vec<DispatchEventEntry> },

    /// <https://html.spec.whatwg.org/#steps-to-fire-beforeunload>
    RunBeforeUnload {
        document_id: DocumentId,
        check_id: BeforeUnloadCheckId,
        navigation_id: NavigationId,
    },
}

/// <https://html.spec.whatwg.org/#task-queue>
/// Note: This one queue is shared by every task source: the handle the event
/// loop waits on and the handles global scopes hold through
/// [`EventLoopTaskSources`] are clones of the same channel.
#[derive(Clone)]
pub(crate) struct TaskQueue {
    sender: Sender<Task>,
    receiver: Receiver<Task>,
}

impl TaskQueue {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = unbounded();
        Self { sender, receiver }
    }

    /// <https://html.spec.whatwg.org/#queue-a-task>
    pub(crate) fn queue_a_task(&self, task: Task) {
        // Steps 1-7: "If event loop was not given, set event loop to the implied
        // event loop." ... "Set task's script evaluation environment settings
        // object set to an empty set."
        // Note: The caller builds the task as the [`Task`] whose `run_task` arm
        // is the task's steps; the event loop is this content process's, and the
        // task's source, document and script evaluation environment settings
        // object set are not tracked.
        // Step 8: "Let queue be the task queue to which source is associated on event loop."
        // Step 9: "Append task to queue."
        if let Err(error) = self.sender.send(task) {
            error!("failed to queue a task on the event loop: {error}");
        }
    }

    pub(crate) fn receiver(&self) -> Receiver<Task> {
        self.receiver.clone()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.receiver.is_empty()
    }
}

/// <https://html.spec.whatwg.org/#task-source>
#[derive(Clone)]
pub(crate) struct EventLoopTaskSources {
    task_queue: TaskQueue,
    active_timers: Rc<RefCell<MapOfActiveTimers>>,
}

impl EventLoopTaskSources {
    pub(crate) fn new(
        task_queue: TaskQueue,
        active_timers: Rc<RefCell<MapOfActiveTimers>>,
    ) -> Self {
        Self {
            task_queue,
            active_timers,
        }
    }

    pub(crate) fn task_queue(&self) -> TaskQueue {
        self.task_queue.clone()
    }

    /// <https://html.spec.whatwg.org/#run-steps-after-a-timeout>
    pub(crate) fn run_steps_after_a_timeout(
        &self,
        realm: TimerRealm,
        timer_key: WindowTimerKey,
        milliseconds: u32,
        timer_id: u32,
        nesting_level: u32,
    ) {
        self.active_timers.borrow_mut().run_steps_after_a_timeout(
            realm,
            timer_key,
            milliseconds,
            timer_id,
            nesting_level,
        );
    }

    /// <https://html.spec.whatwg.org/#run-steps-after-a-timeout>
    pub(crate) fn remove_active_timer(&self, timer_key: WindowTimerKey) {
        self.active_timers
            .borrow_mut()
            .remove_active_timer(timer_key);
    }
}
