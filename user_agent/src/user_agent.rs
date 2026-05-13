mod event_loop;
mod fetch;
mod id;
mod timer;

use blitz_traits::shell::ColorScheme;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use embedder::{FinalizeNavigation, FormalWebUserEvent};
use ipc_messages::{
    content::{
        BeforeUnloadResult, Command as ContentCommand, DispatchEventEntry,
        FetchRequest as ContentFetchRequest, FetchResponse as ContentFetchResponse,
        FinalizeNavigation as ContentFinalizeNavigation, LoadedDocumentResponse, NavigateRequest,
        UserNavigationInvolvement, WebviewId,
    },
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::id::UserAgentIds;
use crate::event_loop::{
    EventLoopCommand, EventLoopEntry, destroyed_document_id, document_id_from_command,
    spawn_event_loop_entry, stop_event_loop_entry, traversable_viewport_command,
    viewport_command,
};
use crate::fetch::{FetchCommand, run_fetch_thread};
use crate::timer::{TimerCommand, run_timer_thread};

/// https://html.spec.whatwg.org/multipage/#cross-origin-isolation-mode
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CrossOriginIsolationMode {
    #[default]
    None,
    Logical,
    Concrete,
}

/// https://html.spec.whatwg.org/multipage/#agent-cluster-key
///
/// The Rust runtime stores origins as serialized strings here until the dedicated origin model is
/// shared across all browser components.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum AgentClusterKey {
    Site(String),
    Origin(String),
}

/// https://tc39.es/ecma262/#sec-agents
#[derive(Clone, Debug)]
pub struct Agent {
    /// Model-local identifier standing in for the signifier created by
    /// https://html.spec.whatwg.org/multipage/#create-an-agent.
    pub id: u64,
    /// https://tc39.es/ecma262/#sec-agents
    pub can_block: bool,
    /// https://html.spec.whatwg.org/multipage/#concept-agent-event-loop
    pub event_loop_id: usize,
}

/// https://html.spec.whatwg.org/multipage/#agent-cluster-cross-origin-isolation
#[derive(Clone, Debug)]
pub struct AgentCluster {
    /// Model-local identifier for https://html.spec.whatwg.org/multipage/#agent-cluster.
    pub id: u64,
    /// https://html.spec.whatwg.org/multipage/#agent-cluster-cross-origin-isolation
    pub cross_origin_isolation_mode: CrossOriginIsolationMode,
    /// https://html.spec.whatwg.org/multipage/#is-origin-keyed
    pub is_origin_keyed: bool,
    /// The single
    /// https://html.spec.whatwg.org/multipage/#similar-origin-window-agent associated with the
    /// current top-level traversable in the Rust runtime.
    pub similar_origin_window_agent: Agent,
}

/// https://html.spec.whatwg.org/multipage/#top-level-browsing-context
#[derive(Clone, Debug)]
pub struct BrowsingContext {
    /// Model-local identifier for https://html.spec.whatwg.org/multipage/#browsing-context.
    pub id: u64,
    /// https://html.spec.whatwg.org/multipage/#tlbc-group
    pub group_id: u64,
}

/// https://html.spec.whatwg.org/multipage/#browsing-context-group
#[derive(Clone, Debug, Default)]
pub struct BrowsingContextGroup {
    /// Model-local identifier for https://html.spec.whatwg.org/multipage/#browsing-context-group.
    pub id: u64,
    /// https://html.spec.whatwg.org/multipage/#browsing-context-set
    pub browsing_context_set: HashMap<u64, BrowsingContext>,
    /// https://html.spec.whatwg.org/multipage/#agent-cluster-map
    pub agent_cluster_map: HashMap<AgentClusterKey, AgentCluster>,
    /// https://html.spec.whatwg.org/multipage/#historical-agent-cluster-key-map
    pub historical_agent_cluster_key_map: HashMap<String, AgentClusterKey>,
    /// https://html.spec.whatwg.org/multipage/#bcg-cross-origin-isolation
    pub cross_origin_isolation_mode: CrossOriginIsolationMode,
}

