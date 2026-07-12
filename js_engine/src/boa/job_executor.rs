//! Custom `BoaJobExecutor` — replaces Boa's default `SimpleJobExecutor` and
//! wraps ECMAScript jobs into generic [`Job<BoaTypes>`](crate::Job)s that are
//! forwarded to the domain microtask queue via a callback.
//!
//! # Design
//!
//! When Boa internally calls `enqueue_job` (e.g. via `HostEnqueuePromiseJob`),
//! our executor converts the native `PromiseJob`/`GenericJob` into a generic
//! `Job<BoaTypes>` whose closure calls `PromiseJob::call(context)` or
//! `GenericJob::call(context)` (extracting `&mut Context` from the trait
//! object via the repr(transparent) cast).  The generic `Job` is then
//! forwarded to an enqueue callback set by the content crate.
//!
//! The content crate's callback pushes a `Microtask::BoaJob { job }` variant
//! into the shared domain microtask queue (see `content/src/html/microtask.rs`).
//! When the domain queue is drained and the `BoaJob` microtask is executed,
//! it calls `ec.run_job(job)` which dispatches back to
//! `BoaContext::run_job` → `PromiseJob::call(context)`.
//!
//! This way `BoaContext::run_jobs()` (the trait method) can be a no-op:
//! all job execution flows through the domain microtask queue.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use boa_engine::{
    Context, JsResult, JsValue,
    job::{Job as BoaJob, JobExecutor},
};

use crate::Job;
use crate::boa::BoaTypes;
use crate::boa::engine::ec_to_ctx;

/// A FIFO job executor for Boa's ECMAScript jobs that forwards them to the
/// domain microtask queue via a callback.
///
/// Replaces `SimpleJobExecutor` in `build_boa_context`.  Boa's promise and
/// generic jobs are wrapped into generic [`Job<BoaTypes>`] values and pushed
/// to `enqueue_callback`, which the content crate sets to push into the
/// shared `Vec<Microtask>` queue.
#[allow(clippy::type_complexity)]
pub struct BoaJobExecutor {
    /// Callback invoked for each enqueued job.  Set by the content crate to
    /// push `Microtask::BoaJob` into the shared domain queue.
    /// Uses `RefCell` for interior mutability so the callback can be set
    /// through a shared `Rc<BoaJobExecutor>` reference.
    enqueue_callback: RefCell<Option<Box<dyn Fn(Job<BoaTypes>)>>>,

    /// Buffer of jobs enqueued before the callback was set.
    /// Drained into the callback when `set_enqueue_callback` is called.
    pending_jobs: RefCell<VecDeque<Job<BoaTypes>>>,
}

impl BoaJobExecutor {
    /// Creates a new `BoaJobExecutor` with no callback.
    /// Jobs will be buffered until `set_enqueue_callback` is called.
    #[must_use]
    pub fn new() -> Self {
        Self {
            enqueue_callback: RefCell::new(None),
            pending_jobs: RefCell::new(VecDeque::new()),
        }
    }

    /// Creates a new `BoaJobExecutor` with an enqueue callback.
    pub fn with_callback(callback: Box<dyn Fn(Job<BoaTypes>)>) -> Self {
        Self {
            enqueue_callback: RefCell::new(Some(callback)),
            pending_jobs: RefCell::new(VecDeque::new()),
        }
    }

    /// Set the enqueue callback after construction.
    /// Drains any buffered jobs into the callback.
    /// Uses `RefCell::replace` for interior mutability, allowing the
    /// callback to be set through a shared `&self` reference.
    pub fn set_enqueue_callback(&self, callback: Box<dyn Fn(Job<BoaTypes>)>) {
        // First flush any buffered jobs into the new callback.
        let pending = self.pending_jobs.borrow_mut().drain(..).collect::<Vec<_>>();
        for job in pending {
            callback(job);
        }
        self.enqueue_callback.replace(Some(callback));
    }
}

impl Default for BoaJobExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl JobExecutor for BoaJobExecutor {
    /// <https://tc39.es/ecma262/#sec-hostenqueuepromisejob>
    /// <https://tc39.es/ecma262/#sec-hostenqueuegenericjob>
    ///
    /// Wraps Boa's native `Job` into a generic [`Job<BoaTypes>`] and forwards
    /// it to the domain microtask queue via `enqueue_callback`.
    fn enqueue_job(self: Rc<Self>, job: BoaJob, _context: &mut Context) {
        let generic_job: Job<BoaTypes> = match job {
            BoaJob::PromiseJob(promise_job) => {
                Job::new(move |ec| {
                    // SAFETY: ec is backed by BoaContext (repr(transparent) over Context).
                    let ctx: &mut Context = unsafe { ec_to_ctx(ec) };
                    match promise_job.call(ctx) {
                        Ok(_) => Ok(()),
                        Err(error) => Err(error.into_opaque(ctx).unwrap_or(JsValue::undefined())),
                    }
                })
            }
            BoaJob::GenericJob(generic_job) => {
                Job::new(move |ec| {
                    // SAFETY: ec is backed by BoaContext (repr(transparent) over Context).
                    let ctx: &mut Context = unsafe { ec_to_ctx(ec) };
                    match generic_job.call(ctx) {
                        Ok(_) => Ok(()),
                        Err(error) => Err(error.into_opaque(ctx).unwrap_or(JsValue::undefined())),
                    }
                })
            }
            // Timeout, interval, async, and finalization-registry jobs are
            // not used by formal-web's content process (timers go through the
            // HTML event loop, async jobs are not wired into the engine).
            _ => return,
        };

        // If the callback is set, forward immediately.  Otherwise buffer
        // so jobs enqueued during initialization are not lost.
        let callback_borrow = self.enqueue_callback.borrow();
        if let Some(ref callback) = *callback_borrow {
            callback(generic_job);
        } else {
            drop(callback_borrow);
            self.pending_jobs.borrow_mut().push_back(generic_job);
        }
    }

    /// No-op: jobs are forwarded to the domain microtask queue, not stored
    /// locally.  `BoaContext::run_jobs()` is also a no-op for the same reason.
    fn run_jobs(self: Rc<Self>, _context: &mut Context) -> JsResult<()> {
        Ok(())
    }
}
