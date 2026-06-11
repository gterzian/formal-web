use crossbeam_channel::{Receiver, Sender, select};
use ipc_messages::content::{DocumentFetchId, DocumentId, EventLoopId, WindowTimerKey};
use std::collections::HashMap;
use log::debug;
use std::time::{Duration, Instant};
use uuid::Uuid;
use verification::TraceSender;

use crate::UserAgentCommand;

/// Commands that the user-agent and event-loop workers can send into the dedicated timer worker.
pub enum TimerCommand {
    Schedule {
        timer_key: Uuid,
        delay: Duration,
        completion: TimerCompletion,
    },
    Clear {
        timer_key: Uuid,
    },
    Shutdown {
        reply: Sender<Result<(), String>>,
    },
}

/// Completions that the timer worker routes back into the user-agent thread when a deadline expires.
#[derive(Clone)]
pub enum TimerCompletion {
    DocumentFetchTimeout {
        event_loop_id: EventLoopId,
        handler_id: DocumentFetchId,
    },
    WindowTimerTask {
        event_loop_id: EventLoopId,
        document_id: DocumentId,
        timer_id: u32,
        timer_key: WindowTimerKey,
        nesting_level: u32,
    },
}

/// one active host-side timer deadline owned by the timer worker.
pub struct ScheduledTimer {
    /// Deadline used by the Rust timer thread to wake the next due timer.
    pub deadline: Instant,
    /// tiebreaker and wake-generation analogue for timers scheduled at the same
    /// instant.
    pub sequence_number: u64,
    /// Completion routed back into the user-agent thread when the deadline expires.
    pub completion: TimerCompletion,
}

/// Stateful owner of HTML timer deadlines plus the fetch watchdog deadlines used by the
/// user-agent implementation.
struct TimerWorker {
    /// Receiver for timer schedule/clear/shutdown commands.
    command_receiver: Receiver<TimerCommand>,
    /// Sender back into the user-agent thread for timer expirations.
    user_agent_command_sender: Sender<UserAgentCommand>,
    /// Active timers keyed by the raw UUID from the timer key supplied by content or
    /// fetch callers. Both WindowTimerKey and DocumentFetchId wrap Uuid so the timer worker uses
    /// the inner Uuid directly as a neutral key.
    active_timers: HashMap<Uuid, ScheduledTimer>,
    /// Monotonic sequence used to preserve deterministic ordering among equal deadlines.
    next_sequence_number: u64,
}

/// timer debug output related to schedule, clear, and fire operations.
fn log_timer_debug(message: impl AsRef<str>) {
    if std::env::var_os("FORMAL_WEB_DEBUG_TIMERS").is_some() {
        debug!("[timer-debug][user-agent] {}", message.as_ref());
    }
}

impl TimerWorker {
    /// creating the dedicated timer worker owned by `UserAgentWorker`.
    fn new(
        command_receiver: Receiver<TimerCommand>,
        user_agent_command_sender: Sender<UserAgentCommand>,
    ) -> Self {
        Self {
            command_receiver,
            user_agent_command_sender,
            active_timers: HashMap::new(),
            next_sequence_number: 0,
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#timers>
    fn dispatch_due_timers(&mut self) {
        let now = Instant::now();
        let mut due_timers = self
            .active_timers
            .iter()
            .filter_map(|(timer_key, timer)| {
                if timer.deadline <= now {
                    Some((
                        *timer_key,
                        timer.deadline,
                        timer.sequence_number,
                        timer.completion.clone(),
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        due_timers.sort_by(|lhs, rhs| lhs.1.cmp(&rhs.1).then(lhs.2.cmp(&rhs.2)));

        for (timer_key, _deadline, _sequence_number, completion) in due_timers {
            self.active_timers.remove(&timer_key);
            match completion {
                TimerCompletion::DocumentFetchTimeout {
                    event_loop_id,
                    handler_id,
                } => {
                    // Document fetch timeouts are a watchdog around fetches owned
                    // by an event loop; they are not Window timers.
                    let _ = self.user_agent_command_sender.send(
                        UserAgentCommand::DocumentFetchTimeout {
                            event_loop_id,
                            handler_id,
                        },
                    );
                }
                TimerCompletion::WindowTimerTask {
                    event_loop_id,
                    document_id,
                    timer_id,
                    timer_key,
                    nesting_level,
                } => {
                    // Content already computed the timer id, key, and nesting level; the timer
                    // worker now re-enqueues the timer task on the owning event loop.
                    let _ =
                        self.user_agent_command_sender
                            .send(UserAgentCommand::WindowTimerTask {
                                event_loop_id,
                                document_id,
                                timer_id,
                                timer_key,
                                nesting_level,
                            });
                }
            }
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#timer-initialisation-steps>
    /// <https://html.spec.whatwg.org/multipage/#dom-cleartimeout>
    fn apply_command(&mut self, command: TimerCommand) -> Option<Sender<Result<(), String>>> {
        match command {
            TimerCommand::Schedule {
                timer_key,
                delay,
                completion,
            } => {
                // Scheduling records the host-side deadline associated with a timer created by
                // HTML's timer initialization steps or with a document fetch timeout.
                log_timer_debug(format!(
                    "schedule key={} delay_ms={}",
                    timer_key,
                    delay.as_millis()
                ));
                self.active_timers.insert(
                    timer_key,
                    ScheduledTimer {
                        deadline: Instant::now() + delay,
                        sequence_number: self.next_sequence_number,
                        completion,
                    },
                );
                self.next_sequence_number += 1;
                None
            }
            TimerCommand::Clear { timer_key } => {
                // The clearTimeout() and clearInterval() method steps are to remove this's map of
                // setTimeout and setInterval IDs[id].
                log_timer_debug(format!("clear key={}", timer_key));
                self.active_timers.remove(&timer_key);
                None
            }
            TimerCommand::Shutdown { reply } => Some(reply),
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#timers>
    fn run(&mut self) {
        // This loop waits until the next host-side deadline or until new schedule, clear, or
        // shutdown commands arrive from the user-agent and event-loop workers.
        loop {
            self.dispatch_due_timers();

            let next_deadline = self
                .active_timers
                .values()
                .map(|timer| timer.deadline)
                .min();
            if let Some(deadline) = next_deadline {
                let wait_duration: Duration = deadline.saturating_duration_since(Instant::now());
                let command_receiver = &self.command_receiver;
                select! {
                    recv(command_receiver) -> command => {
                        let Ok(command) = command else {
                            break;
                        };
                        if let Some(reply) = self.apply_command(command) {
                            self.active_timers.clear();
                            let _ = reply.send(Ok(()));
                            break;
                        }
                    }
                    recv(crossbeam_channel::after(wait_duration)) -> _ => {}
                }
            } else {
                let Ok(command) = self.command_receiver.recv() else {
                    break;
                };
                if let Some(reply) = self.apply_command(command) {
                    let _ = reply.send(Ok(()));
                    break;
                }
            }
        }
    }
}

/// spawning the dedicated timer worker thread owned by `UserAgentWorker`.
pub fn run_timer_thread(
    command_receiver: Receiver<TimerCommand>,
    user_agent_command_sender: Sender<UserAgentCommand>,
    _trace_sender: Option<TraceSender>,
) {
    let mut worker = TimerWorker::new(command_receiver, user_agent_command_sender);
    worker.run();
}
