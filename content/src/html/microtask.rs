use ipc::IpcSender;
use ipc_messages::content::{DocumentId, Event as ContentEvent, NavigableId};
use ipc_messages::media::VideoPaintId;
use js_engine::gc::{GcCell, JsValueCell};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, Job};

use crate::js::Types;
use crate::streams::{
    ByteTeeState, PipeToState, TeeState, readable_byte_stream_tee_default_reader_chunk_steps,
    readable_stream_default_tee_read_request_chunk_steps,
};

/// <https://html.spec.whatwg.org/#microtask-queue>
///
/// A microtask is an algorithm queued on the event loop's microtask queue.
/// Each variant stores the data needed by a specific algorithm step.
/// See <https://html.spec.whatwg.org/#queue-a-microtask>.
#[gc_struct]
pub(crate) enum Microtask {
    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    PipeToReadSettled {
        state: PipeToState,
        result: JsValueCell,
    },
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
    DefaultTeeChunkSteps {
        tee_state: GcCell<TeeState>,
        clone_for_branch2: bool,
        chunk: JsValueCell,
    },
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    ByteTeeChunkSteps {
        tee_state: GcCell<ByteTeeState>,
        chunk: JsValueCell,
    },
    /// <https://html.spec.whatwg.org/#media-element-load-algorithm>
    /// Synchronous section of the resource selection algorithm.
    MediaElementAwaitStableState {
        #[ignore_trace]
        event_sender: Option<IpcSender<ContentEvent>>,
        #[ignore_trace]
        traversable_id: Option<NavigableId>,
        #[ignore_trace]
        document_id: Option<DocumentId>,
        #[ignore_trace]
        resolved_src: Option<String>,
        #[ignore_trace]
        video_paint_id: VideoPaintId,
    },
    /// <https://tc39.es/ecma262/#sec-jobs>
    ///
    /// An ECMAScript job (promise reaction, resolve-thenable, or generic)
    /// queued by the engine's job executor and forwarded to the domain
    /// microtask queue.  Executed by calling `ec.run_job(job)`.
    ///
    /// For Boa, this wraps Boa's native `PromiseJob`/`GenericJob` types.
    /// For JSC, this variant is never constructed — JSC handles its own
    /// internal microtask queue.
    ///
    /// `RefCell` is needed because `Job` is `FnOnce` (can only be called
    /// once) but `Microtask::call` receives `&self`.
    JsJob {
        #[ignore_trace]
        job: std::cell::RefCell<Option<Job<Types>>>,
    },
}

impl Microtask {
    /// <https://html.spec.whatwg.org/#queue-a-microtask>
    pub(crate) fn call(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        match self {
            Self::PipeToReadSettled { state, result } => {
                let value = result.borrow().clone();
                state.clone().call_on_read_request_settled(value, ec)
            }
            Self::DefaultTeeChunkSteps {
                tee_state,
                clone_for_branch2,
                chunk,
            } => {
                let value = chunk.borrow().clone();
                readable_stream_default_tee_read_request_chunk_steps(
                    tee_state.clone(),
                    *clone_for_branch2,
                    value,
                    ec,
                )
            }
            Self::ByteTeeChunkSteps { tee_state, chunk } => {
                let value = chunk.borrow().clone();
                readable_byte_stream_tee_default_reader_chunk_steps(tee_state.clone(), value, ec)
            }
            Self::MediaElementAwaitStableState {
                event_sender,
                traversable_id,
                document_id,
                resolved_src,
                video_paint_id,
            } => super::html_media_element::media_element_await_stable_state_microtask(
                event_sender.clone(),
                *traversable_id,
                *document_id,
                resolved_src.clone(),
                *video_paint_id,
                ec,
            ),
            Self::JsJob { job } => {
                if let Some(job) = job.borrow_mut().take() {
                    job.call(ec)
                } else {
                    Ok(())
                }
            }
        }
    }
}

impl std::fmt::Debug for Microtask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PipeToReadSettled { .. } => f.debug_struct("PipeToReadSettled").finish(),
            Self::DefaultTeeChunkSteps { .. } => f.debug_struct("DefaultTeeChunkSteps").finish(),
            Self::ByteTeeChunkSteps { .. } => f.debug_struct("ByteTeeChunkSteps").finish(),
            Self::MediaElementAwaitStableState { .. } => {
                f.debug_struct("MediaElementAwaitStableState").finish()
            }
            Self::JsJob { .. } => f.debug_struct("JsJob").finish(),
        }
    }
}