/// https://html.spec.whatwg.org/multipage/#browsing-context-group-set
#[derive(Clone, Debug, Default)]
pub struct BrowsingContextGroupSet {
    /// https://html.spec.whatwg.org/multipage/#browsing-context-group-set
    pub members: HashMap<u64, BrowsingContextGroup>,
}

impl BrowsingContextGroupSet {
    fn next_group_id(&self) -> u64 {
        self.members.keys().copied().max().map_or(0, |group_id| group_id + 1)
    }

    fn remove_browsing_context(&mut self, browsing_context_id: u64) {
        let matching_group_id = self.members.iter().find_map(|(group_id, group)| {
            group.browsing_context_set
                .contains_key(&browsing_context_id)
                .then_some(*group_id)
        });

        let Some(group_id) = matching_group_id else {
            return;
        };

        let remove_group = if let Some(group) = self.members.get_mut(&group_id) {
            group.browsing_context_set.remove(&browsing_context_id);
            group.browsing_context_set.is_empty()
        } else {
            false
        };

        if remove_group {
            self.members.remove(&group_id);
        }
    }
}

/// https://html.spec.whatwg.org/multipage/#top-level-traversable
#[derive(Clone, Debug)]
pub struct TopLevelTraversable {
    /// Model-local identifier for https://html.spec.whatwg.org/multipage/#top-level-traversable.
    pub id: u64,
    /// Model-local browser-ui flag selecting the active top-level traversable.
    pub is_active: bool,
    /// Model-local mirror of
    /// https://html.spec.whatwg.org/multipage/#document-state-nav-target-name.
    pub target_name: String,
    /// Model-local reference to the active
    /// https://html.spec.whatwg.org/multipage/#browsing-context.
    pub active_browsing_context_id: Option<u64>,
    /// Model-local cache of the active document exposed by the current session history entry.
    pub active_document_id: Option<u64>,
    /// Model-local owner event loop for the traversable's content runtime.
    pub event_loop_id: usize,
    /// Model-local owner handle for the traversable's content runtime.
    pub handle: usize,
    /// https://html.spec.whatwg.org/multipage/#ongoing-navigation
    pub ongoing_navigation_id: Option<u64>,
    /// Model-local marker for deferred update-the-rendering work while navigation is still ongoing.
    pub has_deferred_update_the_rendering: bool,
    /// https://html.spec.whatwg.org/multipage/#tn-current-session-history-step
    pub current_session_history_step: usize,
    /// Model-local mirror of
    /// https://html.spec.whatwg.org/multipage/#tn-session-history-entries using active document
    /// identifiers.
    pub session_history_document_ids: Vec<u64>,
}

/// https://html.spec.whatwg.org/multipage/#top-level-traversable-set
#[derive(Clone, Debug, Default)]
pub struct TopLevelTraversableSet {
    /// https://html.spec.whatwg.org/multipage/#top-level-traversable-set
    pub members: HashMap<u64, TopLevelTraversable>,
}

impl TopLevelTraversableSet {
    fn find_by_target_name(&self, target_name: &str) -> Option<u64> {
        self.members.iter().find_map(|(traversable_id, traversable)| {
            (traversable.target_name == target_name).then_some(*traversable_id)
        })
    }
}

