mod event_loop;
mod fetch;
mod timer;

use blitz_traits::shell::ColorScheme;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use embedder::{FinalizeNavigation, FormalWebUserEvent};
use ipc_messages::{
    content::{
        BeforeUnloadResult, Command as ContentCommand, DispatchEventEntry,
        FetchResponse as ContentFetchResponse, FinalizeNavigation as ContentFinalizeNavigation,
        LoadedDocumentResponse, NavigateRequest, WebviewId,
    },
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::event_loop::{
    EventLoopCommand, EventLoopEntry, destroyed_document_id, document_id_from_command,
    spawn_event_loop_entry, stop_event_loop_entry, traversable_viewport_command,
    viewport_command,
};
use crate::fetch::{FetchCommand, run_fetch_thread};
use crate::timer::{TimerCommand, run_timer_thread};

pub struct UserAgentState {
    pub next_handle: usize,
    pub next_event_loop_id: usize,
    pub next_traversable_id: u64,
    pub next_document_id: u64,
    pub next_navigation_id: u64,
    pub next_before_unload_check_id: u64,
    pub event_loops: HashMap<usize, EventLoopEntry>,
    pub handles_by_event_loop_id: HashMap<usize, usize>,
    pub traversable_handles: HashMap<u64, usize>,
    pub traversable_target_names: HashMap<u64, String>,
    pub active_documents_by_traversable: HashMap<u64, u64>,
    pub known_child_navigables: HashMap<u64, u64>,
    pub documents: HashMap<u64, DocumentState>,
    pub pending_before_unload_navigations: HashMap<u64, PendingBeforeUnloadNavigation>,
    pub pending_navigation_fetches: HashMap<u64, PendingNavigationFetch>,
    pub pending_navigation_finalizations: HashMap<u64, PendingNavigationFinalization>,
}

#[derive(Clone)]
pub struct DocumentState {
    pub traversable_id: u64,
    pub url: String,
    pub is_initial_about_blank: bool,
}

#[derive(Clone)]
pub struct PendingBeforeUnloadNavigation {
    pub traversable_id: u64,
    pub document_id: u64,
    pub destination_url: String,
}

#[derive(Clone)]
pub struct PendingNavigationFetch {
    pub traversable_id: u64,
    pub previous_document_id: Option<u64>,
    pub destination_url: String,
}

#[derive(Clone)]
pub struct PendingNavigationFinalization {
    pub traversable_id: u64,
    pub previous_document_id: Option<u64>,
    pub url: String,
}

impl Default for UserAgentState {
    fn default() -> Self {
        Self {
            next_handle: 1,
            next_event_loop_id: 1,
            next_traversable_id: 1,
            next_document_id: 1,
            next_navigation_id: 1,
            next_before_unload_check_id: 1,
            event_loops: HashMap::new(),
            handles_by_event_loop_id: HashMap::new(),
            traversable_handles: HashMap::new(),
            traversable_target_names: HashMap::new(),
            active_documents_by_traversable: HashMap::new(),
            known_child_navigables: HashMap::new(),
            documents: HashMap::new(),
            pending_before_unload_navigations: HashMap::new(),
            pending_navigation_fetches: HashMap::new(),
            pending_navigation_finalizations: HashMap::new(),
        }
    }
}

pub enum UserAgentCommand {
    StartTopLevelTraversable {
        destination_url: String,
        reply: Sender<Result<(), String>>,
    },
    QueueTopLevelTraversable {
        destination_url: String,
    },
    StartNavigation {
        request: NavigateRequest,
        reply: Sender<Result<(), String>>,
    },
    QueueNavigation {
        request: NavigateRequest,
    },
    CompleteBeforeUnload {
        result: BeforeUnloadResult,
        reply: Sender<Result<(), String>>,
    },
    QueueCompleteBeforeUnload {
        result: BeforeUnloadResult,
    },
    FinalizeNavigation {
        finalized: ContentFinalizeNavigation,
        reply: Sender<Result<(), String>>,
    },
    QueueFinalizeNavigation {
        finalized: ContentFinalizeNavigation,
    },
    StartEventLoop {
        event_loop_id: usize,
        reply: Sender<Result<usize, String>>,
    },
    StopHandle {
        handle: usize,
        reply: Sender<Result<(), String>>,
    },
    StopEventLoop {
        event_loop_id: usize,
        reply: Sender<Result<(), String>>,
    },
    SendCommand {
        handle: usize,
        command: ContentCommand,
        reply: Sender<Result<(), String>>,
    },
    EvaluateScript {
        traversable_id: u64,
        source: String,
        timeout: Duration,
        reply: Sender<Result<serde_json::Value, String>>,
    },
    BroadcastViewport {
        snapshot: (u32, u32, f32, ColorScheme),
    },
    SetTraversableViewport {
        traversable_id: u64,
        snapshot: (u32, u32, f32, ColorScheme),
        offset_x: f32,
        offset_y: f32,
    },
    DispatchEventFor {
        traversable_id: u64,
        event: String,
    },
    RenderingOpportunityFor {
        traversable_id: u64,
    },
    DocumentFetchCompleted {
        event_loop_id: usize,
        handler_id: u64,
        response: ContentFetchResponse,
    },
    DocumentFetchFailed {
        event_loop_id: usize,
        handler_id: u64,
    },
    NavigationFetchCompleted {
        navigation_id: u64,
        response: ContentFetchResponse,
    },
    NavigationFetchFailed {
        navigation_id: u64,
    },
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
    IframeTraversableRemoved {
        parent_traversable_id: u64,
        content_navigable_id: u64,
        reply: Sender<Result<(), String>>,
    },
    ChildNavigableCreated {
        parent_traversable_id: u64,
        content_navigable_id: u64,
        reply: Sender<Result<(), String>>,
    },
    Shutdown {
        reply: Sender<Result<(), String>>,
    },
}

pub static NEXT_SCRIPT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub struct UserAgent {
    handle: UserAgentHandle,
    join_handle: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct UserAgentHandle {
    command_sender: Sender<UserAgentCommand>,
}

impl UserAgent {
    pub fn start() -> Result<Self, String> {
        let (command_sender, command_receiver) = unbounded();
        let handle = UserAgentHandle {
            command_sender: command_sender.clone(),
        };
        let mut worker = UserAgentWorker::new(command_sender, command_receiver);
        let join_handle = thread::spawn(move || worker.run());
        Ok(Self {
            handle,
            join_handle: Some(join_handle),
        })
    }

    pub fn handle(&self) -> UserAgentHandle {
        self.handle.clone()
    }

    pub fn shutdown(mut self) -> Result<(), String> {
        let (reply_sender, reply_receiver) = bounded(1);
        self.handle
            .command_sender
            .send(UserAgentCommand::Shutdown {
                reply: reply_sender,
            })
            .map_err(|error| format!("failed to request user-agent shutdown: {error}"))?;
        let shutdown_result = reply_receiver
            .recv()
            .map_err(|error| format!("user-agent shutdown reply channel closed: {error}"))?;

        if let Some(join_handle) = self.join_handle.take()
            && join_handle.join().is_err()
            && shutdown_result.is_ok()
        {
            return Err(String::from("user-agent thread panicked"));
        }

        shutdown_result
    }

    pub fn evaluate_script(
        &self,
        traversable_id: u64,
        source: String,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        webview::UserAgentApi::evaluate_script(&self.handle, traversable_id, source, timeout)
    }
}

impl webview::UserAgentApi for UserAgentHandle {
    fn start_top_level_traversable(&self, destination_url: String) -> Result<(), String> {
        let (reply_sender, reply_receiver) = bounded(1);
        self.command_sender
            .send(UserAgentCommand::StartTopLevelTraversable {
                destination_url,
                reply: reply_sender,
            })
            .map_err(|error| format!("failed to start top-level traversable: {error}"))?;
        reply_receiver
            .recv()
            .map_err(|error| format!("top-level traversable reply channel closed: {error}"))?
    }

    fn start_navigation(&self, request: NavigateRequest) -> Result<(), String> {
        let (reply_sender, reply_receiver) = bounded(1);
        self.command_sender
            .send(UserAgentCommand::StartNavigation {
                request,
                reply: reply_sender,
            })
            .map_err(|error| format!("failed to start navigation: {error}"))?;
        reply_receiver
            .recv()
            .map_err(|error| format!("navigation reply channel closed: {error}"))?
    }

    fn dispatch_event_for(&self, traversable_id: u64, event: String) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::DispatchEventFor {
                traversable_id,
                event,
            })
            .map_err(|error| format!("failed to queue dispatch-event request: {error}"))
    }

    fn note_rendering_opportunity(&self, traversable_id: u64) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::RenderingOpportunityFor { traversable_id })
            .map_err(|error| format!("failed to queue rendering-opportunity request: {error}"))
    }

    fn set_default_viewport(
        &self,
        snapshot: Option<(u32, u32, f32, ColorScheme)>,
    ) -> Result<(), String> {
        let Some(snapshot) = snapshot else {
            return Ok(());
        };
        self.command_sender
            .send(UserAgentCommand::BroadcastViewport { snapshot })
            .map_err(|error| format!("failed to broadcast viewport: {error}"))
    }

    fn set_traversable_viewport(
        &self,
        traversable_id: u64,
        snapshot: (u32, u32, f32, ColorScheme),
        offset_x: f32,
        offset_y: f32,
    ) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::SetTraversableViewport {
                traversable_id,
                snapshot,
                offset_x,
                offset_y,
            })
            .map_err(|error| format!("failed to set traversable viewport: {error}"))
    }

    fn evaluate_script(
        &self,
        traversable_id: u64,
        source: String,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        let (reply_sender, reply_receiver) = bounded(1);
        self.command_sender
            .send(UserAgentCommand::EvaluateScript {
                traversable_id,
                source,
                timeout,
                reply: reply_sender,
            })
            .map_err(|error| format!("failed to send script evaluation request: {error}"))?;
        reply_receiver
            .recv()
            .map_err(|error| format!("script evaluation reply channel closed: {error}"))?
    }
}

