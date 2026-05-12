use ipc_messages::{
    content::{Command as ContentCommand, DispatchEventEntry, NavigateRequest},
};
use std::ffi::{CStr, c_char};
use std::panic::{self, AssertUnwindSafe};
use std::time::Duration;
use webview::RuntimeHooks;

fn timer_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_TIMERS").is_some()
}

fn log_timer_debug(message: impl AsRef<str>) {
    if timer_debug_enabled() {
        eprintln!("[timer-debug][ffi] {}", message.as_ref());
    }
}

const DISPATCH_EVENT_BATCH_SEPARATOR: char = '\u{001e}';
const DISPATCH_EVENT_FIELD_SEPARATOR: char = '\u{001f}';

#[repr(C)]
pub struct lean_object {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn formalWebInitializeLeanRuntime() -> *mut lean_object;
    fn formalWebFinalizeLeanRuntime() -> *mut lean_object;
    fn formalWebStartKernel() -> *mut lean_object;
    fn formalWebShutdownKernel() -> *mut lean_object;
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

fn decode_dispatch_event_batch(batch: &str) -> Result<Vec<DispatchEventEntry>, String> {
    if batch.is_empty() {
        return Ok(Vec::new());
    }

    batch
        .split(DISPATCH_EVENT_BATCH_SEPARATOR)
        .map(|entry| {
            let (document_id, event) = entry.split_once(DISPATCH_EVENT_FIELD_SEPARATOR).ok_or_else(
                || format!("malformed dispatch-event batch entry: {entry:?}"),
            )?;
            let document_id = document_id.parse::<u64>().map_err(|error| {
                format!("invalid dispatch-event document id {document_id:?}: {error}")
            })?;
            Ok(DispatchEventEntry {
                document_id,
                event: event.to_owned(),
            })
        })
        .collect()
}

fn runtime_hooks() -> RuntimeHooks {
    RuntimeHooks {
        handle_runtime_request: user_agent::handle_runtime_request,
        start_navigation_request: start_navigation_from_runtime_parts,
    }
}

fn start_navigation_from_runtime_parts(request: NavigateRequest) -> Result<(), String> {
    user_agent::start_navigation_from_rust(request)
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

pub fn install_user_agent_hooks() {
    user_agent::install_hooks();
}

pub fn evaluate_script(
    traversable_id: u64,
    source: String,
    timeout: Duration,
) -> Result<serde_json::Value, String> {
    user_agent::evaluate_script(traversable_id, source, timeout)
}

pub fn start_kernel() -> Result<(), String> {
    install_user_agent_hooks();
    user_agent::start_user_agent_thread()?;
    let io_result = unsafe { formalWebStartKernel() };
    match unit_result_from_lean(io_result, "failed to start the Lean kernel") {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = user_agent::shutdown_user_agent_thread();
            Err(error)
        }
    }
}

pub fn shutdown_kernel() -> Result<(), String> {
    let io_result = unsafe { formalWebShutdownKernel() };
    let shutdown_result =
        unit_result_from_lean(io_result, "failed to shut down the Lean kernel");
    let user_agent_result = user_agent::shutdown_user_agent_thread();
    shutdown_result.and(user_agent_result)
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
        install_user_agent_hooks();
        embedder::run_event_loop(user_agent::runtime_client())
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic running winit event loop"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessStart(event_loop_id: usize) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        install_user_agent_hooks();
        user_agent::start(event_loop_id)
    })) {
        Ok(Ok(handle)) => ok_usize_result(handle),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic starting content"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessStop(handle: usize) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| user_agent::stop(handle))) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic stopping content"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessStopEventLoop(event_loop_id: usize) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| user_agent::stop_event_loop(event_loop_id))) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic stopping content by event loop"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessCreateEmptyDocument(
    handle: usize,
    traversable_id: usize,
    document_id: usize,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        user_agent::send_command(
            handle,
            ContentCommand::CreateEmptyDocument {
                traversable_id: traversable_id as u64,
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
    traversable_id: usize,
    document_id: usize,
    final_url: *mut lean_object,
    status: usize,
    content_type: *mut lean_object,
    body: *mut lean_object,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let status = u16::try_from(status)
            .map_err(|_| format!("loaded document status out of range: {status}"))?;
        let c_final_url = unsafe { leanStringCstr(final_url) };
        let final_url = unsafe { CStr::from_ptr(c_final_url) }
            .to_string_lossy()
            .into_owned();
        let c_content_type = unsafe { leanStringCstr(content_type) };
        let content_type = unsafe { CStr::from_ptr(c_content_type) }
            .to_string_lossy()
            .into_owned();
        let c_body = unsafe { leanStringCstr(body) };
        let body = unsafe { CStr::from_ptr(c_body) }.to_string_lossy().into_owned();
        user_agent::send_command(
            handle,
            ContentCommand::CreateLoadedDocument {
                traversable_id: traversable_id as u64,
                document_id: document_id as u64,
                response: ipc_messages::content::LoadedDocumentResponse {
                    final_url,
                    status,
                    content_type,
                    body,
                },
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic creating loaded content document"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessDestroyDocument(
    handle: usize,
    document_id: usize,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        user_agent::send_command(
            handle,
            ContentCommand::DestroyDocument {
                document_id: document_id as u64,
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic destroying content document"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessDispatchEvent(
    handle: usize,
    batch: *mut lean_object,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let c_batch = unsafe { leanStringCstr(batch) };
        let batch = unsafe { CStr::from_ptr(c_batch) }.to_string_lossy().into_owned();
        let events = decode_dispatch_event_batch(&batch)?;
        user_agent::send_command(
            handle,
            ContentCommand::DispatchEvent { events },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic dispatching content event"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessRunBeforeUnload(
    handle: usize,
    document_id: usize,
    check_id: usize,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        user_agent::send_command(
            handle,
            ContentCommand::RunBeforeUnload {
                document_id: document_id as u64,
                check_id: check_id as u64,
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic running content beforeunload"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessUpdateTheRendering(
    handle: usize,
    traversable_id: usize,
    document_id: usize,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        user_agent::send_command(
            handle,
            ContentCommand::UpdateTheRendering {
                traversable_id: traversable_id as u64,
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
pub extern "C" fn contentProcessRunWindowTimer(
    handle: usize,
    document_id: usize,
    timer_id: usize,
    timer_key: usize,
    nesting_level: usize,
) -> *mut lean_object {
    log_timer_debug(format!(
        "dispatch to content handle={} document={} id={} key={} nesting={}",
        handle, document_id, timer_id, timer_key, nesting_level
    ));
    match panic::catch_unwind(AssertUnwindSafe(|| {
        user_agent::send_command(
            handle,
            ContentCommand::RunWindowTimer {
                document_id: document_id as u64,
                timer_id: timer_id as u32,
                timer_key: timer_key as u64,
                nesting_level: nesting_level as u32,
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic running content window timer"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessFailDocumentFetch(
    handle: usize,
    handler_id: usize,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        user_agent::send_command(
            handle,
            ContentCommand::FailDocumentFetch {
                handler_id: handler_id as u64,
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic failing content fetch"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessCompleteDocumentFetch(
    handle: usize,
    handler_id: usize,
    final_url: *mut lean_object,
    status: usize,
    content_type: *mut lean_object,
    bytes: *mut lean_object,
) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let status = u16::try_from(status)
            .map_err(|_| format!("document fetch status out of range: {status}"))?;
        let c_final_url = unsafe { leanStringCstr(final_url) };
        let final_url = unsafe { CStr::from_ptr(c_final_url) }
            .to_string_lossy()
            .into_owned();
        let c_content_type = unsafe { leanStringCstr(content_type) };
        let content_type = unsafe { CStr::from_ptr(c_content_type) }
            .to_string_lossy()
            .into_owned();
        let size = unsafe { leanByteArraySize(bytes) };
        let bytes_ptr = unsafe { leanByteArrayCptr(bytes) };
        let payload = unsafe { std::slice::from_raw_parts(bytes_ptr, size) };
        user_agent::send_command(
            handle,
            ContentCommand::CompleteDocumentFetch {
                handler_id: handler_id as u64,
                response: ipc_messages::content::FetchResponse {
                    final_url,
                    status,
                    content_type,
                    body: payload.to_vec(),
                },
            },
        )
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic completing content fetch"),
    }
}
