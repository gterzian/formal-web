mod content_bridge;

use ipc_messages::content::{Command as ContentCommand, DispatchEventEntry};
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
    fn lean_mk_string_from_bytes(value: *const c_char, size: usize) -> *mut lean_object;
    fn handleRuntimeMessage(message: *mut lean_object) -> *mut lean_object;
    fn startDocumentFetch(
        event_loop_id: usize,
        handler: usize,
        url: *mut lean_object,
        method: *mut lean_object,
        body: *mut lean_object,
    ) -> *mut lean_object;
    fn scheduleWindowTimer(
        event_loop_id: usize,
        document_id: usize,
        timer_id: usize,
        timer_key: usize,
        timeout_ms: usize,
        nesting_level: usize,
    ) -> *mut lean_object;
    fn clearWindowTimer(event_loop_id: usize, timer_key: usize) -> *mut lean_object;
    fn startNavigation(
        source_navigable_id: usize,
        destination_url: *mut lean_object,
        target: *mut lean_object,
        user_involvement: *mut lean_object,
        noopener: usize,
    ) -> *mut lean_object;
    fn startNavigationFromEventLoop(
        event_loop_id: usize,
        source_navigable_id: usize,
        destination_url: *mut lean_object,
        target: *mut lean_object,
        user_involvement: *mut lean_object,
        noopener: usize,
    ) -> *mut lean_object;
    fn completeBeforeUnload(document_id: usize, check_id: usize, canceled: usize)
    -> *mut lean_object;
    fn finalizeNavigation(document_id: usize, url: *mut lean_object) -> *mut lean_object;
    fn removeIframeTraversable(
        parent_traversable_id: usize,
        source_navigable_id: usize,
    ) -> *mut lean_object;
    fn runNextEventLoopTask(event_loop_id: usize) -> *mut lean_object;
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

pub(crate) fn call_lean_document_fetch_start_parts(
    event_loop_id: usize,
    handler: usize,
    url: &str,
    method: &str,
    body: &str,
) -> Result<(), String> {
    let lean_url = lean_string_from_owned(url.to_owned());
    let lean_method = lean_string_from_owned(method.to_owned());
    let lean_body = lean_string_from_owned(body.to_owned());
    let io_result = unsafe {
        startDocumentFetch(event_loop_id, handler, lean_url, lean_method, lean_body)
    };
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
        unsafe { leanDec(io_result) };
        return Err(String::from("Lean document fetch start failed"));
    }
    unsafe { leanDec(io_result) };
    Ok(())
}

pub(crate) fn call_lean_schedule_window_timer_parts(
    event_loop_id: usize,
    document_id: usize,
    timer_id: usize,
    timer_key: usize,
    timeout_ms: usize,
    nesting_level: usize,
) -> Result<(), String> {
    log_timer_debug(format!(
        "schedule lean event_loop={} document={} id={} key={} timeout_ms={} nesting={}",
        event_loop_id, document_id, timer_id, timer_key, timeout_ms, nesting_level
    ));
    let io_result = unsafe {
        scheduleWindowTimer(
            event_loop_id,
            document_id,
            timer_id,
            timer_key,
            timeout_ms,
            nesting_level,
        )
    };
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
        unsafe { leanDec(io_result) };
        return Err(String::from("Lean window timer schedule failed"));
    }
    unsafe { leanDec(io_result) };
    Ok(())
}

pub(crate) fn call_lean_clear_window_timer_parts(
    event_loop_id: usize,
    timer_key: usize,
) -> Result<(), String> {
    log_timer_debug(format!("clear lean event_loop={} key={}", event_loop_id, timer_key));
    let io_result = unsafe { clearWindowTimer(event_loop_id, timer_key) };
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
        unsafe { leanDec(io_result) };
        return Err(String::from("Lean window timer clear failed"));
    }
    unsafe { leanDec(io_result) };
    Ok(())
}

pub(crate) fn call_lean_navigation_start_parts(
    source_navigable_id: usize,
    destination_url: &str,
    target: &str,
    user_involvement: &str,
    noopener: bool,
) -> Result<(), String> {
    let lean_destination_url = lean_string_from_owned(destination_url.to_owned());
    let lean_target = lean_string_from_owned(target.to_owned());
    let lean_user_involvement = lean_string_from_owned(user_involvement.to_owned());
    let io_result = unsafe {
        startNavigation(
            source_navigable_id,
            lean_destination_url,
            lean_target,
            lean_user_involvement,
            usize::from(noopener),
        )
    };
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
        unsafe { leanDec(io_result) };
        return Err(String::from("Lean navigation start failed"));
    }
    unsafe { leanDec(io_result) };
    Ok(())
}

