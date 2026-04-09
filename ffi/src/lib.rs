use embedder::RuntimeHooks;
use ipc_messages::content::Command as ContentCommand;
use std::ffi::{CStr, c_char};
use std::panic::{self, AssertUnwindSafe};

#[repr(C)]
pub struct lean_object {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn formalWebInitializeLeanRuntime() -> *mut lean_object;
    fn formalWebFinalizeLeanRuntime() -> *mut lean_object;
    fn formalWebStartKernel() -> *mut lean_object;
    fn formalWebShutdownKernel() -> *mut lean_object;
    fn lean_mk_string_from_bytes(value: *const c_char, size: usize) -> *mut lean_object;
    fn handleRuntimeMessage(message: *mut lean_object) -> *mut lean_object;
    fn startDocumentFetch(
        handler: usize,
        url: *mut lean_object,
        method: *mut lean_object,
        body: *mut lean_object,
    ) -> *mut lean_object;
    fn userAgentNoteRenderingOpportunity(message: *mut lean_object) -> *mut lean_object;
    fn leanIoResultMkOkUnit() -> *mut lean_object;
    fn leanIoResultMkOkUsize(value: usize) -> *mut lean_object;
    fn leanIoResultMkErrorFromBytes(value: *const c_char, size: usize) -> *mut lean_object;
    fn leanIoResultIsOk(result: *mut lean_object) -> u8;
    fn leanIoResultShowError(result: *mut lean_object);
    fn leanStringCstr(value: *mut lean_object) -> *const c_char;
    fn leanByteArraySize(value: *mut lean_object) -> usize;
    fn leanByteArrayCptr(value: *mut lean_object) -> *const u8;
    fn leanDec(value: *mut lean_object);
}

fn lean_string_from_owned(value: String) -> *mut lean_object {
    unsafe { lean_mk_string_from_bytes(value.as_ptr() as *const c_char, value.len()) }
}

fn ok_unit_result() -> *mut lean_object {
    unsafe { leanIoResultMkOkUnit() }
}

fn ok_usize_result(value: usize) -> *mut lean_object {
    unsafe { leanIoResultMkOkUsize(value) }
}

fn error_result(message: &str) -> *mut lean_object {
    unsafe { leanIoResultMkErrorFromBytes(message.as_ptr() as *const c_char, message.len()) }
}

fn unit_result_from_lean(io_result: *mut lean_object, context: &str) -> Result<(), String> {
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
        unsafe { leanDec(io_result) };
        return Err(String::from(context));
    }
    unsafe { leanDec(io_result) };
    Ok(())
}

fn call_lean_runtime_message_handler(message: &str) {
    let lean_message = lean_string_from_owned(message.to_owned());
    let io_result = unsafe { handleRuntimeMessage(lean_message) };
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
    }
    unsafe { leanDec(io_result) };
}

fn call_lean_document_fetch_start_parts(
    handler: usize,
    url: &str,
    method: &str,
    body: &str,
) -> Result<(), String> {
    let lean_url = lean_string_from_owned(url.to_owned());
    let lean_method = lean_string_from_owned(method.to_owned());
    let lean_body = lean_string_from_owned(body.to_owned());
    let io_result = unsafe { startDocumentFetch(handler, lean_url, lean_method, lean_body) };
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
        unsafe { leanDec(io_result) };
        return Err(String::from("Lean document fetch start failed"));
    }
    unsafe { leanDec(io_result) };
    Ok(())
}

fn user_agent_note_rendering_opportunity(message: &str) {
    let lean_message = lean_string_from_owned(message.to_owned());
    let io_result = unsafe { userAgentNoteRenderingOpportunity(lean_message) };
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
    }
    unsafe { leanDec(io_result) };
}

fn runtime_hooks() -> RuntimeHooks {
    RuntimeHooks {
        handle_runtime_message: call_lean_runtime_message_handler,
        start_document_fetch_parts: call_lean_document_fetch_start_parts,
        note_rendering_opportunity: user_agent_note_rendering_opportunity,
    }
}

