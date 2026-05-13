use crossbeam_channel::{Receiver, Sender, select};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::UserAgentCommand;

pub enum TimerCommand {
    Schedule {
        timer_key: u64,
        delay: Duration,
        completion: TimerCompletion,
    },
    Clear {
        timer_key: u64,
    },
    Shutdown {
        reply: Sender<Result<(), String>>,
    },
}

#[derive(Clone)]
pub enum TimerCompletion {
    DocumentFetchTimeout {
        event_loop_id: usize,
        handler_id: u64,
    },
    WindowTimerTask {
        event_loop_id: usize,
        document_id: u64,
        timer_id: u32,
        timer_key: u64,
        nesting_level: u32,
    },
}

pub struct ScheduledTimer {
    /// Deadline used by the Rust timer thread to wake the next due timer.
    pub deadline: Instant,
    /// Model-local tiebreaker and wake-generation analogue for timers scheduled at the same
    /// instant.
    pub sequence_number: u64,
    /// Completion routed back into the user-agent thread when the deadline expires.
    pub completion: TimerCompletion,
}

struct TimerWorker {
    /// Receiver for timer schedule/clear/shutdown commands.
    command_receiver: Receiver<TimerCommand>,
    /// Sender back into the user-agent thread for timer expirations.
    user_agent_command_sender: Sender<UserAgentCommand>,
    /// Active timers keyed by the model-local timer key supplied by content or fetch callers.
    active_timers: HashMap<u64, ScheduledTimer>,
    /// Monotonic sequence used to preserve deterministic ordering among equal deadlines.
    next_sequence_number: u64,
}

fn timer_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_TIMERS").is_some()
}

fn log_timer_debug(message: impl AsRef<str>) {
    if timer_debug_enabled() {
        eprintln!("[timer-debug][user-agent] {}", message.as_ref());
    }
}

impl TimerWorker {
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
                    let _ = self
                        .user_agent_command_sender
                        .send(UserAgentCommand::DocumentFetchTimeout {
                            event_loop_id,
                            handler_id,
                        });
                }
                TimerCompletion::WindowTimerTask {
                    event_loop_id,
                    document_id,
                    timer_id,
                    timer_key,
                    nesting_level,
                } => {
                    let _ = self
                        .user_agent_command_sender
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

    fn apply_command(&mut self, command: TimerCommand) -> Option<Sender<Result<(), String>>> {
        match command {
            TimerCommand::Schedule {
                timer_key,
                delay,
                completion,
            } => {
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
                log_timer_debug(format!("clear key={}", timer_key));
                self.active_timers.remove(&timer_key);
                None
            }
            TimerCommand::Shutdown { reply } => Some(reply),
        }
    }

    fn run(&mut self) {
        loop {
            self.dispatch_due_timers();

            let next_deadline = self.active_timers.values().map(|timer| timer.deadline).min();
            if let Some(deadline) = next_deadline {
                let wait_duration = deadline.saturating_duration_since(Instant::now());
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

pub fn run_timer_thread(
    command_receiver: Receiver<TimerCommand>,
    user_agent_command_sender: Sender<UserAgentCommand>,
) {
    let mut worker = TimerWorker::new(command_receiver, user_agent_command_sender);
    worker.run();
}