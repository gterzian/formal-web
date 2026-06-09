use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender, unbounded};
use wasmtime::Module;

/// <https://www.w3.org/TR/wasm-js-api/#asynchronously-compile-a-webassembly-module>
///
/// Requests sent to the background wasm compilation thread.
#[derive(Debug)]
pub(crate) enum WasmRequest {
    /// Compile a WebAssembly module from the given bytes.
    Compile {
        /// Unique request id for correlating the result.
        request_id: u64,
        /// A copy of the bytes held by the buffer (stable bytes per spec step 1).
        bytes: Vec<u8>,
    },
    /// Shutdown the background thread.
    Shutdown,
}

/// <https://www.w3.org/TR/wasm-js-api/#asynchronously-compile-a-webassembly-module>
///
/// Results sent back from the background wasm compilation thread to the main thread.
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
}

/// <https://www.w3.org/TR/wasm-js-api/#associated-store>
///
/// Manages a background thread for WebAssembly compilation.
/// Module compilation is the expensive part of the wasm pipeline and can run
/// in parallel with other content-process work.
///
/// Note: The wasmtime `Engine` is shared between the main thread and the
/// background thread because it is `Send + Sync`.  The `Store` (associated
/// store per agent) lives on the main thread and handles instantiation,
/// memory access, and other synchronous wasm operations.
pub(crate) struct WasmThread {
    /// Channel for sending compilation requests to the background thread.
    /// `None` until the background thread has been started.
    request_sender: Option<Sender<WasmRequest>>,
    /// Channel for receiving compilation results from the background thread.
    result_receiver: Option<Receiver<WasmResult>>,
    /// The thread handle, joined on drop.
    handle: Option<JoinHandle<()>>,
    /// The shared wasmtime engine (Send + Sync).
    engine: wasmtime::Engine,
    /// Next request id.
    next_request_id: u64,
}

impl WasmThread {
    /// Create a new wasm background compilation thread handle.
    ///
    /// The thread is lazily initialized — it is only spawned when the first
    /// compilation request is sent.  Until then `result_receiver()` returns
    /// `None`.
    pub(crate) fn new(engine: wasmtime::Engine) -> Self {
        Self {
            request_sender: None,
            result_receiver: None,
            handle: None,
            engine,
            next_request_id: 0,
        }
    }

    /// Allocate a unique request id.
    pub(crate) fn next_request_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        id
    }

    /// Get a shared reference to the wasmtime engine.
    #[allow(dead_code)]
    pub(crate) fn engine(&self) -> &wasmtime::Engine {
        &self.engine
    }

    /// Send a compilation request to the background thread.
    ///
    /// Starts the background thread if it has not been started yet.
    /// Returns the unique request id assigned to this compilation request.
    pub(crate) fn submit_compile(&mut self, bytes: Vec<u8>) -> u64 {
        let request_id = self.next_request_id();
        self.ensure_thread_started();
        if let Some(sender) = &self.request_sender {
            if let Err(error) = sender.send(WasmRequest::Compile { request_id, bytes }) {
                eprintln!("wasm: failed to send compile request to background thread: {error}");
            }
        }
        request_id
    }

    /// Get the receiver for wasm compilation results, if the thread has been started.
    pub(crate) fn result_receiver(&self) -> Option<&Receiver<WasmResult>> {
        self.result_receiver.as_ref()
    }

    /// Start the background thread if it hasn't been started yet.
    fn ensure_thread_started(&mut self) {
        if self.request_sender.is_some() {
            return;
        }

        let (request_sender, request_receiver) = unbounded::<WasmRequest>();
        let (result_sender, result_receiver) = unbounded::<WasmResult>();

        let engine = self.engine.clone();

        let handle = thread::Builder::new()
            .name("wasm-compiler".to_string())
            .spawn(move || {
                Self::background_thread_loop(engine, request_receiver, result_sender);
            })
            .expect("failed to spawn wasm compilation thread");

        self.request_sender = Some(request_sender);
        self.result_receiver = Some(result_receiver);
        self.handle = Some(handle);
    }

    /// The main loop for the background compilation thread.
    fn background_thread_loop(
        engine: wasmtime::Engine,
        request_receiver: Receiver<WasmRequest>,
        result_sender: Sender<WasmResult>,
    ) {
        loop {
            match request_receiver.recv() {
                Ok(WasmRequest::Compile { request_id, bytes }) => {
                    // Step 2.1: "Compile the WebAssembly module bytes and store the result as module."
                    let result = Module::new(&engine, &bytes);
                    match result {
                        Ok(module) => {
                            if let Err(error) = result_sender.send(WasmResult::Compiled {
                                request_id,
                                module,
                            }) {
                                eprintln!("wasm: failed to send compilation result back: {error}");
                                break;
                            }
                        }
                        Err(error) => {
                            if let Err(send_error) =
                                result_sender.send(WasmResult::CompileError {
                                    request_id,
                                    message: error.to_string(),
                                })
                            {
                                eprintln!("wasm: failed to send compilation error back: {send_error}");
                                break;
                            }
                        }
                    }
                }
                Ok(WasmRequest::Shutdown) | Err(_) => break,
            }
        }
    }
}

impl Drop for WasmThread {
    fn drop(&mut self) {
        if let Some(sender) = &self.request_sender {
            let _ = sender.send(WasmRequest::Shutdown);
        }
    }
}