fn render_state_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_RENDER_STATE").is_some()
}

fn log_render_state_debug(message: impl AsRef<str>) {
    if render_state_debug_enabled() {
        eprintln!("[render-state][user-agent] {}", message.as_ref());
    }
}

fn allocate_event_loop_id(state: &mut UserAgentState) -> usize {
    let event_loop_id = state.next_event_loop_id;
    state.next_event_loop_id += 1;
    event_loop_id
}

fn allocate_traversable_id(state: &mut UserAgentState) -> u64 {
    let traversable_id = state.next_traversable_id;
    state.next_traversable_id += 1;
    traversable_id
}

fn allocate_document_id(state: &mut UserAgentState) -> u64 {
    let document_id = state.next_document_id;
    state.next_document_id += 1;
    document_id
}

fn allocate_navigation_id(state: &mut UserAgentState) -> u64 {
    let navigation_id = state.next_navigation_id;
    state.next_navigation_id += 1;
    navigation_id
}

fn allocate_before_unload_check_id(state: &mut UserAgentState) -> u64 {
    let check_id = state.next_before_unload_check_id;
    state.next_before_unload_check_id += 1;
    check_id
}

fn normalize_navigation_target_name(target_name: &str) -> String {
    if target_name.eq_ignore_ascii_case("_self") {
        String::new()
    } else {
        target_name.to_owned()
    }
}

