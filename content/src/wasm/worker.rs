use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Sender, unbounded};
use wasmtime::{Instance as WasmtimeInstance, Module, Store};

/// Requests sent to the background wasm worker.
#[derive(Debug)]
pub(crate) enum WasmRequest {
    /// <https://www.w3.org/TR/wasm-js-api/#asynchronously-compile-a-webassembly-module>
    ///
    /// Compile a WebAssembly module from the given bytes.
    Compile {
        /// Unique request id for correlating the result.
        request_id: u64,
        /// A copy of the bytes held by the buffer (stable bytes per spec step 1).
        bytes: Vec<u8>,
    },
    /// <https://webassembly.github.io/spec/js-api/#asynchronously-instantiate-a-webassembly-module>
    ///
    /// Instantiate a previously-compiled module.
    Instantiate {
        /// Unique request id for correlating the result.
        request_id: u64,
        /// The compiled module.
        module: Module,
    },
    /// Shutdown the background worker.
    Shutdown,
}

/// Results sent back from the background wasm worker to the main thread.
#[derive(Debug)]
pub(crate) enum WasmResult {
    /// A module was compiled successfully.
    Compiled {
        /// The request id that this result corresponds to.
        request_id: u64,
        /// The compiled WebAssembly module.
        module: Module,
    },
    /// Module compilation failed.
    CompileError {
        /// The request id that this error corresponds to.
        request_id: u64,
        /// The error message describing why compilation failed.
        message: String,
    },
    /// A module was instantiated successfully.
    Instantiated {
        /// The request id that this result corresponds to.
        request_id: u64,
        /// The wasmtime store (shared via Arc).
        store: Arc<Store<()>>,
        /// The wasmtime instance.
        instance: WasmtimeInstance,
    },
    /// Instantiation failed.
    InstantiateError {
        /// The request id that this result corresponds to.
        request_id: u64,
        /// The error message describing why instantiation failed.
        message: String,
    },
}

/// <https://www.w3.org/TR/wasm-js-api/#associated-store>
///
/// Manages a background worker thread for WebAssembly compilation and
/// instantiation.  A one-way crossbeam Sender passes requests from the
/// main thread to the worker.  The worker pushes results into a shared
/// `Mutex<VecDeque<WasmResult>>` which the content process drains inside
/// `handle_command` — everything runs on a single event loop.
///
/// The `Store` is shared via `Arc` so that exported-function wrappers on
/// the main thread can call into wasm through the same store the worker
/// created during instantiation.
pub(crate) struct WasmWorker {
    /// One-way sender for requests to the background worker.
    request_sender: Option<Sender<WasmRequest>>,
    /// Shared result queue: worker pushes, main thread drains.
    results: Arc<Mutex<VecDeque<WasmResult>>>,
    /// Crossbeam sender to signal the main thread that results are ready.
    signal_sender: Option<Sender<()>>,
    /// The worker thread handle, joined on drop.
    handle: Option<JoinHandle<()>>,
    /// The shared wasmtime engine (Send + Sync).
    engine: wasmtime::Engine,
    /// Next request id.
    next_request_id: u64,
}

impl WasmWorker {
    pub(crate) fn new(engine: wasmtime::Engine, signal_sender: Sender<()>) -> Self {
        Self {
            request_sender: None,
            results: Arc::new(Mutex::new(VecDeque::new())),
            signal_sender: Some(signal_sender),
            handle: None,
            engine,
            next_request_id: 0,
        }
    }