/// Top-level state for the Rust user-agent thread.
///
/// This mirrors the role of `FormalWeb.UserAgent`: allocator state, spec-facing global sets,
/// worker ownership, and the pending navigation/fetch continuations that connect the embedder,
/// content, fetch, and timer threads.
pub struct UserAgentState {
    /// Model-local allocator block mirroring the counters in `FormalWeb.UserAgent`.
    pub ids: UserAgentIds,
    /// https://html.spec.whatwg.org/multipage/#browsing-context-group-set
    pub browsing_context_group_set: BrowsingContextGroupSet,
    /// https://html.spec.whatwg.org/multipage/#top-level-traversable-set
    pub top_level_traversable_set: TopLevelTraversableSet,
    /// Model-local map from Rust worker handles to the owned event-loop workers.
    pub event_loops: HashMap<usize, EventLoopEntry>,
    /// Model-local reverse index from https://html.spec.whatwg.org/multipage/#event-loop ids to
    /// the owning Rust handle.
    pub handles_by_event_loop_id: HashMap<usize, usize>,
    /// Model-local reverse index from top-level traversable ids to the owning Rust handle.
    pub traversable_handles: HashMap<u64, usize>,
    /// Model-local cache of each traversable's active target name derived from
    /// `top_level_traversable_set`.
    pub traversable_target_names: HashMap<u64, String>,
    /// Model-local cache of each traversable's active document derived from
    /// `top_level_traversable_set`.
    pub active_documents_by_traversable: HashMap<u64, u64>,
    /// Model-local map from iframe child content-navigable ids to the parent traversable id that
    /// owns the iframe host slot.
    pub known_child_navigables: HashMap<u64, u64>,
    /// Model-local cache of active and pending documents keyed by
    /// https://dom.spec.whatwg.org/#concept-document identifiers.
    pub documents: HashMap<u64, DocumentState>,
    /// Model-local queue of navigations paused while content runs `beforeunload`.
    pub pending_before_unload_navigations: HashMap<u64, PendingBeforeUnloadNavigation>,
    /// Model-local queue of fetch-backed navigations suspended at the response wait point.
    pub pending_navigation_fetches: HashMap<u64, PendingNavigationFetch>,
    /// Model-local reverse index from https://fetch.spec.whatwg.org/#fetch-controller ids to
    /// pending navigation ids.
    pub pending_navigation_fetch_ids_by_fetch_id: HashMap<u64, u64>,
    /// Model-local queue of documents waiting for
    /// https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation.
    pub pending_navigation_finalizations: HashMap<u64, PendingNavigationFinalization>,
    /// Model-local reverse index from https://html.spec.whatwg.org/multipage/#navigation-params-id
    /// to pending finalization document ids.
    pub pending_navigation_finalization_ids_by_navigation_id: HashMap<u64, u64>,
}

/// Model-local cache of the active document state held by the user agent.
#[derive(Clone)]
pub struct DocumentState {
    /// Model-local reference back to the top-level traversable that currently presents the
    /// document.
    pub traversable_id: u64,
    /// Model-local reference to the active browsing context for the document.
    pub browsing_context_id: Option<u64>,
    /// Model-local owner event loop for the document's content runtime.
    pub event_loop_id: usize,
    /// Active document URL.
    pub url: String,
    /// Model-local flag for the initial about:blank special case.
    pub is_initial_about_blank: bool,
}

/// Model-local continuation for
/// https://html.spec.whatwg.org/multipage/document-lifecycle.html#checking-if-unloading-is-canceled.
#[derive(Clone)]
pub struct PendingBeforeUnloadNavigation {
    /// Model-local identifier for the queued beforeunload check.
    pub check_id: u64,
    /// Model-local identifier corresponding to
    /// https://html.spec.whatwg.org/multipage/#navigation-params-id.
    pub navigation_id: u64,
    pub traversable_id: u64,
    pub document_id: u64,
    pub destination_url: String,
    pub user_involvement: ipc_messages::content::UserNavigationInvolvement,
}

/// Pending fetch-backed navigation paused at the response wait point.
#[derive(Clone)]
pub struct PendingNavigationFetch {
    /// Model-local identifier corresponding to https://fetch.spec.whatwg.org/#fetch-controller.
    pub fetch_id: u64,
    /// Model-local identifier corresponding to
    /// https://html.spec.whatwg.org/multipage/#navigation-params-id.
    pub navigation_id: u64,
    pub traversable_id: u64,
    pub previous_document_id: Option<u64>,
    pub destination_url: String,
    pub user_involvement: ipc_messages::content::UserNavigationInvolvement,
}