fn send_event_loop_command(
    command_sender: &Sender<EventLoopCommand>,
    command: ContentCommand,
) -> Result<Option<u64>, String> {
    let (reply_sender, reply_receiver) = bounded(1);
    command_sender
        .send(EventLoopCommand::SendCommand {
            command,
            reply: reply_sender,
        })
        .map_err(|error| format!("failed to send event-loop command: {error}"))?;
    reply_receiver
        .recv()
        .map_err(|error| format!("event-loop command reply channel closed: {error}"))?
}

fn create_or_get_event_loop_handle(
    state: &mut UserAgentState,
    event_loop_id: usize,
    user_agent_command_sender: &Sender<UserAgentCommand>,
    fetch_command_sender: &Sender<FetchCommand>,
    timer_command_sender: &Sender<TimerCommand>,
) -> Result<usize, String> {
    if let Some(handle) = state.handles_by_event_loop_id.get(&event_loop_id).copied() {
        state.next_event_loop_id = state.next_event_loop_id.max(event_loop_id + 1);
        return Ok(handle);
    }

    state.next_event_loop_id = state.next_event_loop_id.max(event_loop_id + 1);
    let handle = state.next_handle;
    state.next_handle += 1;
    let entry = spawn_event_loop_entry(
        event_loop_id,
        user_agent_command_sender.clone(),
        fetch_command_sender.clone(),
        timer_command_sender.clone(),
    )?;
    state.handles_by_event_loop_id.insert(event_loop_id, handle);
    state.event_loops.insert(handle, entry);
    Ok(handle)
}

fn command_sender_for_traversable(
    state: &UserAgentState,
    traversable_id: u64,
) -> Result<Sender<EventLoopCommand>, String> {
    let handle = state
        .traversable_handles
        .get(&traversable_id)
        .copied()
        .ok_or_else(|| format!("unknown traversable id: {traversable_id}"))?;
    state
        .event_loops
        .get(&handle)
        .map(|entry| entry.command_sender.clone())
        .ok_or_else(|| format!("missing event loop for handle {handle}"))
}

fn find_traversable_by_target_name(state: &UserAgentState, target_name: &str) -> Option<u64> {
    state
        .traversable_target_names
        .iter()
        .find_map(|(traversable_id, traversable_target_name)| {
            (traversable_target_name == target_name).then_some(*traversable_id)
        })
}

fn create_top_level_traversable(
    state: &mut UserAgentState,
    target_name: String,
    user_agent_command_sender: &Sender<UserAgentCommand>,
    fetch_command_sender: &Sender<FetchCommand>,
    timer_command_sender: &Sender<TimerCommand>,
) -> Result<u64, String> {
    let traversable_id = allocate_traversable_id(state);
    let event_loop_id = allocate_event_loop_id(state);
    let document_id = allocate_document_id(state);
    let handle = create_or_get_event_loop_handle(
        state,
        event_loop_id,
        user_agent_command_sender,
        fetch_command_sender,
        timer_command_sender,
    )?;
    let command_sender = state
        .event_loops
        .get(&handle)
        .map(|entry| entry.command_sender.clone())
        .ok_or_else(|| format!("missing event loop entry for handle {handle}"))?;

    send_event_loop_command(
        &command_sender,
        ContentCommand::CreateEmptyDocument {
            traversable_id,
            document_id,
        },
    )?;

    state
        .event_loops
        .get_mut(&handle)
        .expect("event loop entry disappeared during top-level creation")
        .traversable_ids
        .insert(traversable_id);
    state.traversable_handles.insert(traversable_id, handle);
    state
        .traversable_target_names
        .insert(traversable_id, target_name.clone());
    state
        .active_documents_by_traversable
        .insert(traversable_id, document_id);
    state.documents.insert(
        document_id,
        DocumentState {
            traversable_id,
            url: String::from("about:blank"),
            is_initial_about_blank: true,
        },
    );

    embedder::send_user_event(FormalWebUserEvent::NewTopLevelTraversable(
        WebviewId(traversable_id),
        target_name,
    ))?;
    Ok(traversable_id)
}

fn clear_pending_navigation_for_traversable(state: &mut UserAgentState, traversable_id: u64) {
    state
        .pending_before_unload_navigations
        .retain(|_, pending| pending.traversable_id != traversable_id);
    state
        .pending_navigation_fetches
        .retain(|_, pending| pending.traversable_id != traversable_id);

    let stale_document_ids = state
        .pending_navigation_finalizations
        .iter()
        .filter_map(|(document_id, pending)| {
            (pending.traversable_id == traversable_id).then_some(*document_id)
        })
        .collect::<Vec<_>>();
    let command_sender = command_sender_for_traversable(state, traversable_id).ok();

    for document_id in stale_document_ids {
        if let Some(command_sender) = command_sender.as_ref() {
            let _ = send_event_loop_command(
                command_sender,
                ContentCommand::DestroyDocument { document_id },
            );
        }
        state.pending_navigation_finalizations.remove(&document_id);
        state.documents.remove(&document_id);
    }
}

fn notify_navigation_failed(traversable_id: u64, message: String) {
    let _ = embedder::send_user_event(FormalWebUserEvent::NavigationFailed {
        webview_id: WebviewId(traversable_id),
        message,
    });
}

fn start_navigation_fetch(
    state: &mut UserAgentState,
    fetch_command_sender: &Sender<FetchCommand>,
    traversable_id: u64,
    destination_url: String,
) -> Result<(), String> {
    clear_pending_navigation_for_traversable(state, traversable_id);

    let navigation_id = allocate_navigation_id(state);
    let previous_document_id = state.active_documents_by_traversable.get(&traversable_id).copied();
    state.pending_navigation_fetches.insert(
        navigation_id,
        PendingNavigationFetch {
            traversable_id,
            previous_document_id,
            destination_url: destination_url.clone(),
        },
    );

    if let Err(error) = fetch_command_sender.send(FetchCommand::StartNavigationFetch {
        navigation_id,
        request: ipc_messages::content::FetchRequest {
            handler_id: navigation_id,
            url: destination_url,
            method: String::from("GET"),
            body: String::new(),
        },
    }) {
        state.pending_navigation_fetches.remove(&navigation_id);
        return Err(format!("failed to start navigation fetch: {error}"));
    }

    Ok(())
}