    pub(crate) fn next_request_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        id
    }

    /// Drain all available results from the shared queue.
    /// Called at the end of `handle_command`.
    pub(crate) fn drain_results(&self) -> Vec<WasmResult> {
        let mut queue = self.results.lock().unwrap();
        queue.drain(..).collect()
    }

    /// Submit a compilation request.
    pub(crate) fn submit_compile(&mut self, bytes: Vec<u8>) -> u64 {
        let request_id = self.next_request_id();
        self.ensure_worker_started();
        if let Some(sender) = &self.request_sender {
            if let Err(error) = sender.send(WasmRequest::Compile { request_id, bytes }) {
                eprintln!("wasm: failed to send compile request: {error}");
            }
        }
        request_id
    }

    /// Submit an instantiation request for a previously-compiled module.
    pub(crate) fn submit_instantiate(&mut self, module: Module) -> u64 {
        let request_id = self.next_request_id();
        self.ensure_worker_started();
        if let Some(sender) = &self.request_sender {
            if let Err(error) = sender.send(WasmRequest::Instantiate { request_id, module }) {
                eprintln!("wasm: failed to send instantiate request: {error}");
            }
        }
        request_id
    }

    /// Start the background worker if it hasn't been started yet.
    fn ensure_worker_started(&mut self) {
        if self.request_sender.is_some() {
            return;
        }

        let (request_sender, request_receiver) = unbounded::<WasmRequest>();
        let engine = self.engine.clone();
        let results = Arc::clone(&self.results);

        let signal_sender = self.signal_sender.clone().expect("signal_sender set at construction");
        let handle = thread::Builder::new()
            .name("wasm-worker".to_string())
            .spawn(move || {
                Self::worker_loop(engine, request_receiver, results, signal_sender);
            })
            .expect("failed to spawn wasm worker");

        self.request_sender = Some(request_sender);
        self.handle = Some(handle);
    }

    fn push_result(
        results: &Arc<Mutex<VecDeque<WasmResult>>>,
        signal_sender: &Sender<()>,
        result: WasmResult,
    ) {
        if let Ok(mut queue) = results.lock() {
            queue.push_back(result);
        }
        // Signal the main thread to wake up and drain results.
        let _ = signal_sender.send(());
    }

    /// The main loop for the background worker.
    fn worker_loop(
        engine: wasmtime::Engine,
        request_receiver: crossbeam_channel::Receiver<WasmRequest>,
        results: Arc<Mutex<VecDeque<WasmResult>>>,
        signal_sender: Sender<()>,
    ) {
        loop {
            match request_receiver.recv() {
                Ok(WasmRequest::Compile { request_id, bytes }) => {
                    let result = Module::new(&engine, &bytes);
                    match result {
                        Ok(module) => {
                            Self::push_result(
                                &results,
                                &signal_sender,
                                WasmResult::Compiled { request_id, module },
                            );
                        }
                        Err(error) => {
                            Self::push_result(
                                &results,
                                &signal_sender,
                                WasmResult::CompileError {
                                    request_id,
                                    message: error.to_string(),
                                },
                            );
                        }
                    }
                }
                Ok(WasmRequest::Instantiate { request_id, module }) => {
                    let mut store = Store::new(&engine, ());
                    let result = WasmtimeInstance::new(&mut store, &module, &[]);
                    match result {
                        Ok(instance) => {
                            let store = Arc::new(store);
                            Self::push_result(
                                &results,
                                &signal_sender,
                                WasmResult::Instantiated {
                                    request_id,
                                    store: Arc::clone(&store),
                                    instance,
                                },
                            );
                        }
                        Err(error) => {
                            Self::push_result(
                                &results,
                                &signal_sender,
                                WasmResult::InstantiateError {
                                    request_id,
                                    message: error.to_string(),
                                },
                            );
                        }
                    }
                }
                Ok(WasmRequest::Shutdown) | Err(_) => break,
            }
        }
    }
}

impl Drop for WasmWorker {
    fn drop(&mut self) {
        if let Some(sender) = &self.request_sender {
            let _ = sender.send(WasmRequest::Shutdown);
        }
        if let Some(handle) = self.handle.take() {
            if let Err(error) = handle.join() {
                eprintln!("wasm: failed to join worker: {error:?}");
            }
        }
    }
}