/// Model-local continuation for
/// https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation.
#[derive(Clone)]
pub struct PendingNavigationFinalization {
    /// Model-local identifier for the loaded document that will emit the finalization signal.
    pub document_id: u64,
    /// Model-local identifier corresponding to
    /// https://html.spec.whatwg.org/multipage/#navigation-params-id.
    pub navigation_id: u64,
    pub traversable_id: u64,
    pub previous_document_id: Option<u64>,
    pub url: String,
}

impl Default for UserAgentState {
    fn default() -> Self {
        Self {
            ids: UserAgentIds::default(),
            browsing_context_group_set: BrowsingContextGroupSet::default(),
            top_level_traversable_set: TopLevelTraversableSet::default(),
            event_loops: HashMap::new(),
            handles_by_event_loop_id: HashMap::new(),
            traversable_handles: HashMap::new(),
            traversable_target_names: HashMap::new(),
            active_documents_by_traversable: HashMap::new(),
            known_child_navigables: HashMap::new(),
            documents: HashMap::new(),
            pending_before_unload_navigations: HashMap::new(),
            pending_navigation_fetches: HashMap::new(),
            pending_navigation_fetch_ids_by_fetch_id: HashMap::new(),
            pending_navigation_finalizations: HashMap::new(),
            pending_navigation_finalization_ids_by_navigation_id: HashMap::new(),
        }
    }
}

impl UserAgentState {
    fn set_active_top_level_traversable(&mut self, traversable_id: u64) {
        for (candidate_id, traversable) in &mut self.top_level_traversable_set.members {
            traversable.is_active = *candidate_id == traversable_id;
        }
    }

    fn set_traversable_active_document(&mut self, traversable_id: u64, document_id: u64) {
        self.active_documents_by_traversable
            .insert(traversable_id, document_id);
        if let Some(traversable) = self.top_level_traversable_set.members.get_mut(&traversable_id) {
            traversable.active_document_id = Some(document_id);
        }
    }

    fn set_traversable_ongoing_navigation(
        &mut self,
        traversable_id: u64,
        navigation_id: Option<u64>,
    ) {
        if let Some(traversable) = self.top_level_traversable_set.members.get_mut(&traversable_id) {
            traversable.ongoing_navigation_id = navigation_id;
        }
    }

    fn append_session_history_document(&mut self, traversable_id: u64, document_id: u64) {
        if let Some(traversable) = self.top_level_traversable_set.members.get_mut(&traversable_id) {
            traversable.session_history_document_ids.push(document_id);
            traversable.current_session_history_step = traversable
                .session_history_document_ids
                .len()
                .saturating_sub(1);
        }
    }

    fn insert_pending_navigation_fetch(&mut self, pending: PendingNavigationFetch) {
        self.pending_navigation_fetch_ids_by_fetch_id
            .insert(pending.fetch_id, pending.navigation_id);
        self.pending_navigation_fetches
            .insert(pending.navigation_id, pending);
    }

    fn take_pending_navigation_fetch_by_navigation_id(
        &mut self,
        navigation_id: u64,
    ) -> Option<PendingNavigationFetch> {
        let pending = self.pending_navigation_fetches.remove(&navigation_id)?;
        self.pending_navigation_fetch_ids_by_fetch_id
            .remove(&pending.fetch_id);
        Some(pending)
    }

    fn take_pending_navigation_fetch_by_fetch_id(
        &mut self,
        fetch_id: u64,
    ) -> Option<PendingNavigationFetch> {
        let navigation_id = self.pending_navigation_fetch_ids_by_fetch_id.remove(&fetch_id)?;
        self.pending_navigation_fetches.remove(&navigation_id)
    }

