use std::cell::Cell;
use std::vec::Vec;

use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use js_engine::{JsTypes, records::PromiseResolvers};

use crate::js::Types;

type JsObject = <Types as JsTypes>::JsObject;

/// The lifecycle state of a pending wasm request.
#[cfg(all(boa_backend, feature = "wasm"))]
#[gc_struct]
#[derive(Debug, PartialEq, Eq)]
pub enum PendingState {
    Pending,
    Processing,
}

/// A pending WebAssembly request stored during JS execution.
/// JS-typed fields (promise, resolvers) are stored separately
/// in `WasmState.pending_wasm_resolvers` keyed by `request_id`.
#[cfg(all(boa_backend, feature = "wasm"))]
#[gc_struct]
pub enum PendingRequest {
    WasmCompile {
        #[ignore_trace]
        bytes: Vec<u8>,
        #[ignore_trace]
        request_id: u64,
        is_instantiate: bool,
        #[ignore_trace]
        state: PendingState,
    },
    WasmInstantiate {
        #[ignore_trace]
        module: wasmtime::Module,
        #[ignore_trace]
        request_id: u64,
        #[ignore_trace]
        state: PendingState,
    },
}

/// Consolidated wasm state used by GlobalScope.
/// All wasm-related fields and methods are here so GlobalScope
/// only needs a single `Option<WasmState>` field.
#[cfg(all(boa_backend, feature = "wasm"))]
#[gc_struct]
pub struct WasmState {
    /// Queue of pending async requests created during JS execution.
    pending_requests: GcCell<Vec<PendingRequest>>,

    /// Counter for generating unique request IDs.
    #[ignore_trace]
    request_id_counter: Cell<u64>,

    /// Map of request_id → (promise, resolvers) for pending operations.
    /// Separate from PendingRequest so domain code can push requests
    /// without importing boa_engine.
    pending_resolvers: GcCell<Vec<(u64, JsObject, PromiseResolvers<Types>)>>,
}

#[cfg(all(boa_backend, feature = "wasm"))]
impl WasmState {
    pub fn new() -> Self {
        Self {
            pending_requests: gc_cell_new(Vec::new()),
            request_id_counter: Cell::new(0),
            pending_resolvers: gc_cell_new(Vec::new()),
        }
    }

    pub fn push_pending_request(&self, request: PendingRequest) {
        self.pending_requests.borrow_mut().push(request);
    }

    pub fn next_request_id(&self) -> u64 {
        let id = self.request_id_counter.get();
        self.request_id_counter.set(id.wrapping_add(1));
        id
    }

    pub fn take_pending_wasm_batches(&self) -> Vec<(u64, Vec<u8>)> {
        let mut requests = self.pending_requests.borrow_mut();
        let mut batches = Vec::new();
        for request in requests.iter_mut() {
            if let PendingRequest::WasmCompile {
                bytes,
                request_id,
                state,
                ..
            } = request
            {
                if *state == PendingState::Pending {
                    *state = PendingState::Processing;
                    batches.push((*request_id, bytes.clone()));
                }
            }
        }
        batches
    }

    pub fn take_pending_wasm_instantiates(&self) -> Vec<(u64, wasmtime::Module)> {
        let mut requests = self.pending_requests.borrow_mut();
        let mut instantiates = Vec::new();
        for request in requests.iter_mut() {
            if let PendingRequest::WasmInstantiate {
                module,
                request_id,
                state,
            } = request
            {
                if *state == PendingState::Pending {
                    *state = PendingState::Processing;
                    instantiates.push((*request_id, module.clone()));
                }
            }
        }
        instantiates
    }

    pub fn store_wasm_resolver(
        &self,
        request_id: u64,
        promise: JsObject,
        resolvers: PromiseResolvers<Types>,
    ) {
        self.pending_resolvers
            .borrow_mut()
            .push((request_id, promise, resolvers));
    }

    pub fn consume_wasm_request(
        &self,
        request_id: u64,
    ) -> Option<(JsObject, PromiseResolvers<Types>)> {
        {
            let mut requests = self.pending_requests.borrow_mut();
            let idx = requests.iter().position(|r| match r {
                PendingRequest::WasmCompile {
                    request_id: rid, ..
                } => *rid == request_id,
                PendingRequest::WasmInstantiate {
                    request_id: rid, ..
                } => *rid == request_id,
            });
            if let Some(idx) = idx {
                requests.swap_remove(idx);
            }
        }
        let mut resolvers = self.pending_resolvers.borrow_mut();
        let idx = resolvers
            .iter()
            .position(|(rid, _, _)| *rid == request_id)?;
        let (_rid, promise, res) = resolvers.swap_remove(idx);
        Some((promise, res))
    }
}

/// Content-process-level wasm state: background worker + pending request tracking.
/// Consolidates all wasm-related fields that ContentProcess owns.
#[cfg(all(boa_backend, feature = "wasm"))]
pub struct ContentWasmState {
    pub worker: crate::wasm::WasmWorker,
    pub pending_requests: std::collections::HashMap<u64, ipc_messages::content::DocumentId>,
    pub pending_modules: std::collections::HashMap<u64, wasmtime::Module>,
}

#[cfg(all(boa_backend, feature = "wasm"))]
impl ContentWasmState {
    pub fn new(wasm_signal_sender: crossbeam_channel::Sender<()>) -> Self {
        Self {
            worker: crate::wasm::WasmWorker::new(wasmtime::Engine::default(), wasm_signal_sender),
            pending_requests: std::collections::HashMap::new(),
            pending_modules: std::collections::HashMap::new(),
        }
    }
}