pub(crate) fn call_lean_navigation_start_from_event_loop_parts(
    event_loop_id: usize,
    source_navigable_id: usize,
    destination_url: &str,
    target: &str,
    user_involvement: &str,
    noopener: bool,
) -> Result<(), String> {
    let lean_destination_url = lean_string_from_owned(destination_url.to_owned());
    let lean_target = lean_string_from_owned(target.to_owned());
    let lean_user_involvement = lean_string_from_owned(user_involvement.to_owned());
    let io_result = unsafe {
        startNavigationFromEventLoop(
            event_loop_id,
            source_navigable_id,
            lean_destination_url,
            lean_target,
            lean_user_involvement,
            usize::from(noopener),
        )
    };
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
        unsafe { leanDec(io_result) };
        return Err(String::from("Lean event-loop navigation start failed"));
    }
    unsafe { leanDec(io_result) };
    Ok(())
}

pub(crate) fn call_lean_before_unload_completed_parts(
    document_id: usize,
    check_id: usize,
    canceled: bool,
) -> Result<(), String> {
    let io_result = unsafe { completeBeforeUnload(document_id, check_id, usize::from(canceled)) };
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
        unsafe { leanDec(io_result) };
        return Err(String::from("Lean beforeunload completion failed"));
    }
    unsafe { leanDec(io_result) };
    Ok(())
}

pub(crate) fn call_lean_finalize_navigation_parts(
    document_id: usize,
    url: &str,
) -> Result<(), String> {
    let lean_url = lean_string_from_owned(url.to_owned());
    let io_result = unsafe { finalizeNavigation(document_id, lean_url) };
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
        unsafe { leanDec(io_result) };
        return Err(String::from("Lean finalize-navigation failed"));
    }
    unsafe { leanDec(io_result) };
    Ok(())
}

pub(crate) fn call_lean_remove_iframe_traversable_parts(
    parent_traversable_id: usize,
    source_navigable_id: usize,
) -> Result<(), String> {
    let io_result = unsafe {
        removeIframeTraversable(parent_traversable_id, source_navigable_id)
    };
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
        unsafe { leanDec(io_result) };
        return Err(String::from("Lean iframe traversable removal failed"));
    }
    unsafe { leanDec(io_result) };
    Ok(())
}

pub(crate) fn call_lean_run_next_event_loop_task(event_loop_id: usize) -> Result<(), String> {
    let io_result = unsafe { runNextEventLoopTask(event_loop_id) };
    let is_ok = unsafe { leanIoResultIsOk(io_result) } != 0;
    if !is_ok {
        unsafe { leanIoResultShowError(io_result) };
        unsafe { leanDec(io_result) };
        return Err(String::from("Lean run-next-event-loop-task failed"));
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
        start_navigation_parts: call_lean_navigation_start_parts,
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

pub fn install_content_bridge_hooks() {
    content_bridge::install_hooks();
}

pub fn evaluate_script(
    traversable_id: u64,
    source: String,
    timeout: Duration,
) -> Result<serde_json::Value, String> {
    content_bridge::evaluate_script(traversable_id, source, timeout)
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
        install_content_bridge_hooks();
        embedder::run_event_loop()
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic running winit event loop"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessStart(event_loop_id: usize) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        install_content_bridge_hooks();
        content_bridge::start(event_loop_id)
    })) {
        Ok(Ok(handle)) => ok_usize_result(handle),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic starting content"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessStop(handle: usize) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| content_bridge::stop(handle))) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic stopping content"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn contentProcessStopEventLoop(event_loop_id: usize) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| content_bridge::stop_event_loop(event_loop_id))) {
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
        content_bridge::send_command(
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
        content_bridge::send_command(
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
        content_bridge::send_command(
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
        content_bridge::send_command(
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
        content_bridge::send_command(
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
        content_bridge::send_command(
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
        content_bridge::send_command(
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
        content_bridge::send_command(
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
        content_bridge::send_command(
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