    fn remove_pending_navigation_fetches_for_traversable(&mut self, traversable_id: u64) {
        let navigation_ids = self
            .pending_navigation_fetches
            .iter()
            .filter_map(|(navigation_id, pending)| {
                (pending.traversable_id == traversable_id).then_some(*navigation_id)
            })
            .collect::<Vec<_>>();

        for navigation_id in navigation_ids {
            let _ = self.take_pending_navigation_fetch_by_navigation_id(navigation_id);
        }
    }

    fn insert_pending_navigation_finalization(&mut self, pending: PendingNavigationFinalization) {
        self.pending_navigation_finalization_ids_by_navigation_id
            .insert(pending.navigation_id, pending.document_id);
        self.pending_navigation_finalizations
            .insert(pending.document_id, pending);
    }

    fn take_pending_navigation_finalization_by_document_id(
        &mut self,
        document_id: u64,
    ) -> Option<PendingNavigationFinalization> {
        let pending = self.pending_navigation_finalizations.remove(&document_id)?;
        self.pending_navigation_finalization_ids_by_navigation_id
            .remove(&pending.navigation_id);
        Some(pending)
    }

    fn remove_pending_navigation_finalizations_for_traversable(
        &mut self,
        traversable_id: u64,
    ) -> Vec<u64> {
        let document_ids = self
            .pending_navigation_finalizations
            .iter()
            .filter_map(|(document_id, pending)| {
                (pending.traversable_id == traversable_id).then_some(*document_id)
            })
            .collect::<Vec<_>>();

        for document_id in &document_ids {
            let _ = self.take_pending_navigation_finalization_by_document_id(*document_id);
        }

        document_ids
    }

