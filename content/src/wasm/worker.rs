use log::error;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Sender, unbounded};
use wasmtime::{Instance as WasmtimeInstance, Module, Store};

/// Requests sent to the background wasm worker.
#[derive(Debug)]
pub(crate) enum WasmRequest {
    /// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
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
        /// The wasmtime store, wrapped in Mutex for safe shared access.
        store: Arc<Mutex<Store<()>>>,
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
}

impl WasmWorker {
    pub(crate) fn new(engine: wasmtime::Engine, signal_sender: Sender<()>) -> Self {
        Self {
            request_sender: None,
            results: Arc::new(Mutex::new(VecDeque::new())),
            signal_sender: Some(signal_sender),
            handle: None,
            engine,
        }
    }

    /// Drain all available results from the shared queue.
    /// Called at the end of `handle_command`.
    pub(crate) fn drain_results(&self) -> Vec<WasmResult> {
        let mut queue = self.results.lock().unwrap();
        queue.drain(..).collect()
    }

    /// Submit a compilation request with a given request_id.
    /// The caller (drain_all_pending_wasm_requests) owns the request_id from
    /// the document's counter and stores it in pending_wasm_requests.  The
    /// worker uses this same ID so the result can be matched back.
    pub(crate) fn submit_compile(&mut self, bytes: Vec<u8>, request_id: u64) {
        self.ensure_worker_started();
        if let Some(sender) = &self.request_sender {
            if let Err(error) = sender.send(WasmRequest::Compile { request_id, bytes }) {
                error!("WebAssembly: failed to send compile request: {error}");
            }
        }
    }

    /// Submit an instantiation request for a previously-compiled module.
    pub(crate) fn submit_instantiate(&mut self, module: Module, request_id: u64) {
        self.ensure_worker_started();
        if let Some(sender) = &self.request_sender {
            if let Err(error) = sender.send(WasmRequest::Instantiate { request_id, module }) {
                error!("WebAssembly: failed to send instantiate request: {error}");
            }
        }
    }

    /// <https://webassembly.github.io/spec/js-api/#instantiate-the-core-of-a-webassembly-module>
    ///
    /// Runs on the background worker thread.  Creates a fresh store for the
    /// wasmtime engine and instantiates the module with empty imports.
    /// Returns the store and instance on success, or an error message string
    /// on failure.
    ///
    /// Note: The spec algorithm runs in the context of the "surrounding agent's
    /// associated store".  Since each instantiation gets its own store in the
    /// current architecture (rather than a shared per-agent store), step 1
    /// ("Let store be the surrounding agent's associated store") is replaced by
    /// creating a fresh `Store` and step 5 ("Set the surrounding agent's
    /// associated store to store") is a no-op.
    fn instantiate_the_core_of_a_webassembly_module(
        engine: &wasmtime::Engine,
        module: &wasmtime::Module,
    ) -> Result<(wasmtime::Store<()>, wasmtime::Instance), String> {
        // Step 1: "Let store be the surrounding agent's associated store."
        // Note: Each instantiation gets a fresh store (see function doc).
        let mut store = Store::new(engine, ());

        // Step 2: "Let result be module_instantiate(store, module, imports)."
        // Note: imports are empty — read-the-imports step (spec step 3-5 of
        // the outer algorithm) is not yet implemented.
        let result = WasmtimeInstance::new(&mut store, module, &[]);

        // Step 3: "If result is error, throw an appropriate exception type."
        // Note: We return the error as a String; the caller maps it to an
        // appropriate JS error type (LinkError, RuntimeError, etc.) based on
        // the error kind.  Distinguishing link errors from runtime errors is
        // not yet implemented — all errors produce the same string.
        let instance = result.map_err(|error| error.to_string())?;

        // Step 4: "Let (store, instance) be result."
        // Step 5: "Set the surrounding agent's associated store to store."
        // Note: No-op — the store is returned and wrapped in Arc<Mutex<>> for
        // the content process to use.
        //
        // Step 6: "Return instance."
        Ok((store, instance))
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
                    match Self::instantiate_the_core_of_a_webassembly_module(&engine, &module) {
                        Ok((store, instance)) => {
                            let store = Arc::new(Mutex::new(store));
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
                        Err(message) => {
                            Self::push_result(
                                &results,
                                &signal_sender,
                                WasmResult::InstantiateError {
                                    request_id,
                                    message,
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
                error!("WebAssembly: failed to join worker: {error:?}");
            }
        }
    }
}