fn begin_navigation_for_traversable(
    state: &mut UserAgentState,
    fetch_command_sender: &Sender<FetchCommand>,
    traversable_id: u64,
    destination_url: String,
) -> Result<(), String> {
    clear_pending_navigation_for_traversable(state, traversable_id);

    let active_document_id = state.active_documents_by_traversable.get(&traversable_id).copied();
    let should_run_before_unload = active_document_id
        .and_then(|document_id| state.documents.get(&document_id))
        .is_some_and(|document| !document.is_initial_about_blank);

    if should_run_before_unload {
        let document_id = active_document_id.expect("beforeunload document id disappeared");
        let check_id = allocate_before_unload_check_id(state);
        state.pending_before_unload_navigations.insert(
            check_id,
            PendingBeforeUnloadNavigation {
                traversable_id,
                document_id,
                destination_url,
            },
        );
        let command_sender = command_sender_for_traversable(state, traversable_id)?;
        if let Err(error) = send_event_loop_command(
            &command_sender,
            ContentCommand::RunBeforeUnload {
                document_id,
                check_id,
            },
        ) {
            state.pending_before_unload_navigations.remove(&check_id);
            return Err(error);
        }
        Ok(())
    } else {
        start_navigation_fetch(state, fetch_command_sender, traversable_id, destination_url)
    }
}

/// Continue a navigation after the queued `beforeunload` task has resolved.
///
/// This is the Rust continuation for the HTML navigate algorithm's
/// `checking if unloading is canceled` step. Once the document reports that
/// `beforeunload` did not cancel the navigation, the next step is to continue
/// into fetch-backed navigation, not to queue `beforeunload` again.
/// Spec: https://html.spec.whatwg.org/multipage/document-lifecycle.html#checking-if-unloading-is-canceled
fn continue_navigation_after_before_unload(
    state: &mut UserAgentState,
    fetch_command_sender: &Sender<FetchCommand>,
    pending: PendingBeforeUnloadNavigation,
) -> Result<(), String> {
    start_navigation_fetch(
        state,
        fetch_command_sender,
        pending.traversable_id,
        pending.destination_url,
    )
}

fn resolve_target_traversable(
    state: &mut UserAgentState,
    source_navigable_id: u64,
    target_name: &str,
    noopener: bool,
    user_agent_command_sender: &Sender<UserAgentCommand>,
    fetch_command_sender: &Sender<FetchCommand>,
    timer_command_sender: &Sender<TimerCommand>,
) -> Result<u64, String> {
    let normalized_target_name = normalize_navigation_target_name(target_name);
    if noopener || normalized_target_name.eq_ignore_ascii_case("_blank") {
        return create_top_level_traversable(
            state,
            String::new(),
            user_agent_command_sender,
            fetch_command_sender,
            timer_command_sender,
        );
    }

    if normalized_target_name.is_empty() {
        if let Some(parent_traversable_id) = state.known_child_navigables.get(&source_navigable_id)
        {
            let iframe_name = iframe_target_name(*parent_traversable_id, source_navigable_id);
            if let Some(traversable_id) = find_traversable_by_target_name(state, &iframe_name) {
                return Ok(traversable_id);
            }
            return create_top_level_traversable(
                state,
                iframe_name,
                user_agent_command_sender,
                fetch_command_sender,
                timer_command_sender,
            );
        }

        if state.traversable_handles.contains_key(&source_navigable_id) {
            return Ok(source_navigable_id);
        }

        return create_top_level_traversable(
            state,
            String::new(),
            user_agent_command_sender,
            fetch_command_sender,
            timer_command_sender,
        );
    }

    if let Some(traversable_id) = find_traversable_by_target_name(state, &normalized_target_name)
    {
        return Ok(traversable_id);
    }

    create_top_level_traversable(
        state,
        normalized_target_name,
        user_agent_command_sender,
        fetch_command_sender,
        timer_command_sender,
    )
}

fn iframe_target_name(parent_traversable_id: u64, content_navigable_id: u64) -> String {
    format!("_iframe|{parent_traversable_id}|{content_navigable_id}")
}

fn remove_event_loop_entry(state: &mut UserAgentState, handle: usize) -> Option<EventLoopEntry> {
    let entry = state.event_loops.remove(&handle)?;
    state.handles_by_event_loop_id.remove(&entry.event_loop_id);
    let removed_traversable_ids = entry.traversable_ids.iter().copied().collect::<Vec<_>>();
    for traversable_id in &entry.traversable_ids {
        state.traversable_handles.remove(traversable_id);
        state.traversable_target_names.remove(traversable_id);
        state.active_documents_by_traversable.remove(traversable_id);
    }
    state.documents.retain(|_, document| {
        !removed_traversable_ids.contains(&document.traversable_id)
    });
    state.pending_before_unload_navigations.retain(|_, pending| {
        !removed_traversable_ids.contains(&pending.traversable_id)
    });
    state.pending_navigation_fetches.retain(|_, pending| {
        !removed_traversable_ids.contains(&pending.traversable_id)
    });
    state.pending_navigation_finalizations.retain(|_, pending| {
        !removed_traversable_ids.contains(&pending.traversable_id)
    });
    Some(entry)
}