    fn remove_top_level_traversable(&mut self, traversable_id: u64) {
        let browsing_context_id = self
            .top_level_traversable_set
            .members
            .get(&traversable_id)
            .and_then(|traversable| traversable.active_browsing_context_id);

        self.top_level_traversable_set.members.remove(&traversable_id);
        self.traversable_handles.remove(&traversable_id);
        self.traversable_target_names.remove(&traversable_id);
        self.active_documents_by_traversable.remove(&traversable_id);

        if let Some(browsing_context_id) = browsing_context_id {
            self.browsing_context_group_set
                .remove_browsing_context(browsing_context_id);
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
        fetch_id: u64,
        response: ContentFetchResponse,
    },
    NavigationFetchFailed {
        fetch_id: u64,
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
    state.ids.allocate_event_loop_id()
}

fn allocate_traversable_id(state: &mut UserAgentState) -> u64 {
    state.ids.allocate_traversable_id()
}

fn allocate_document_id(state: &mut UserAgentState) -> u64 {
    state.ids.allocate_document_id()
}

fn allocate_navigation_id(state: &mut UserAgentState) -> u64 {
    state.ids.allocate_navigation_id()
}

fn allocate_before_unload_check_id(state: &mut UserAgentState) -> u64 {
    state.ids.allocate_before_unload_check_id()
}

fn normalize_navigation_target_name(target_name: &str) -> String {
    if target_name.eq_ignore_ascii_case("_self") {
        String::new()
    } else {
        target_name.to_owned()
    }
}

fn target_name_keeps_browser_ui_focus(target_name: &str) -> bool {
    !target_name.starts_with("_iframe|")
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
        state.ids.observe_event_loop_id(event_loop_id);
        return Ok(handle);
    }

    state.ids.observe_event_loop_id(event_loop_id);
    let handle = state.ids.allocate_handle();
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
    state.top_level_traversable_set.find_by_target_name(target_name)
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
    let browsing_context_group_id = state.browsing_context_group_set.next_group_id();
    let browsing_context_id = state.ids.allocate_browsing_context_id();
    let document_id = allocate_document_id(state);
    let agent_cluster_id = state.ids.allocate_agent_cluster_id();
    let agent_id = state.ids.allocate_agent_id();
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
    state.set_traversable_active_document(traversable_id, document_id);
    state.browsing_context_group_set.members.insert(
        browsing_context_group_id,
        BrowsingContextGroup {
            id: browsing_context_group_id,
            browsing_context_set: HashMap::from([(
                browsing_context_id,
                BrowsingContext {
                    id: browsing_context_id,
                    group_id: browsing_context_group_id,
                },
            )]),
            agent_cluster_map: HashMap::from([(
                AgentClusterKey::Site(String::from("about:blank")),
                AgentCluster {
                    id: agent_cluster_id,
                    cross_origin_isolation_mode: CrossOriginIsolationMode::None,
                    is_origin_keyed: false,
                    similar_origin_window_agent: Agent {
                        id: agent_id,
                        can_block: false,
                        event_loop_id,
                    },
                },
            )]),
            historical_agent_cluster_key_map: HashMap::new(),
            cross_origin_isolation_mode: CrossOriginIsolationMode::None,
        },
    );
    state.top_level_traversable_set.members.insert(
        traversable_id,
        TopLevelTraversable {
            id: traversable_id,
            is_active: false,
            target_name: target_name.clone(),
            active_browsing_context_id: Some(browsing_context_id),
            active_document_id: Some(document_id),
            event_loop_id,
            handle,
            ongoing_navigation_id: None,
            has_deferred_update_the_rendering: false,
            current_session_history_step: 0,
            session_history_document_ids: vec![document_id],
        },
    );
    if target_name_keeps_browser_ui_focus(&target_name) {
        state.set_active_top_level_traversable(traversable_id);
    }
    state.documents.insert(
        document_id,
        DocumentState {
            traversable_id,
            browsing_context_id: Some(browsing_context_id),
            event_loop_id,
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
    state.remove_pending_navigation_fetches_for_traversable(traversable_id);

    let stale_document_ids =
        state.remove_pending_navigation_finalizations_for_traversable(traversable_id);
    let command_sender = command_sender_for_traversable(state, traversable_id).ok();

    for document_id in stale_document_ids {
        if let Some(command_sender) = command_sender.as_ref() {
            let _ = send_event_loop_command(
                command_sender,
                ContentCommand::DestroyDocument { document_id },
            );
        }
        state.documents.remove(&document_id);
    }
    state.set_traversable_ongoing_navigation(traversable_id, None);
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
    navigation_id: u64,
    traversable_id: u64,
    destination_url: String,
    user_involvement: UserNavigationInvolvement,
) -> Result<(), String> {
    let fetch_id = state.ids.allocate_fetch_id();
    let previous_document_id = state.active_documents_by_traversable.get(&traversable_id).copied();
    state.insert_pending_navigation_fetch(PendingNavigationFetch {
        fetch_id,
        navigation_id,
        traversable_id,
        previous_document_id,
        destination_url: destination_url.clone(),
        user_involvement: user_involvement.clone(),
    });
    state.set_traversable_ongoing_navigation(traversable_id, Some(navigation_id));

    if let Err(error) = fetch_command_sender.send(FetchCommand::StartNavigationFetch {
        fetch_id,
        request: ContentFetchRequest {
            handler_id: fetch_id,
            url: destination_url,
            method: String::from("GET"),
            body: String::new(),
        },
    }) {
        let _ = state.take_pending_navigation_fetch_by_navigation_id(navigation_id);
        state.set_traversable_ongoing_navigation(traversable_id, None);
        return Err(format!("failed to start navigation fetch: {error}"));
    }

    Ok(())
}

fn begin_navigation_for_traversable(
    state: &mut UserAgentState,
    fetch_command_sender: &Sender<FetchCommand>,
    traversable_id: u64,
    destination_url: String,
    user_involvement: UserNavigationInvolvement,
) -> Result<(), String> {
    clear_pending_navigation_for_traversable(state, traversable_id);

    let navigation_id = allocate_navigation_id(state);
    let active_document_id = state.active_documents_by_traversable.get(&traversable_id).copied();
    let should_run_before_unload = active_document_id
        .and_then(|document_id| state.documents.get(&document_id))
        .is_some_and(|document| !document.is_initial_about_blank);

    state.set_traversable_ongoing_navigation(traversable_id, Some(navigation_id));

    if should_run_before_unload {
        let document_id = active_document_id.expect("beforeunload document id disappeared");
        let check_id = allocate_before_unload_check_id(state);
        state.pending_before_unload_navigations.insert(
            check_id,
            PendingBeforeUnloadNavigation {
                check_id,
                navigation_id,
                traversable_id,
                document_id,
                destination_url,
                user_involvement,
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
            state.set_traversable_ongoing_navigation(traversable_id, None);
            return Err(error);
        }
        Ok(())
    } else {
        start_navigation_fetch(
            state,
            fetch_command_sender,
            navigation_id,
            traversable_id,
            destination_url,
            user_involvement,
        )
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
        pending.navigation_id,
        pending.traversable_id,
        pending.destination_url,
        pending.user_involvement,
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
    for traversable_id in &removed_traversable_ids {
        state.remove_top_level_traversable(*traversable_id);
        state.remove_pending_navigation_fetches_for_traversable(*traversable_id);
        let _ = state.remove_pending_navigation_finalizations_for_traversable(*traversable_id);
    }
    state.documents.retain(|_, document| {
        !removed_traversable_ids.contains(&document.traversable_id)
    });
    state.pending_before_unload_navigations.retain(|_, pending| {
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
    /// Spec-facing browser state plus the model-local indices that make the Rust runtime route
    /// commands quickly.
    state: UserAgentState,
    /// Sender cloned into worker threads and embedder handles so all browser coordination funnels
    /// back through the single user-agent command loop.
    command_sender: Sender<UserAgentCommand>,
    /// Receiver for browser, embedder, automation, fetch, timer, and content-originated commands.
    command_receiver: Receiver<UserAgentCommand>,
    /// Sender for the dedicated fetch worker that owns the network sidecar bridge.
    fetch_command_sender: Sender<FetchCommand>,
    /// Join handle for the fetch worker thread during shutdown.
    fetch_join_handle: Option<JoinHandle<()>>,
    /// Sender for the dedicated timer worker that owns the timer heap/map state.
    timer_command_sender: Sender<TimerCommand>,
    /// Join handle for the timer worker thread during shutdown.
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
                        UserNavigationInvolvement::BrowserUi,
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
                        UserNavigationInvolvement::BrowserUi,
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
                        request.user_involvement.clone(),
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
                        request.user_involvement.clone(),
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
                            self.state
                                .set_traversable_ongoing_navigation(pending.traversable_id, None);
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
                            self.state
                                .set_traversable_ongoing_navigation(pending.traversable_id, None);
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
                let result = if let Some(pending) = self
                    .state
                    .take_pending_navigation_finalization_by_document_id(finalized.document_id)
                {
                    self.state.set_traversable_active_document(
                        pending.traversable_id,
                        finalized.document_id,
                    );
                    self.state.append_session_history_document(
                        pending.traversable_id,
                        finalized.document_id,
                    );
                    self.state
                        .set_traversable_ongoing_navigation(pending.traversable_id, None);
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
                let result = if let Some(pending) = self
                    .state
                    .take_pending_navigation_finalization_by_document_id(finalized.document_id)
                {
                    self.state.set_traversable_active_document(
                        pending.traversable_id,
                        finalized.document_id,
                    );
                    self.state.append_session_history_document(
                        pending.traversable_id,
                        finalized.document_id,
                    );
                    self.state
                        .set_traversable_ongoing_navigation(pending.traversable_id, None);
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
                                        self.state.ids.observe_traversable_id(traversable_id);
                                        self.state.top_level_traversable_set.members.entry(traversable_id).or_insert_with(|| TopLevelTraversable {
                                            id: traversable_id,
                                            is_active: false,
                                            target_name: String::new(),
                                            active_browsing_context_id: None,
                                            active_document_id: None,
                                            event_loop_id: entry.event_loop_id,
                                            handle,
                                            ongoing_navigation_id: None,
                                            has_deferred_update_the_rendering: false,
                                            current_session_history_step: 0,
                                            session_history_document_ids: Vec::new(),
                                        });
                                        if let Some(document_id) = document_id_from_command(&tracked_command) {
                                            self.state.ids.observe_document_id(document_id);
                                            self.state.set_traversable_active_document(
                                                traversable_id,
                                                document_id,
                                            );
                                        }
                                    }

                                    if let Some(document_id) = destroyed_document_id(&tracked_command) {
                                        self.state.documents.remove(&document_id);
                                        let _ = self
                                            .state
                                            .take_pending_navigation_finalization_by_document_id(document_id);
                                        let affected_traversable_ids = self
                                            .state
                                            .active_documents_by_traversable
                                            .iter()
                                            .filter_map(|(traversable_id, active_document_id)| {
                                                (*active_document_id == document_id)
                                                    .then_some(*traversable_id)
                                            })
                                            .collect::<Vec<_>>();
                                        for traversable_id in affected_traversable_ids {
                                            self.state.active_documents_by_traversable.remove(&traversable_id);
                                            if let Some(traversable) = self
                                                .state
                                                .top_level_traversable_set
                                                .members
                                                .get_mut(&traversable_id)
                                                && traversable.active_document_id == Some(document_id)
                                            {
                                                traversable.active_document_id = None;
                                            }
                                        }
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
            UserAgentCommand::NavigationFetchCompleted { fetch_id, response } => {
                let Some(pending) = self.state.take_pending_navigation_fetch_by_fetch_id(fetch_id) else {
                    continue;
                };
                let command_sender = match command_sender_for_traversable(&self.state, pending.traversable_id) {
                    Ok(command_sender) => command_sender,
                    Err(error) => {
                        self.state
                            .set_traversable_ongoing_navigation(pending.traversable_id, None);
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
                                browsing_context_id: self
                                    .state
                                    .top_level_traversable_set
                                    .members
                                    .get(&pending.traversable_id)
                                    .and_then(|traversable| traversable.active_browsing_context_id),
                                event_loop_id: self
                                    .state
                                    .top_level_traversable_set
                                    .members
                                    .get(&pending.traversable_id)
                                    .map_or(0, |traversable| traversable.event_loop_id),
                                url: final_url.clone(),
                                is_initial_about_blank: false,
                            },
                        );
                        self.state.insert_pending_navigation_finalization(PendingNavigationFinalization {
                            document_id,
                            navigation_id: pending.navigation_id,
                            traversable_id: pending.traversable_id,
                            previous_document_id: pending.previous_document_id,
                            url: final_url,
                        });
                    }
                    Err(error) => {
                        self.state
                            .set_traversable_ongoing_navigation(pending.traversable_id, None);
                        notify_navigation_failed(pending.traversable_id, error);
                    }
                }
            }
            UserAgentCommand::NavigationFetchFailed { fetch_id } => {
                let Some(pending) = self.state.take_pending_navigation_fetch_by_fetch_id(fetch_id) else {
                    continue;
                };
                self.state
                    .set_traversable_ongoing_navigation(pending.traversable_id, None);
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
                self.state.browsing_context_group_set.members.clear();
                self.state.top_level_traversable_set.members.clear();
                self.state.traversable_handles.clear();
                self.state.traversable_target_names.clear();
                self.state.active_documents_by_traversable.clear();
                self.state.known_child_navigables.clear();
                self.state.documents.clear();
                self.state.pending_before_unload_navigations.clear();
                self.state.pending_navigation_fetches.clear();
                self.state.pending_navigation_fetch_ids_by_fetch_id.clear();
                self.state.pending_navigation_finalizations.clear();
                self.state.pending_navigation_finalization_ids_by_navigation_id.clear();

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