pub fn initialize_lean_runtime() -> Result<(), String> {
    let io_result = unsafe { formalWebInitializeLeanRuntime() };
    unit_result_from_lean(io_result, "failed to initialize the Lean runtime")
}

pub fn finalize_lean_runtime() -> Result<(), String> {
    let io_result = unsafe { formalWebFinalizeLeanRuntime() };
    unit_result_from_lean(io_result, "failed to finalize the Lean runtime")
}

pub fn install_runtime_hooks() {
    embedder::set_runtime_hooks(runtime_hooks());
}

pub fn start_kernel() -> Result<(), String> {
    let io_result = unsafe { formalWebStartKernel() };
    unit_result_from_lean(io_result, "failed to start the Lean kernel")
}

pub fn shutdown_kernel() -> Result<(), String> {
    let io_result = unsafe { formalWebShutdownKernel() };
    unit_result_from_lean(io_result, "failed to shut down the Lean kernel")
}

#[unsafe(no_mangle)]
pub extern "C" fn sendEmbedderMessage(message: *mut lean_object) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let c_message = unsafe { leanStringCstr(message) };
        let message = unsafe { CStr::from_ptr(c_message) }.to_string_lossy().into_owned();
        embedder::send_runtime_message(&message)
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic sending runtime message"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn runEmbedderEventLoop(_: *mut lean_object) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        install_runtime_hooks();
        embedder::run_event_loop()
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic running winit event loop"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessStart(event_loop_id: usize) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| embedder::start_content(event_loop_id))) {
        Ok(Ok(handle)) => ok_usize_result(handle),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic starting content"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessStop(handle: usize) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| embedder::stop_content(handle))) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic stopping content"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessCreateEmptyDocument(handle: usize, document_id: usize) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        embedder::send_content_command(
            handle,
            ContentCommand::CreateEmptyDocument {
                document_id: document_id as u64,
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic creating content document"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessCreateLoadedDocument(
    handle: usize,
    document_id: usize,
    url: *mut lean_object,
    body: *mut lean_object,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let c_url = unsafe { leanStringCstr(url) };
        let url = unsafe { CStr::from_ptr(c_url) }.to_string_lossy().into_owned();
        let c_body = unsafe { leanStringCstr(body) };
        let body = unsafe { CStr::from_ptr(c_body) }.to_string_lossy().into_owned();
        embedder::send_content_command(
            handle,
            ContentCommand::CreateLoadedDocument {
                document_id: document_id as u64,
                url,
                body,
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic creating loaded content document"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessDispatchEvent(
    handle: usize,
    document_id: usize,
    event: *mut lean_object,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let c_event = unsafe { leanStringCstr(event) };
        let event = unsafe { CStr::from_ptr(c_event) }.to_string_lossy().into_owned();
        embedder::send_content_command(
            handle,
            ContentCommand::DispatchEvent {
                document_id: document_id as u64,
                event,
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic dispatching content event"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessUpdateTheRendering(handle: usize, document_id: usize) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        embedder::send_content_command(
            handle,
            ContentCommand::UpdateTheRendering {
                document_id: document_id as u64,
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic running content update-the-rendering"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessCompleteDocumentFetch(
    handle: usize,
    handler_id: usize,
    resolved_url: *mut lean_object,
    bytes: *mut lean_object,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let c_resolved_url = unsafe { leanStringCstr(resolved_url) };
        let resolved_url = unsafe { CStr::from_ptr(c_resolved_url) }.to_string_lossy().into_owned();
        let size = unsafe { leanByteArraySize(bytes) };
        let bytes_ptr = unsafe { leanByteArrayCptr(bytes) };
        let payload = unsafe { std::slice::from_raw_parts(bytes_ptr, size) };
        embedder::send_content_command(
            handle,
            ContentCommand::CompleteDocumentFetch {
                handler_id: handler_id as u64,
                resolved_url,
                body: payload.to_vec(),
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic completing content fetch"),
    }
}