fn stop_event_loop_handle(state: &mut UserAgentState, handle: usize) -> Result<(), String> {
    match remove_event_loop_entry(state, handle) {
        Some(entry) => stop_event_loop_entry(entry),
        None => Ok(()),
    }
}

struct UserAgentWorker {
    state: UserAgentState,
    command_sender: Sender<UserAgentCommand>,
    command_receiver: Receiver<UserAgentCommand>,
    fetch_command_sender: Sender<FetchCommand>,
    fetch_join_handle: Option<JoinHandle<()>>,
    timer_command_sender: Sender<TimerCommand>,
    timer_join_handle: Option<JoinHandle<()>>,
}

impl UserAgentWorker {
    fn new(
        user_agent_command_sender: Sender<UserAgentCommand>,
        command_receiver: Receiver<UserAgentCommand>,
    ) -> Self {
        let (fetch_command_sender, fetch_command_receiver) = unbounded();
        let fetch_user_agent_command_sender = user_agent_command_sender.clone();
        let fetch_join_handle = thread::spawn(move || {
            run_fetch_thread(fetch_command_receiver, fetch_user_agent_command_sender)
        });
        let (timer_command_sender, timer_command_receiver) = unbounded();
        let timer_user_agent_command_sender = user_agent_command_sender.clone();
        let timer_join_handle = thread::spawn(move || {
            run_timer_thread(timer_command_receiver, timer_user_agent_command_sender)
        });

        Self {
            state: UserAgentState::default(),
            command_sender: user_agent_command_sender,
            command_receiver,
            fetch_command_sender,
            fetch_join_handle: Some(fetch_join_handle),
            timer_command_sender,
            timer_join_handle: Some(timer_join_handle),
        }
    }

