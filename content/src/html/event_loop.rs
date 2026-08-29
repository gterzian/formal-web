//! The content process's half of the HTML event loop: the task queue sender
//! handed to global scopes, plus the facade over the map of active timers
//! owned by `super::timers`.

use std::cell::RefCell;
use std::rc::Rc;

use crossbeam_channel::Sender;
use ipc_messages::content::{Command, DocumentId, WindowTimerKey};

use super::timers::MapOfActiveTimers;

/// Whether this command should be run as an HTML event-loop task: popped from
/// the task queue as oldestTask and run via step 2 of the event loop
/// processing model ("perform a task", then a microtask checkpoint).  These are
/// commands that execute page work — timers, message ports, render
/// opportunities, script/event automation, beforeunload.  Anything else is a
/// control message that drives the content process directly (viewport, document
/// lifecycle, navigation fetch completion, shutdown) and is handled without the
/// task-queue ceremony.
/// <https://html.spec.whatwg.org/#event-loop-processing-model>
pub(crate) fn command_is_event_loop_task(command: &Command) -> bool {
    matches!(
        command,
        Command::RunWindowTimer { .. }
            | Command::RunPortMessageTask { .. }
            | Command::PortTask { .. }
            | Command::PostMessage(_)
            | Command::UpdateTheRendering { .. }
            | Command::EvaluateScript { .. }
            | Command::ClickElement { .. }
            | Command::DispatchEvent { .. }
            | Command::RunBeforeUnload { .. }
    )
}

/// <https://html.spec.whatwg.org/#task-source>
#[derive(Clone)]
pub(crate) struct EventLoopTaskSources {
    task_sender: Sender<Command>,
    active_timers: Rc<RefCell<MapOfActiveTimers>>,
}

impl EventLoopTaskSources {
    pub(crate) fn new(
        task_sender: Sender<Command>,
        active_timers: Rc<RefCell<MapOfActiveTimers>>,
    ) -> Self {
        Self {
            task_sender,
            active_timers,
        }
    }

    /// <https://html.spec.whatwg.org/#queue-a-task>
    pub(crate) fn queue_a_task(&self, task: Command) -> Result<(), String> {
        // Steps 1-7: "If event loop was not given, set event loop to the implied
        // event loop." ... "Set task's script evaluation environment settings
        // object set to an empty set."
        // Note: The caller builds the task as the `Command` whose
        // `handle_command_inner` arm is the task's steps; the event loop is this
        // content process's, and the task's source, document and script
        // evaluation environment settings object set are not tracked.
        // Step 8: "Let queue be the task queue to which source is associated on event loop."
        // Step 9: "Append task to queue."
        // Note: Every task source shares one queue, owned by the content main
        // loop, which appends what it receives here.
        self.task_sender
            .send(task)
            .map_err(|error| format!("failed to queue a task on the event loop: {error}"))
    }

    /// <https://html.spec.whatwg.org/#run-steps-after-a-timeout>
    pub(crate) fn run_steps_after_a_timeout(
        &self,
        document_id: DocumentId,
        timer_key: WindowTimerKey,
        milliseconds: u32,
        timer_id: u32,
        nesting_level: u32,
    ) {
        self.active_timers.borrow_mut().run_steps_after_a_timeout(
            document_id,
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