    fn run(&mut self) {
        while let Ok(command) = self.command_receiver.recv() {
            match command {
            UserAgentCommand::StartTopLevelTraversable {
                destination_url,
                reply,
            } => {
                let result = (|| {
                    let traversable_id = create_top_level_traversable(
                        &mut self.state,
                        String::new(),
                        &self.command_sender,
                        &self.fetch_command_sender,
                        &self.timer_command_sender,
                    )?;
                    begin_navigation_for_traversable(
                        &mut self.state,
                        &self.fetch_command_sender,
                        traversable_id,
                        destination_url,
                    )
                })();
                let _ = reply.send(result);
            }
            UserAgentCommand::QueueTopLevelTraversable { destination_url } => {
                let result = (|| {
                    let traversable_id = create_top_level_traversable(
                        &mut self.state,
                        String::new(),
                        &self.command_sender,
                        &self.fetch_command_sender,
                        &self.timer_command_sender,
                    )?;
                    begin_navigation_for_traversable(
                        &mut self.state,
                        &self.fetch_command_sender,
                        traversable_id,
                        destination_url,
                    )
                })();
                if let Err(error) = result {
                    eprintln!("failed to queue top-level traversable start: {error}");
                }
            }
            UserAgentCommand::StartNavigation { request, reply } => {
                let destination_url = request.destination_url.clone();
                let result = (|| {
                    let traversable_id = resolve_target_traversable(
                        &mut self.state,
                        request.source_navigable_id,
                        &request.target,
                        request.noopener,
                        &self.command_sender,
                        &self.fetch_command_sender,
                        &self.timer_command_sender,
                    )?;
                    begin_navigation_for_traversable(
                        &mut self.state,
                        &self.fetch_command_sender,
                        traversable_id,
                        destination_url.clone(),
                    )?;
                    embedder::send_user_event(FormalWebUserEvent::NavigationRequested {
                        webview_id: WebviewId(traversable_id),
                        destination_url,
                    })
                })();
                let _ = reply.send(result);
            }
            UserAgentCommand::QueueNavigation { request } => {
                let destination_url = request.destination_url.clone();
                let result = (|| {
                    let traversable_id = resolve_target_traversable(
                        &mut self.state,
                        request.source_navigable_id,
                        &request.target,
                        request.noopener,
                        &self.command_sender,
                        &self.fetch_command_sender,
                        &self.timer_command_sender,
                    )?;
                    begin_navigation_for_traversable(
                        &mut self.state,
                        &self.fetch_command_sender,
                        traversable_id,
                        destination_url.clone(),
                    )?;
                    embedder::send_user_event(FormalWebUserEvent::NavigationRequested {
                        webview_id: WebviewId(traversable_id),
                        destination_url,
                    })
                })();
                if let Err(error) = result {
                    eprintln!("failed to queue navigation: {error}");
                }
            }
            UserAgentCommand::CompleteBeforeUnload { result, reply } => {
                let completion = if let Some(pending) = self.state
                    .pending_before_unload_navigations
                    .remove(&result.check_id)
                {
                    if pending.document_id == result.document_id {
                        if result.canceled {
                            embedder::send_user_event(FormalWebUserEvent::BeforeUnloadCompleted(
                                result,
                            ))
                        } else {
                            continue_navigation_after_before_unload(
                                &mut self.state,
                                &self.fetch_command_sender,
                                pending,
                            )
                        }
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                };
                let _ = reply.send(completion);
            }
            UserAgentCommand::QueueCompleteBeforeUnload { result } => {
                let completion = if let Some(pending) = self.state
                    .pending_before_unload_navigations
                    .remove(&result.check_id)
                {
                    if pending.document_id == result.document_id {
                        if result.canceled {
                            embedder::send_user_event(FormalWebUserEvent::BeforeUnloadCompleted(
                                result,
                            ))
                        } else {
                            continue_navigation_after_before_unload(
                                &mut self.state,
                                &self.fetch_command_sender,
                                pending,
                            )
                        }
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                };
                if let Err(error) = completion {
                    eprintln!("failed to complete queued beforeunload: {error}");
                }
            }
            UserAgentCommand::FinalizeNavigation { finalized, reply } => {
                let result = if let Some(pending) = self.state
                    .pending_navigation_finalizations
                    .remove(&finalized.document_id)
                {
                    self.state
                        .active_documents_by_traversable
                        .insert(pending.traversable_id, finalized.document_id);
                    if let Some(document) = self.state.documents.get_mut(&finalized.document_id) {
                        document.url = finalized.url.clone();
                        document.is_initial_about_blank = finalized.url == "about:blank";
                    }
                    let notify_result = embedder::send_user_event(
                        FormalWebUserEvent::FinalizeNavigation(FinalizeNavigation {
                            webview_id: WebviewId(pending.traversable_id),
                            url: finalized.url.clone(),
                        }),
                    );

                    if let Some(previous_document_id) = pending.previous_document_id {
                        if previous_document_id != finalized.document_id {
                            if let Ok(command_sender) =
                                command_sender_for_traversable(&self.state, pending.traversable_id)
                            {
                                let _ = send_event_loop_command(
                                    &command_sender,
                                    ContentCommand::DestroyDocument {
                                        document_id: previous_document_id,
                                    },
                                );
                            }
                            self.state.documents.remove(&previous_document_id);
                        }
                    }

                    notify_result
                } else {
                    Ok(())
                };
                let _ = reply.send(result);
            }
            UserAgentCommand::QueueFinalizeNavigation { finalized } => {
                let result = if let Some(pending) = self.state
                    .pending_navigation_finalizations
                    .remove(&finalized.document_id)
                {
                    self.state
                        .active_documents_by_traversable
                        .insert(pending.traversable_id, finalized.document_id);
                    if let Some(document) = self.state.documents.get_mut(&finalized.document_id) {
                        document.url = finalized.url.clone();
                        document.is_initial_about_blank = finalized.url == "about:blank";
                    }
                    let notify_result = embedder::send_user_event(
                        FormalWebUserEvent::FinalizeNavigation(FinalizeNavigation {
                            webview_id: WebviewId(pending.traversable_id),
                            url: finalized.url.clone(),
                        }),
                    );

                    if let Some(previous_document_id) = pending.previous_document_id {
                        if previous_document_id != finalized.document_id {
                            if let Ok(command_sender) =
                                command_sender_for_traversable(&self.state, pending.traversable_id)
                            {
                                let _ = send_event_loop_command(
                                    &command_sender,
                                    ContentCommand::DestroyDocument {
                                        document_id: previous_document_id,
                                    },
                                );
                            }
                            self.state.documents.remove(&previous_document_id);
                        }
                    }

                    notify_result
                } else {
                    Ok(())
                };
                if let Err(error) = result {
                    eprintln!("failed to finalize queued navigation: {error}");
                }
            }
            UserAgentCommand::StartEventLoop {
                event_loop_id,
                reply,
            } => {
                let result = if let Some(handle) = self.state.handles_by_event_loop_id.get(&event_loop_id)
                {
                    Ok(*handle)
                } else {
                    match create_or_get_event_loop_handle(
                        &mut self.state,
                        event_loop_id,
                        &self.command_sender,
                        &self.fetch_command_sender,
                        &self.timer_command_sender,
                    ) {
                        Ok(handle) => Ok(handle),
                        Err(error) => Err(error),
                    }
                };
                let _ = reply.send(result);
            }
            UserAgentCommand::StopHandle { handle, reply } => {
                let result = stop_event_loop_handle(&mut self.state, handle);
                let _ = reply.send(result);
            }
            UserAgentCommand::StopEventLoop {
                event_loop_id,
                reply,
            } => {
                let result = match self.state.handles_by_event_loop_id.get(&event_loop_id).copied() {
                    Some(handle) => stop_event_loop_handle(&mut self.state, handle),
                    None => Ok(()),
                };
                let _ = reply.send(result);
            }
            UserAgentCommand::SendCommand {
                handle,
                command,
                reply,
            } => {
                let result = match self.state.event_loops.get_mut(&handle) {
                    Some(entry) => {
                        let tracked_command = command.clone();
                        let (event_loop_reply_sender, event_loop_reply_receiver) = bounded(1);
                        let send_result = entry.command_sender.send(EventLoopCommand::SendCommand {
                            command,
                            reply: event_loop_reply_sender,
                        });
                        match send_result {
                            Ok(()) => match event_loop_reply_receiver.recv() {
                                Ok(Ok(traversable_id)) => {
                                    if let Some(traversable_id) = traversable_id {
                                        entry.traversable_ids.insert(traversable_id);
                                        self.state.traversable_handles.insert(traversable_id, handle);
                                        self.state.next_traversable_id =
                                            self.state.next_traversable_id.max(traversable_id + 1);
                                        if let Some(document_id) = document_id_from_command(&tracked_command) {
                                            self.state.next_document_id =
                                                self.state.next_document_id.max(document_id + 1);
                                            self.state
                                                .active_documents_by_traversable
                                                .insert(traversable_id, document_id);
                                        }
                                    }

                                    if let Some(document_id) = destroyed_document_id(&tracked_command) {
                                        self.state.documents.remove(&document_id);
                                        self.state.pending_navigation_finalizations.remove(&document_id);
                                        self.state.active_documents_by_traversable.retain(
                                            |_, active_document_id| *active_document_id != document_id,
                                        );
                                    }
                                    Ok(())
                                }
                                Ok(Err(error)) => Err(error),
                                Err(error) => Err(format!(
                                    "content command reply channel closed: {error}"
                                )),
                            },
                            Err(error) => Err(format!(
                                "failed to send command to event loop {handle}: {error}"
                            )),
                        }
                    }
                    None => Err(format!("unknown content handle: {handle}")),
                };
                let _ = reply.send(result);
            }
            UserAgentCommand::EvaluateScript {
                traversable_id,
                source,
                timeout,
                reply,
            } => {
                let error_reply = reply.clone();
                let send_result = match self.state.traversable_handles.get(&traversable_id).copied() {
                    Some(handle) => match self.state.event_loops.get(&handle) {
                        Some(entry) => {
                            let request_id = NEXT_SCRIPT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
                            entry.command_sender
                                .send(EventLoopCommand::EvaluateScript {
                                    traversable_id,
                                    request_id,
                                    source,
                                    reply,
                                })
                                .map_err(|error| {
                                    format!(
                                        "failed to send script evaluation to event loop {handle}: {error}"
                                    )
                                })
                        }
                        None => Err(format!(
                            "no content event loop found for traversable {traversable_id}"
                        )),
                    },
                    None => Err(format!(
                        "no content process owns traversable {traversable_id}"
                    )),
                };

                let _ = timeout;
                if let Err(error) = send_result {
                    let _ = error_reply.send(Err(error));
                }
            }
            UserAgentCommand::BroadcastViewport { snapshot } => {
                let command = viewport_command(snapshot);
                for entry in self.state.event_loops.values() {
                    let _ = entry
                        .command_sender
                        .send(EventLoopCommand::FireAndForget { command: command.clone() });
                }
            }
            UserAgentCommand::SetTraversableViewport {
                traversable_id,
                snapshot,
                offset_x,
                offset_y,
            } => {
                let Some(handle) = self.state.traversable_handles.get(&traversable_id).copied() else {
                    continue;
                };
                let Some(entry) = self.state.event_loops.get(&handle) else {
                    continue;
                };
                let command =
                    traversable_viewport_command(traversable_id, snapshot, offset_x, offset_y);
                let _ = entry
                    .command_sender
                    .send(EventLoopCommand::FireAndForget { command });
            }
            UserAgentCommand::DispatchEventFor {
                traversable_id,
                event,
            } => {
                let _ = match self.state.traversable_handles.get(&traversable_id).copied() {
                    Some(handle) => match self.state.active_documents_by_traversable.get(&traversable_id) {
                        Some(document_id) => match self.state.event_loops.get(&handle) {
                            Some(entry) => {
                                let command = ContentCommand::DispatchEvent {
                                    events: vec![DispatchEventEntry {
                                        document_id: *document_id,
                                        event,
                                    }],
                                };
                                match entry
                                    .command_sender
                                    .send(EventLoopCommand::FireAndForget { command })
                                {
                                    Ok(()) => Ok(true),
                                    Err(error) => Err(format!(
                                        "failed to send dispatch event to event loop {handle}: {error}"
                                    )),
                                }
                            }
                            None => Ok(false),
                        },
                        None => Ok(false),
                    },
                    None => Ok(false),
                };
            }
            UserAgentCommand::RenderingOpportunityFor {
                traversable_id,
            } => {
                let _ = match self.state.traversable_handles.get(&traversable_id).copied() {
                    Some(handle) => match self.state.active_documents_by_traversable.get(&traversable_id) {
                        Some(document_id) => match self.state.event_loops.get(&handle) {
                            Some(entry) => {
                                log_render_state_debug(format!(
                                    "queue rendering opportunity traversable={} document={} handle={}",
                                    traversable_id, document_id, handle,
                                ));
                                let command = ContentCommand::UpdateTheRendering {
                                    traversable_id,
                                    document_id: *document_id,
                                };
                                match entry
                                    .command_sender
                                    .send(EventLoopCommand::FireAndForget { command })
                                {
                                    Ok(()) => Ok(true),
                                    Err(error) => Err(format!(
                                        "failed to queue rendering update for event loop {handle}: {error}"
                                    )),
                                }
                            }
                            None => Ok(false),
                        },
                        None => Ok(false),
                    },
                    None => Ok(false),
                };
            }
            UserAgentCommand::DocumentFetchCompleted {
                event_loop_id,
                handler_id,
                response,
            } => {
                let _ = self.timer_command_sender.send(TimerCommand::Clear { timer_key: handler_id });
                let Some(handle) = self.state.handles_by_event_loop_id.get(&event_loop_id).copied() else {
                    continue;
                };
                let Some(entry) = self.state.event_loops.get(&handle) else {
                    continue;
                };
                let command = ContentCommand::CompleteDocumentFetch {
                    handler_id,
                    response,
                };
                let _ = entry
                    .command_sender
                    .send(EventLoopCommand::FireAndForget { command });
            }
            UserAgentCommand::DocumentFetchFailed {
                event_loop_id,
                handler_id,
            } => {
                let _ = self.timer_command_sender.send(TimerCommand::Clear { timer_key: handler_id });
                let Some(handle) = self.state.handles_by_event_loop_id.get(&event_loop_id).copied() else {
                    continue;
                };
                let Some(entry) = self.state.event_loops.get(&handle) else {
                    continue;
                };
                let command = ContentCommand::FailDocumentFetch { handler_id };
                let _ = entry
                    .command_sender
                    .send(EventLoopCommand::FireAndForget { command });
            }
            UserAgentCommand::NavigationFetchCompleted {
                navigation_id,
                response,
            } => {
                let Some(pending) = self.state.pending_navigation_fetches.remove(&navigation_id) else {
                    continue;
                };
                let command_sender = match command_sender_for_traversable(&self.state, pending.traversable_id) {
                    Ok(command_sender) => command_sender,
                    Err(error) => {
                        notify_navigation_failed(pending.traversable_id, error);
                        continue;
                    }
                };
                let document_id = allocate_document_id(&mut self.state);
                let final_url = response.final_url.clone();
                let loaded_response = LoadedDocumentResponse {
                    final_url: final_url.clone(),
                    status: response.status,
                    content_type: response.content_type.clone(),
                    body: String::from_utf8_lossy(&response.body).into_owned(),
                };
                match send_event_loop_command(
                    &command_sender,
                    ContentCommand::CreateLoadedDocument {
                        traversable_id: pending.traversable_id,
                        document_id,
                        response: loaded_response,
                    },
                ) {
                    Ok(_) => {
                        self.state.documents.insert(
                            document_id,
                            DocumentState {
                                traversable_id: pending.traversable_id,
                                url: final_url.clone(),
                                is_initial_about_blank: false,
                            },
                        );
                        self.state.pending_navigation_finalizations.insert(
                            document_id,
                            PendingNavigationFinalization {
                                traversable_id: pending.traversable_id,
                                previous_document_id: pending.previous_document_id,
                                url: final_url,
                            },
                        );
                    }
                    Err(error) => {
                        notify_navigation_failed(pending.traversable_id, error);
                    }
                }
            }
            UserAgentCommand::NavigationFetchFailed { navigation_id } => {
                let Some(pending) = self.state.pending_navigation_fetches.remove(&navigation_id) else {
                    continue;
                };
                notify_navigation_failed(
                    pending.traversable_id,
                    format!(
                        "navigation fetch failed for {}",
                        pending.destination_url
                    ),
                );
            }
            UserAgentCommand::DocumentFetchTimeout {
                event_loop_id,
                handler_id,
            } => {
                let Some(handle) = self.state.handles_by_event_loop_id.get(&event_loop_id).copied() else {
                    continue;
                };
                let Some(entry) = self.state.event_loops.get(&handle) else {
                    continue;
                };
                let command = ContentCommand::FailDocumentFetch { handler_id };
                let _ = entry
                    .command_sender
                    .send(EventLoopCommand::FireAndForget { command });
            }
            UserAgentCommand::WindowTimerTask {
                event_loop_id,
                document_id,
                timer_id,
                timer_key,
                nesting_level,
            } => {
                let Some(handle) = self.state.handles_by_event_loop_id.get(&event_loop_id).copied() else {
                    continue;
                };
                let Some(entry) = self.state.event_loops.get(&handle) else {
                    continue;
                };
                let command = ContentCommand::RunWindowTimer {
                    document_id,
                    timer_id,
                    timer_key,
                    nesting_level,
                };
                let _ = entry
                    .command_sender
                    .send(EventLoopCommand::FireAndForget { command });
            }
            UserAgentCommand::IframeTraversableRemoved {
                parent_traversable_id,
                content_navigable_id,
                reply,
            } => {
                self.state.known_child_navigables.remove(&content_navigable_id);
                let target_name = iframe_target_name(parent_traversable_id, content_navigable_id);
                let mut handles = self.state
                    .traversable_target_names
                    .iter()
                    .filter_map(|(traversable_id, traversable_target_name)| {
                        if traversable_target_name == &target_name {
                            self.state.traversable_handles.get(traversable_id).copied()
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                handles.sort_unstable();
                handles.dedup();

                let mut result = Ok(());
                for handle in handles {
                    if let Err(error) = stop_event_loop_handle(&mut self.state, handle) {
                        result = Err(error);
                        break;
                    }
                }

                self.state
                    .traversable_target_names
                    .retain(|_, traversable_target_name| traversable_target_name != &target_name);
                let _ = reply.send(result);
            }
            UserAgentCommand::ChildNavigableCreated {
                parent_traversable_id,
                content_navigable_id,
                reply,
            } => {
                self.state
                    .known_child_navigables
                    .insert(content_navigable_id, parent_traversable_id);
                let _ = reply.send(Ok(()));
            }
            UserAgentCommand::Shutdown { reply } => {
                let entries = self.state.event_loops.drain().map(|(_, entry)| entry).collect::<Vec<_>>();
                self.state.handles_by_event_loop_id.clear();
                self.state.traversable_handles.clear();
                self.state.traversable_target_names.clear();
                self.state.active_documents_by_traversable.clear();
                self.state.known_child_navigables.clear();
                self.state.documents.clear();
                self.state.pending_before_unload_navigations.clear();
                self.state.pending_navigation_fetches.clear();
                self.state.pending_navigation_finalizations.clear();

                let mut shutdown_result = Ok(());
                for entry in entries {
                    if let Err(error) = stop_event_loop_entry(entry) {
                        shutdown_result = Err(error);
                        break;
                    }
                }

                let (fetch_reply_sender, fetch_reply_receiver) = bounded(1);
                if let Err(error) = self.fetch_command_sender.send(FetchCommand::Shutdown {
                    reply: fetch_reply_sender,
                }) {
                    shutdown_result = Err(format!("failed to request fetch shutdown: {error}"));
                } else if let Err(error) = fetch_reply_receiver.recv() {
                    shutdown_result = Err(format!("fetch shutdown reply channel closed: {error}"));
                }

                if let Some(fetch_join_handle) = self.fetch_join_handle.take()
                    && fetch_join_handle.join().is_err()
                {
                    shutdown_result = Err(String::from("fetch thread panicked"));
                }

                let (timer_reply_sender, timer_reply_receiver) = bounded(1);
                if let Err(error) = self.timer_command_sender.send(TimerCommand::Shutdown {
                    reply: timer_reply_sender,
                }) {
                    shutdown_result = Err(format!("failed to request timer shutdown: {error}"));
                } else if let Err(error) = timer_reply_receiver.recv() {
                    shutdown_result = Err(format!("timer shutdown reply channel closed: {error}"));
                }

                if let Some(timer_join_handle) = self.timer_join_handle.take()
                    && timer_join_handle.join().is_err()
                {
                    shutdown_result = Err(String::from("timer thread panicked"));
                }

                let _ = reply.send(shutdown_result);
                break;
            }
        }
    }

    }

}