mod event_loop;
mod fetch;
pub(crate) mod ipc_manifest;
mod timer;
pub(crate) mod ui_event;

use blitz_traits::shell::ColorScheme;
use crossbeam_channel::{Receiver, Sender, bounded, select, unbounded};
use ipc_messages::content::{
    AgentClusterId, AgentId, BeforeUnloadCheckId, BeforeUnloadResult, BrowsingContextGroupId,
    BrowsingContextId, Command as ContentCommand, DispatchEventEntry, DocumentId, EventLoopId,
    FetchResponse as ContentFetchResponse, FinalizeNavigation as ContentFinalizeNavigation,
    FrameId, LoadedDocumentResponse, NavigableId, NavigateRequest, NavigationFetchId, NavigationId,
    NewTraversableInfo, UserNavigationInvolvement, WebviewId, WebviewProviderMessage,
    WindowTimerKey, iframe_target_name,
};
use log::{debug, error, trace};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use url::Url;
use verification::{TLATracer, TraceSender};

fn startup_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_STARTUP").is_some()
}

use crate::event_loop::{
    EventLoopCommand, EventLoopEntry, spawn_event_loop_entry, stop_event_loop_entry,
    traversable_viewport_command,
};
use crate::timer::{TimerCommand, run_timer_thread};
use ipc_messages::media::{MediaPipelineId, VideoPaintId};

pub(crate) fn sidecar_executable_path(binary_name: &str) -> Result<PathBuf, String> {
    let current_executable = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let executable_directory = current_executable
        .parent()
        .ok_or_else(|| String::from("failed to resolve executable directory"))?;
    let executable_name = format!("{binary_name}{}", std::env::consts::EXE_SUFFIX);

    for candidate in sidecar_search_paths(executable_directory, &executable_name) {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    let attempted_paths = sidecar_search_paths(executable_directory, &executable_name)
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    Err(format!(
        "failed to locate sidecar executable {binary_name}; looked in: {attempted_paths}"
    ))
}

fn sidecar_search_paths(executable_directory: &Path, executable_name: &str) -> Vec<PathBuf> {
    let mut search_paths = vec![executable_directory.join(executable_name)];

    let Some(profile_dir_name) = executable_directory
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return search_paths;
    };
    if !matches!(profile_dir_name, "debug" | "release") {
        return search_paths;
    }

    if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR") {
        let target_dir = PathBuf::from(target_dir);
        search_paths.push(
            target_dir
                .join("sidecar-prebuild")
                .join(profile_dir_name)
                .join(executable_name),
        );
        search_paths.push(target_dir.join(profile_dir_name).join(executable_name));
    }

    for ancestor in executable_directory.ancestors().skip(1) {
        search_paths.push(
            ancestor
                .join("target")
                .join("sidecar-prebuild")
                .join(profile_dir_name)
                .join(executable_name),
        );
        search_paths.push(
            ancestor
                .join("target")
                .join(profile_dir_name)
                .join(executable_name),
        );
    }

    search_paths.dedup();
    search_paths
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavigationCompletion {
    Committed { url: String },
    Aborted { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationCompleted {
    pub webview_id: WebviewId,
    pub status: NavigationCompletion,
}

pub trait Embedder: Send + Sync {
    fn navigation_requested(
        &self,
        webview_id: WebviewId,
        destination_url: String,
    ) -> Result<(), String>;
    fn navigation_completed(&self, completed: NavigationCompleted) -> Result<(), String>;
    fn new_webview(&self, webview_id: WebviewId, target_name: String) -> Result<(), String>;
    fn webview_provider_sync(&self) -> Result<(), String>;
    fn new_frame_rendered(&self) -> Result<(), String>;
    fn request_redraw(&self, webview_id: WebviewId);
    fn viewport_scale_factor(&self) -> f32;
    fn window_viewport_snapshot(&self) -> Option<(u32, u32, f32, ColorScheme)>;
    fn clipboard_get_text(&self, timeout: Duration) -> Result<String, String>;
    fn clipboard_set_text(&self, text: String, timeout: Duration) -> Result<(), String>;
    /// Forward a composed web content scene from the graphics process to the
    /// embedder for rendering.
    fn new_web_content_scene(
        &self,
        webview_id: WebviewId,
        scene_bytes: Vec<u8>,
        font_registrations: Vec<ipc_messages::content::RegisteredFont>,
        font_data: std::collections::HashMap<usize, Vec<u8>>,
        frame_hit_info: Vec<ipc_messages::graphics::FrameHitInfo>,
    ) -> Result<(), String>;
}

/// <https://html.spec.whatwg.org/multipage/#cross-origin-isolation-mode>
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CrossOriginIsolationMode {
    #[default]
    None,
    Logical,
    Concrete,
}

/// <https://html.spec.whatwg.org/multipage/#agent-cluster-key>
///
/// The implementation stores origins as serialized strings here until the dedicated origin model is
/// shared across all browser components.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum AgentClusterKey {
    Site(String),
    Origin(String),
}

/// <https://tc39.es/ecma262/#sec-agents>
#[derive(Clone, Debug)]
pub struct Agent {
    /// identifier standing in for the signifier created by
    /// <https://html.spec.whatwg.org/multipage/#create-an-agent>
    pub id: AgentId,
    /// <https://tc39.es/ecma262/#sec-agents>
    pub can_block: bool,
    /// <https://html.spec.whatwg.org/multipage/#concept-agent-event-loop>
    pub event_loop_id: EventLoopId,
}

/// <https://html.spec.whatwg.org/multipage/#agent-cluster-cross-origin-isolation>
#[derive(Clone, Debug)]
pub struct AgentCluster {
    /// identifier for <https://html.spec.whatwg.org/multipage/#agent-cluster>
    pub id: AgentClusterId,
    /// <https://html.spec.whatwg.org/multipage/#agent-cluster-cross-origin-isolation>
    pub cross_origin_isolation_mode: CrossOriginIsolationMode,
    /// <https://html.spec.whatwg.org/multipage/#is-origin-keyed>
    pub is_origin_keyed: bool,
    /// The single
    /// <https://html.spec.whatwg.org/multipage/#similar-origin-window-agent> associated with the
    /// current top-level traversable in the implementation.
    pub similar_origin_window_agent: Agent,
}

/// <https://html.spec.whatwg.org/multipage/#top-level-browsing-context>
#[derive(Clone, Debug)]
pub struct BrowsingContext {
    /// Identifier for <https://html.spec.whatwg.org/multipage/#browsing-context>
    pub id: BrowsingContextId,

    /// <https://html.spec.whatwg.org/multipage/#auxiliary-browsing-context>
    pub is_auxiliary: bool,

    /// <https://html.spec.whatwg.org/multipage/#opener-browsing-context>
    pub opener_browsing_context: Option<BrowsingContextId>,

    /// <https://html.spec.whatwg.org/multipage/#is-popup>
    pub is_popup: bool,
}

/// <https://html.spec.whatwg.org/multipage/#browsing-context-group>
#[derive(Clone, Debug)]
pub struct BrowsingContextGroup {
    /// identifier for <https://html.spec.whatwg.org/multipage/#browsing-context-group>
    pub id: BrowsingContextGroupId,
    /// <https://html.spec.whatwg.org/multipage/#browsing-context-set>
    pub browsing_context_set: HashMap<BrowsingContextId, BrowsingContext>,
    /// <https://html.spec.whatwg.org/multipage/#agent-cluster-map>
    pub agent_cluster_map: HashMap<AgentClusterKey, AgentCluster>,
    /// <https://html.spec.whatwg.org/multipage/#historical-agent-cluster-key-map>
    pub historical_agent_cluster_key_map: HashMap<String, AgentClusterKey>,
    /// <https://html.spec.whatwg.org/multipage/#bcg-cross-origin-isolation>
    pub cross_origin_isolation_mode: CrossOriginIsolationMode,
}

/// <https://html.spec.whatwg.org/multipage/#browsing-context-group-set>
#[derive(Clone, Debug, Default)]
pub struct BrowsingContextGroupSet {
    /// <https://html.spec.whatwg.org/multipage/#browsing-context-group-set>
    pub members: HashMap<BrowsingContextGroupId, BrowsingContextGroup>,
}

impl BrowsingContextGroupSet {
    /// allocating the next browser-global browsing-context-group id.
    fn next_group_id(&self) -> BrowsingContextGroupId {
        BrowsingContextGroupId::new()
    }

    /// removing one <https://html.spec.whatwg.org/multipage/#browsing-context>
    /// from the user agent's browsing-context-group set.
    fn remove_browsing_context(&mut self, browsing_context_id: BrowsingContextId) {
        let matching_group_id = self.members.iter().find_map(|(group_id, group)| {
            group
                .browsing_context_set
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

/// <https://html.spec.whatwg.org/multipage/#navigable>
///
/// A navigable is a traversable navigable when `event_loop_id` is `Some`; those entries also
/// carry the session-history and browsing-context fields below.
#[derive(Clone, Debug)]
pub struct Navigable {
    /// Identifier for <https://html.spec.whatwg.org/multipage/#navigable-id>.
    pub id: NavigableId,
    /// <https://html.spec.whatwg.org/multipage/#nav-parent>
    pub parent_navigable_id: Option<NavigableId>,
    /// Active document exposed by this navigable.
    pub active_document_id: Option<DocumentId>,
    // --- Traversable-specific fields (only populated when `event_loop_id` is `Some`) ---
    /// Browser-UI flag selecting the active traversable.
    pub is_active: bool,
    /// <https://html.spec.whatwg.org/multipage/#document-state-nav-target-name>
    pub target_name: String,
    /// <https://html.spec.whatwg.org/multipage/#nav-bc>
    pub active_browsing_context_id: Option<BrowsingContextId>,
    /// Owning event loop; `Some` when this is a traversable navigable.
    pub event_loop_id: Option<EventLoopId>,
    /// Owning handle; `Some` when this is a traversable navigable.
    pub handle: Option<EventLoopId>,
    /// <https://html.spec.whatwg.org/multipage/#ongoing-navigation>
    pub ongoing_navigation_id: Option<NavigationId>,
    /// Marker for deferred update-the-rendering work while navigation is still ongoing.
    pub has_deferred_update_the_rendering: bool,
    /// Compositor frame slot for child traversables; `None` for top-level traversables.
    pub frame_id: Option<FrameId>,
    /// <https://html.spec.whatwg.org/multipage/#tn-current-session-history-step>
    pub current_session_history_step: usize,
    /// <https://html.spec.whatwg.org/multipage/#tn-session-history-entries>
    pub session_history_entries: Vec<SessionHistoryEntry>,
}

/// <https://html.spec.whatwg.org/multipage/#session-history-entry>
#[derive(Clone, Debug)]
pub struct SessionHistoryEntry {
    /// <https://html.spec.whatwg.org/multipage/#she-step>
    pub step: usize,
    /// reference to <https://dom.spec.whatwg.org/#concept-document>
    pub document_id: DocumentId,
    /// <https://html.spec.whatwg.org/multipage/#session-history-entry-url>
    pub url: String,
}

/// <https://html.spec.whatwg.org/multipage/#history-handling-behavior>
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryHandlingBehavior {
    Push,
    Replace,
}

/// <https://w3c.github.io/navigation-timing/#dom-navigationtimingtype>
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NavigationTimingType {
    #[default]
    Navigate,
}

/// <https://html.spec.whatwg.org/multipage/#source-snapshot-params>
#[derive(Clone, Debug, Default)]
pub struct SourceSnapshotParams {
    /// <https://html.spec.whatwg.org/multipage/#source-snapshot-params-activation>
    pub has_transient_activation: bool,
    /// placeholder for
    /// <https://html.spec.whatwg.org/multipage/#source-snapshot-params-client>
    pub fetch_client_id: Option<u64>,
    /// serialized placeholder for
    /// <https://html.spec.whatwg.org/multipage/#source-snapshot-params-policy-container>
    pub source_policy_container: Option<String>,
}

impl SourceSnapshotParams {
    /// <https://html.spec.whatwg.org/multipage/#source-snapshot-params>
    fn for_user_involvement(user_involvement: &UserNavigationInvolvement) -> Self {
        Self {
            has_transient_activation: matches!(
                user_involvement,
                UserNavigationInvolvement::Activation
            ),
            fetch_client_id: None,
            source_policy_container: None,
        }
    }
}

/// <https://html.spec.whatwg.org/multipage/#target-snapshot-params>
#[derive(Clone, Debug, Default)]
pub struct TargetSnapshotParams {
    /// serialized placeholder for
    /// <https://html.spec.whatwg.org/multipage/#target-snapshot-params-sandbox>
    pub sandboxing_flags: Vec<String>,
    /// <https://html.spec.whatwg.org/multipage/#target-snapshot-params-iframe-referrer-policy>
    pub iframe_element_referrer_policy: Option<String>,
}

#[derive(Clone, Copy, Debug)]
struct BrowsingContextNavigationSelection {
    browsing_context_id: BrowsingContextId,
    swapped_group: bool,
}

/// <https://fetch.spec.whatwg.org/#concept-request>
#[derive(Clone, Debug)]
pub struct NavigationRequest {
    /// <https://fetch.spec.whatwg.org/#concept-request-url>
    pub url: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-method>
    pub method: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-referrer>
    pub referrer: String,
    /// <https://fetch.spec.whatwg.org/#concept-request-referrer-policy>
    pub referrer_policy: String,
    /// serialized placeholder for
    /// <https://fetch.spec.whatwg.org/#concept-request-policy-container>
    pub policy_container: Option<String>,
    /// <https://fetch.spec.whatwg.org/#concept-request-body>
    pub body: Option<String>,
}

impl NavigationRequest {
    /// <https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching>
    fn for_destination_url(
        destination_url: String,
        user_involvement: &UserNavigationInvolvement,
    ) -> Self {
        // <https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching>
        // If request's client is null, this only occurs in the case of a browser UI-initiated
        // navigation. Set request's referrer to "no-referrer".
        let referrer = if matches!(user_involvement, UserNavigationInvolvement::BrowserUi) {
            String::from("no-referrer")
        } else {
            String::from("client")
        };
        Self {
            url: destination_url,
            method: String::from("GET"),
            referrer,
            referrer_policy: String::new(),
            policy_container: None,
            body: None,
        }
    }

    /// Convert to the navigation fetch request type used for IPC with the net extension.
    fn to_navigation_fetch_request(&self) -> ipc_messages::network::NavigationFetchRequest {
        ipc_messages::network::NavigationFetchRequest {
            url: self.url.clone(),
            method: self.method.clone(),
            body: self.body.clone(),
            referrer: self.referrer.clone(),
            referrer_policy: self.referrer_policy.clone(),
        }
    }
}

/// Top-level state for the Rust user-agent thread.
///
/// This mirrors the role of `FormalWeb.UserAgent`: spec-facing global sets,
/// worker ownership, and the pending navigation/fetch continuations that connect the embedder,
/// content, fetch, and timer threads.
pub struct UserAgentState {
    /// <https://html.spec.whatwg.org/multipage/#browsing-context-group-set>
    pub browsing_context_group_set: BrowsingContextGroupSet,
    /// <https://html.spec.whatwg.org/multipage/#navigable>
    pub navigables: HashMap<NavigableId, Navigable>,
    /// <https://html.spec.whatwg.org/multipage/#tlbc-group>
    pub top_level_browsing_context_group_ids: HashMap<BrowsingContextId, BrowsingContextGroupId>,
    /// map from event-loop ids to the owned event-loop workers.
    pub event_loops: HashMap<EventLoopId, EventLoopEntry>,
    /// reverse index from top-level traversable ids to the owning event-loop id.
    pub traversable_handles: HashMap<NavigableId, EventLoopId>,
    /// last published viewport per traversable; replayed when ownership moves to a new
    /// content event loop (for example cross-origin child navigations).
    pub traversable_viewports: HashMap<NavigableId, ((u32, u32, f32, ColorScheme), f32, f32)>,
    /// cache of each traversable's active target name derived from
    /// `traversable_set`.
    pub traversable_target_names: HashMap<NavigableId, String>,
    /// cache of each traversable's active document derived from
    /// `traversable_set`.
    pub active_documents_by_traversable: HashMap<NavigableId, DocumentId>,
    /// cache of active and pending documents keyed by
    /// <https://dom.spec.whatwg.org/#concept-document> identifiers.
    pub documents: HashMap<DocumentId, DocumentState>,
    /// The latest hit-testing info for each webview, published by the
    /// graphics process alongside each composed scene.
    pub frame_hit_info: HashMap<WebviewId, Vec<ipc_messages::graphics::FrameHitInfo>>,
    /// queue of navigations paused while content runs `beforeunload`.
    pub pending_before_unload_navigations:
        HashMap<BeforeUnloadCheckId, PendingBeforeUnloadNavigation>,
    /// queue of fetch-backed navigations suspended at the response wait point.
    pub pending_navigation_fetches: HashMap<NavigationId, PendingNavigationFetch>,
    /// reverse index from <https://fetch.spec.whatwg.org/#fetch-controller> ids to
    /// pending navigation ids.
    pub pending_navigation_fetch_ids_by_fetch_id: HashMap<NavigationFetchId, NavigationId>,
    /// queue of documents waiting for
    /// <https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation>
    pub pending_navigation_finalizations: HashMap<DocumentId, PendingNavigationFinalization>,
    /// reverse index from <https://html.spec.whatwg.org/multipage/#navigation-params-id>
    /// to pending finalization document ids.
    pub pending_navigation_finalization_ids_by_navigation_id: HashMap<NavigationId, DocumentId>,
}

/// cache of the active document state held by the user agent.
#[derive(Clone)]
pub struct DocumentState {
    /// reference back to the top-level traversable that currently presents the
    /// document.
    pub traversable_id: NavigableId,
    /// reference to the active browsing context for the document.
    pub browsing_context_id: Option<BrowsingContextId>,
    /// owner event loop for the document's content implementation.
    pub event_loop_id: EventLoopId,
    /// Active document URL.
    pub url: String,
    /// flag for the initial about:blank special case.
    pub is_initial_about_blank: bool,
}

/// continuation for
/// <https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled>
#[derive(Clone)]
pub struct PendingBeforeUnloadNavigation {
    /// identifier for the active beforeunload check.
    pub check_id: BeforeUnloadCheckId,
    /// identifier corresponding to
    /// <https://html.spec.whatwg.org/multipage/#navigation-params-id>
    pub navigation_id: NavigationId,
    pub navigable_id: NavigableId,
    pub destination_url: String,
    pub user_involvement: ipc_messages::content::UserNavigationInvolvement,
    /// Documents still expected to report their beforeunload result for this navigation.
    pub pending_document_ids: HashSet<DocumentId>,
    /// Whether any descendant beforeunload handler canceled the navigation.
    pub canceled: bool,
}

/// Pending fetch-backed navigation paused at the response wait point.
#[derive(Clone)]
pub struct PendingNavigationFetch {
    /// identifier corresponding to <https://fetch.spec.whatwg.org/#fetch-controller>
    pub fetch_id: NavigationFetchId,
    /// identifier corresponding to
    /// <https://html.spec.whatwg.org/multipage/#navigation-params-id>
    pub navigation_id: NavigationId,
    pub traversable_id: NavigableId,
    pub previous_document_id: Option<DocumentId>,
    /// <https://fetch.spec.whatwg.org/#concept-request>
    pub request: NavigationRequest,
    /// <https://html.spec.whatwg.org/multipage/#source-snapshot-params>
    pub source_snapshot_params: SourceSnapshotParams,
    /// <https://html.spec.whatwg.org/multipage/#target-snapshot-params>
    pub target_snapshot_params: TargetSnapshotParams,
    /// <https://w3c.github.io/navigation-timing/#dom-navigationtimingtype>
    pub navigation_timing_type: NavigationTimingType,
    /// summary of the CSP navigation type from
    /// <https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching>
    pub csp_navigation_type: String,
    /// flag for the POST branch in
    /// <https://html.spec.whatwg.org/multipage/#attempt-to-populate-the-history-entry's-document>
    pub allow_post: bool,
    pub user_involvement: ipc_messages::content::UserNavigationInvolvement,
}

/// continuation for
/// <https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation>
#[derive(Clone)]
pub struct PendingNavigationFinalization {
    /// identifier for the loaded document that will emit the finalization signal.
    pub document_id: DocumentId,
    /// identifier corresponding to
    /// <https://html.spec.whatwg.org/multipage/#navigation-params-id>
    pub navigation_id: NavigationId,
    pub traversable_id: NavigableId,
    pub previous_document_id: Option<DocumentId>,
    /// browsing context selected for the new document before commit.
    pub browsing_context_id: Option<BrowsingContextId>,
    /// <https://html.spec.whatwg.org/multipage/#session-history-entry>
    pub history_entry: SessionHistoryEntry,
    /// <https://html.spec.whatwg.org/multipage/#history-handling-behavior>
    pub history_handling: HistoryHandlingBehavior,
    /// <https://html.spec.whatwg.org/multipage/#user-navigation-involvement>
    pub user_involvement: ipc_messages::content::UserNavigationInvolvement,
}

impl Default for UserAgentState {
    /// seeding the browser-global state owned by the user-agent thread.
    fn default() -> Self {
        Self {
            browsing_context_group_set: BrowsingContextGroupSet::default(),
            navigables: HashMap::new(),
            top_level_browsing_context_group_ids: HashMap::new(),
            event_loops: HashMap::new(),
            traversable_handles: HashMap::new(),
            traversable_viewports: HashMap::new(),
            traversable_target_names: HashMap::new(),
            active_documents_by_traversable: HashMap::new(),
            documents: HashMap::new(),
            pending_before_unload_navigations: HashMap::new(),
            pending_navigation_fetches: HashMap::new(),
            pending_navigation_fetch_ids_by_fetch_id: HashMap::new(),
            pending_navigation_finalizations: HashMap::new(),
            pending_navigation_finalization_ids_by_navigation_id: HashMap::new(),
            frame_hit_info: HashMap::new(),
        }
    }
}

impl UserAgentState {
    /// <https://html.spec.whatwg.org/multipage/#nav-document>
    fn nav_document_id(&self, navigable_id: NavigableId) -> Option<DocumentId> {
        self.navigables
            .get(&navigable_id)
            .and_then(|navigable| navigable.active_document_id)
    }

    /// <https://html.spec.whatwg.org/multipage/#top-level-traversable>
    fn top_level_traversable_id(&self, navigable_id: NavigableId) -> Option<NavigableId> {
        let mut current_id = navigable_id;
        loop {
            let navigable = self.navigables.get(&current_id)?;
            if let Some(parent_id) = navigable.parent_navigable_id {
                current_id = parent_id;
                continue;
            }
            return Some(current_id);
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#bc-tlbc>
    fn top_level_browsing_context_id(
        &self,
        browsing_context_id: BrowsingContextId,
    ) -> Option<BrowsingContextId> {
        let traversable_id = self.documents.values().find_map(|document| {
            (document.browsing_context_id == Some(browsing_context_id))
                .then_some(document.traversable_id)
        })?;
        let top_level_traversable_id = self.top_level_traversable_id(traversable_id)?;
        self.navigables
            .get(&top_level_traversable_id)
            .and_then(|navigable| navigable.active_browsing_context_id)
    }

    /// selecting the embedder-visible active
    /// <https://html.spec.whatwg.org/multipage/#top-level-traversable>.
    fn set_active_top_level_traversable(&mut self, navigable_id: NavigableId) {
        let top_level_id = self.top_level_traversable_id(navigable_id);
        for (candidate_id, navigable) in &mut self.navigables {
            if navigable.parent_navigable_id.is_some() {
                continue;
            }
            navigable.is_active = Some(*candidate_id) == top_level_id;
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#nav-document>
    fn set_navigable_active_document(
        &mut self,
        navigable_id: NavigableId,
        document_id: DocumentId,
    ) {
        self.active_documents_by_traversable
            .insert(navigable_id, document_id);
        if let Some(navigable) = self.navigables.get_mut(&navigable_id) {
            navigable.active_document_id = Some(document_id);
        }
    }

    /// caching the active browsing context selected for one traversable-backed navigable.
    fn set_navigable_active_browsing_context(
        &mut self,
        navigable_id: NavigableId,
        browsing_context_id: Option<BrowsingContextId>,
    ) {
        if let Some(navigable) = self.navigables.get_mut(&navigable_id) {
            navigable.active_browsing_context_id = browsing_context_id;
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#opener-browsing-context>
    ///
    /// Used by steps 15.3 and 16.2 of
    /// <https://html.spec.whatwg.org/multipage/#window-open-steps>.
    fn set_opener_for_browsing_context(
        &mut self,
        browsing_context_id: BrowsingContextId,
        opener_browsing_context_id: BrowsingContextId,
    ) {
        // Step 15.3 (and 16.2, same): "Set targetBrowsingContext's opener browsing
        // context to sourceBrowsingContext."
        //
        // <https://html.spec.whatwg.org/multipage/#auxiliary-browsing-context>
        // Set the browsing context's opener and mark it as auxiliary.
        //
        // Walk all browsing context groups to find this browsing context and set its opener.
        for group in self.browsing_context_group_set.members.values_mut() {
            if let Some(browsing_context) = group.browsing_context_set.get_mut(&browsing_context_id)
            {
                browsing_context.opener_browsing_context = Some(opener_browsing_context_id);
                browsing_context.is_auxiliary = true;
                return;
            }
        }
        // Also check the top-level browsing context group ids map.
        if let Some(_group_id) = self
            .top_level_browsing_context_group_ids
            .get(&browsing_context_id)
        {
            // Only set opener on the actual browsing context object, not on the map key.
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#ongoing-navigation>
    fn set_navigable_ongoing_navigation(
        &mut self,
        navigable_id: NavigableId,
        navigation_id: Option<NavigationId>,
    ) {
        if let Some(navigable) = self.navigables.get_mut(&navigable_id) {
            navigable.ongoing_navigation_id = navigation_id;
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation>
    fn commit_session_history_entry(
        &mut self,
        navigable_id: NavigableId,
        history_entry: SessionHistoryEntry,
        history_handling: HistoryHandlingBehavior,
    ) {
        if let Some(navigable) = self.navigables.get_mut(&navigable_id) {
            match history_handling {
                HistoryHandlingBehavior::Push => {
                    navigable
                        .session_history_entries
                        .retain(|entry| entry.step <= navigable.current_session_history_step);
                    let next_step = navigable.current_session_history_step.saturating_add(1);
                    navigable.current_session_history_step = next_step;
                    navigable.session_history_entries.push(SessionHistoryEntry {
                        step: next_step,
                        ..history_entry
                    });
                }
                HistoryHandlingBehavior::Replace => {
                    let current_step = navigable.current_session_history_step;
                    let replacement_entry = SessionHistoryEntry {
                        step: current_step,
                        ..history_entry
                    };
                    if let Some(entry) = navigable
                        .session_history_entries
                        .iter_mut()
                        .find(|entry| entry.step == current_step)
                    {
                        *entry = replacement_entry;
                    } else {
                        navigable.session_history_entries.push(replacement_entry);
                    }
                }
            }
        }
    }

    /// storing the pending fetch continuation of one navigation.
    fn insert_pending_navigation_fetch(&mut self, pending: PendingNavigationFetch) {
        self.pending_navigation_fetch_ids_by_fetch_id
            .insert(pending.fetch_id, pending.navigation_id);
        self.pending_navigation_fetches
            .insert(pending.navigation_id, pending);
    }

    /// removing a pending navigation fetch by navigation id.
    fn take_pending_navigation_fetch_by_navigation_id(
        &mut self,
        navigation_id: NavigationId,
    ) -> Option<PendingNavigationFetch> {
        let pending = self.pending_navigation_fetches.remove(&navigation_id)?;
        self.pending_navigation_fetch_ids_by_fetch_id
            .remove(&pending.fetch_id);
        Some(pending)
    }

    /// removing a pending navigation fetch by
    /// <https://fetch.spec.whatwg.org/#fetch-controller> id.
    fn take_pending_navigation_fetch_by_fetch_id(
        &mut self,
        fetch_id: NavigationFetchId,
    ) -> Option<PendingNavigationFetch> {
        let navigation_id = self
            .pending_navigation_fetch_ids_by_fetch_id
            .remove(&fetch_id)?;
        self.pending_navigation_fetches.remove(&navigation_id)
    }

    /// dropping all pending fetch continuations owned by one traversable.
    fn remove_pending_navigation_fetches_for_traversable(&mut self, traversable_id: NavigableId) {
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

    /// storing the pending finalization continuation of one navigation.
    fn insert_pending_navigation_finalization(&mut self, pending: PendingNavigationFinalization) {
        self.pending_navigation_finalization_ids_by_navigation_id
            .insert(pending.navigation_id, pending.document_id);
        self.pending_navigation_finalizations
            .insert(pending.document_id, pending);
    }

    /// removing a pending finalization continuation by document id.
    fn take_pending_navigation_finalization_by_document_id(
        &mut self,
        document_id: DocumentId,
    ) -> Option<PendingNavigationFinalization> {
        let pending = self.pending_navigation_finalizations.remove(&document_id)?;
        self.pending_navigation_finalization_ids_by_navigation_id
            .remove(&pending.navigation_id);
        Some(pending)
    }

    /// dropping all pending finalization continuations owned by one traversable.
    fn remove_pending_navigation_finalizations_for_traversable(
        &mut self,
        traversable_id: NavigableId,
    ) -> Vec<DocumentId> {
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

    /// removing one traversable and its derived indices from the user-agent state.
    fn remove_traversable(&mut self, traversable_id: NavigableId) {
        let Some(navigable) = self.navigables.get(&traversable_id).cloned() else {
            return;
        };
        let browsing_context_id = navigable.active_browsing_context_id;
        let removed_top_level_browsing_context_id = navigable
            .parent_navigable_id
            .is_none()
            .then_some(browsing_context_id)
            .flatten();

        self.navigables.remove(&traversable_id);
        self.traversable_handles.remove(&traversable_id);
        self.traversable_viewports.remove(&traversable_id);
        self.traversable_target_names.remove(&traversable_id);
        self.active_documents_by_traversable.remove(&traversable_id);

        if let Some(browsing_context_id) = browsing_context_id {
            self.browsing_context_group_set
                .remove_browsing_context(browsing_context_id);
        }
        if let Some(top_level_browsing_context_id) = removed_top_level_browsing_context_id {
            self.top_level_browsing_context_group_ids
                .remove(&top_level_browsing_context_id);
        }
    }
}

/// Commands that enter the user-agent thread from the embedder, event-loop workers, fetch worker,
/// and timer worker.
pub enum UserAgentCommand {
    CreateFreshTopLevelTraversable {
        destination_url: String,
    },
    /// The event loop the content process belongs to.  Required when
    /// `request.new_traversable_info` is `Some` (window.open creating a new
    /// traversable).  For existing-navigable navigations the UA looks up
    /// the event loop from its own state.
    Navigate {
        event_loop_id: Option<EventLoopId>,
        request: NavigateRequest,
    },
    CompleteBeforeUnload {
        result: BeforeUnloadResult,
    },
    FinalizeCrossDocumentNavigation {
        finalized: ContentFinalizeNavigation,
    },
    ClickElement {
        traversable_id: NavigableId,
        selector: String,
        reply: Sender<Result<(), String>>,
    },
    EvaluateScript {
        traversable_id: NavigableId,
        source: String,
        timeout: Duration,
        reply: Sender<Result<serde_json::Value, String>>,
    },
    BroadcastViewport {
        snapshot: (u32, u32, f32, ColorScheme),
    },
    SetTraversableViewport {
        traversable_id: NavigableId,
        snapshot: (u32, u32, f32, ColorScheme),
        offset_x: f32,
        offset_y: f32,
    },
    DispatchEventFor {
        traversable_id: NavigableId,
        event: String,
    },
    RenderingOpportunityFor {
        traversable_id: NavigableId,
    },
    NavigationFetchCompleted {
        fetch_id: NavigationFetchId,
        response: ContentFetchResponse,
    },
    MediaLoadRequested {
        url: String,
        document_id: DocumentId,
        traversable_id: NavigableId,
        pipeline_id: MediaPipelineId,
        video_paint_id: VideoPaintId,
    },
    NavigationFetchFailed {
        fetch_id: NavigationFetchId,
    },
    WindowTimerTask {
        event_loop_id: EventLoopId,
        document_id: DocumentId,
        timer_id: u32,
        timer_key: WindowTimerKey,
        nesting_level: u32,
    },

    IframeTraversableRemoved {
        parent_traversable_id: NavigableId,
        content_navigable_id: NavigableId,
        content_frame_id: FrameId,
        reply: Sender<Result<(), String>>,
    },
    Shutdown {
        reply: Sender<Result<(), String>>,
    },
}

/// Public handle to the dedicated user-agent thread that owns browser-global state and worker
/// lifecycles.
pub struct UserAgent {
    command_sender: Sender<UserAgentCommand>,
    join_handle: Option<JoinHandle<()>>,
}

impl UserAgent {
    /// spawning the dedicated user-agent thread owned by the webview layer.
    pub fn start(
        host: Arc<dyn Embedder>,
        webview_provider_sender: Sender<WebviewProviderMessage>,
        trace_sender: Option<TraceSender>,
    ) -> Result<Self, String> {
        let (command_sender, command_receiver) = unbounded();
        let mut worker = UserAgentWorker::new(
            command_sender.clone(),
            command_receiver,
            host,
            webview_provider_sender,
            trace_sender,
        );
        let join_handle = thread::Builder::new()
            .name(String::from("formal-web:user-agent"))
            .spawn(move || worker.run())
            .unwrap_or_else(|error| {
                panic!("failed to spawn formal-web-user-agent thread: {error}")
            });
        Ok(Self {
            command_sender,
            join_handle: Some(join_handle),
        })
    }

    /// shutting down the owned user-agent thread and its child workers.
    fn shutdown_inner(&mut self) -> Result<(), String> {
        let Some(join_handle) = self.join_handle.take() else {
            return Ok(());
        };

        let (reply_sender, reply_receiver) = bounded(1);
        self.command_sender
            .send(UserAgentCommand::Shutdown {
                reply: reply_sender,
            })
            .map_err(|error| format!("failed to request user-agent shutdown: {error}"))?;
        let shutdown_result = reply_receiver
            .recv()
            .map_err(|error| format!("user-agent shutdown reply channel closed: {error}"))?;

        if join_handle.join().is_err() && shutdown_result.is_ok() {
            return Err(String::from("user-agent thread panicked"));
        }

        shutdown_result
    }

    /// the webview automation hook that delegates to `UserAgentApi`.
    pub fn evaluate_script(
        &self,
        traversable_id: NavigableId,
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
        let result = reply_receiver.recv_timeout(timeout).map_err(|error| {
            format!(
                "timed out after {} ms waiting for script evaluation reply: {error}",
                timeout.as_millis()
            )
        })?;
        result
    }
}

impl Drop for UserAgent {
    /// best-effort shutdown of the owned user-agent thread.
    fn drop(&mut self) {
        if let Err(error) = self.shutdown_inner() {
            error!("failed to shut down user-agent thread: {error}");
        }
    }
}

impl UserAgent {
    /// <https://html.spec.whatwg.org/multipage/#create-a-fresh-top-level-traversable>
    pub fn start_top_level_traversable(&self, destination_url: String) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::CreateFreshTopLevelTraversable { destination_url })
            .map_err(|error| {
                format!("failed to start create-a-fresh-top-level-traversable: {error}")
            })
    }

    /// <https://html.spec.whatwg.org/multipage/#navigate>
    /// Initiates navigation from outside the content event loop (e.g.
    /// browser-chrome URL bar, automation).  `event_loop_id` is `None`
    /// because the event loop is not known at this call site; the UA
    /// looks it up from navigable state in `handle_navigate`.
    pub fn start_navigation(&self, request: NavigateRequest) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::Navigate {
                event_loop_id: None,
                request,
            })
            .map_err(|error| format!("failed to send navigate command: {error}"))
    }

    /// queuing DOM event dispatch on the traversable's owning
    /// <https://html.spec.whatwg.org/multipage/#event-loop>.
    pub fn dispatch_event_for(
        &self,
        traversable_id: NavigableId,
        event: String,
    ) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::DispatchEventFor {
                traversable_id,
                event,
            })
            .map_err(|error| format!("failed to send dispatch-event request: {error}"))
    }

    /// <https://html.spec.whatwg.org/multipage/#update-the-rendering>
    pub fn note_rendering_opportunity(&self, traversable_id: NavigableId) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::RenderingOpportunityFor { traversable_id })
            .map_err(|error| format!("failed to send rendering-opportunity request: {error}"))
    }

    /// broadcasting the embedder viewport to every owned content event loop.
    pub fn set_default_viewport(
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

    /// updating the viewport of one traversable's content implementation.
    pub fn set_traversable_viewport(
        &self,
        traversable_id: NavigableId,
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

    /// the automation-only selector-click bridge into content.
    pub fn click_element(
        &self,
        traversable_id: NavigableId,
        selector: String,
    ) -> Result<(), String> {
        let (reply_sender, reply_receiver) = bounded(1);
        self.command_sender
            .send(UserAgentCommand::ClickElement {
                traversable_id,
                selector,
                reply: reply_sender,
            })
            .map_err(|error| format!("failed to send selector click request: {error}"))?;
        reply_receiver
            .recv()
            .map_err(|error| format!("selector click reply channel closed: {error}"))?
    }
}

/// render-state debug output on the user-agent thread.
fn log_render_state_debug(message: impl AsRef<str>) {
    if std::env::var_os("FORMAL_WEB_DEBUG_RENDER_STATE").is_some() {
        debug!("[render-state][user-agent] {}", message.as_ref());
    }
}

fn input_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_INPUT").is_some()
}

/// <https://html.spec.whatwg.org/multipage/#the-rules-for-choosing-a-navigable>
fn normalize_navigation_target_name(target_name: &str) -> String {
    if target_name.eq_ignore_ascii_case("_self") {
        String::new()
    } else {
        target_name.to_owned()
    }
}

/// keeping synthetic iframe traversables out of the embedder's active
/// browser-ui selection.
fn target_name_keeps_browser_ui_focus(target_name: &str) -> bool {
    !target_name.starts_with("_iframe|")
}

/// <https://html.spec.whatwg.org/multipage/#same-site>
/// Note: This helper reduces the spec's same-site comparison to a same-origin fast path plus a
/// scheme and registrable-domain comparison for host-based URLs. Hostless URLs such as `file:`
/// fall back to the same-origin branch and otherwise compare as cross-site.
fn is_same_site(parent_url: &str, destination_url: &str) -> Result<bool, String> {
    let parent = Url::parse(parent_url)
        .map_err(|error| format!("failed to parse parent URL {parent_url:?}: {error}"))?;

    let destination = Url::parse(destination_url)
        .map_err(|error| format!("failed to parse destination URL {destination_url:?}: {error}"))?;

    let same_origin = parent.scheme().eq_ignore_ascii_case(destination.scheme())
        && parent.host_str() == destination.host_str()
        && parent.port_or_known_default() == destination.port_or_known_default();
    if same_origin {
        return Ok(true);
    }

    let parent_scheme = parent.scheme().to_ascii_lowercase();
    let Some(parent_host) = parent.host_str().map(str::to_ascii_lowercase) else {
        return Ok(false);
    };
    let parent_domain = psl::domain_str(&parent_host)
        .map(str::to_owned)
        .unwrap_or(parent_host);

    let destination_scheme = destination.scheme().to_ascii_lowercase();
    let Some(destination_host) = destination.host_str().map(str::to_ascii_lowercase) else {
        return Ok(false);
    };
    let destination_domain = psl::domain_str(&destination_host)
        .map(str::to_owned)
        .unwrap_or(destination_host);

    Ok((parent_scheme, parent_domain) == (destination_scheme, destination_domain))
}

/// <https://html.spec.whatwg.org/multipage/#same-site>
/// Note: This helper continues the same-site comparison for callers that need the cross-origin
/// branch predicate used by `initialise_the_document_object`.
fn is_cross_origin_navigation(parent_url: &str, destination_url: &str) -> Result<bool, String> {
    is_same_site(parent_url, destination_url).map(|same_site| !same_site)
}

fn content_process_label_from_url(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_owned))
        .unwrap_or_else(|| String::from("about:blank"))
}

fn child_navigable_ids_by_parent(state: &UserAgentState) -> HashMap<NavigableId, Vec<NavigableId>> {
    let mut children_by_parent = HashMap::new();
    for (candidate_id, navigable) in &state.navigables {
        let Some(parent_id) = navigable.parent_navigable_id else {
            continue;
        };
        children_by_parent
            .entry(parent_id)
            .or_insert_with(Vec::new)
            .push(*candidate_id);
    }
    children_by_parent
}

fn descendant_navigable_ids_matching(
    state: &UserAgentState,
    root_navigable_id: NavigableId,
    children_by_parent: &HashMap<NavigableId, Vec<NavigableId>>,
    include_child: impl Fn(&Navigable) -> bool,
) -> Vec<NavigableId> {
    let mut descendants = Vec::new();
    let mut stack = vec![root_navigable_id];
    let mut visited = HashSet::from([root_navigable_id]);

    while let Some(parent_id) = stack.pop() {
        let Some(child_ids) = children_by_parent.get(&parent_id) else {
            continue;
        };
        for child_id in child_ids {
            let Some(navigable) = state.navigables.get(child_id) else {
                continue;
            };
            if !include_child(navigable) || !visited.insert(*child_id) {
                continue;
            }
            descendants.push(*child_id);
            stack.push(*child_id);
        }
    }

    descendants
}

fn descendant_navigable_ids(state: &UserAgentState, navigable_id: NavigableId) -> Vec<NavigableId> {
    let children_by_parent = child_navigable_ids_by_parent(state);
    descendant_navigable_ids_matching(state, navigable_id, &children_by_parent, |_| true)
}

/// <https://html.spec.whatwg.org/multipage/#find-a-navigable-by-target-name>
fn find_navigable_by_target_name(state: &UserAgentState, target_name: &str) -> Option<NavigableId> {
    state
        .navigables
        .iter()
        .find_map(|(navigable_id, navigable)| {
            (navigable.target_name == target_name && !navigable.target_name.is_empty())
                .then_some(*navigable_id)
        })
}

/// user-agent thread coordinates.
struct UserAgentWorker {
    state: UserAgentState,
    command_sender: Sender<UserAgentCommand>,
    command_receiver: Receiver<UserAgentCommand>,
    /// Owns the IPC connection to the net extension and tracks pending navigation fetches.
    net_connection: crate::fetch::NetConnection,
    timer_command_sender: Sender<TimerCommand>,
    timer_join_handle: Option<JoinHandle<()>>,
    /// Crossbeam proxy for media extension events.
    media_event_receiver:
        crossbeam_channel::Receiver<ipc::IpcIncoming<ipc_messages::media::MediaEvent>>,
    /// Child process handle for the media process.
    media_child: Option<std::process::Child>,
    /// IPC sender to the media extension (for direct content connections).
    media_extension_sender: Option<ipc::IpcSender<ipc_messages::media::MediaCommand>>,
    /// Maps media pipeline IDs to their owning webview and paint ID, so that
    /// incoming video frames can be routed to the correct compositor slot.
    pipeline_to_webview: HashMap<MediaPipelineId, (WebviewId, VideoPaintId)>,
    /// IPC sender to the graphics process.
    graphics_extension_sender: Option<ipc::IpcSender<ipc_messages::graphics::GraphicsCommand>>,
    /// Crossbeam proxy for graphics extension events (composed scenes).
    graphics_event_receiver:
        crossbeam_channel::Receiver<ipc::IpcIncoming<ipc_messages::graphics::GraphicsEvent>>,
    /// Child process handle for the graphics process.
    graphics_child: Option<std::process::Child>,

    /// Host integration used to surface navigation, paint, clipboard, and viewport state.
    host: Arc<dyn Embedder>,
    /// Sender for webview-provider updates that must be drained by host sync calls.
    webview_provider_sender: Sender<WebviewProviderMessage>,
    /// Trace logger for the Navigation TLA+ spec.
    navigation_tracer: TLATracer,
    /// Sender cloned into child workers and sidecars when TLA tracing is enabled.
    trace_sender: Option<TraceSender>,
    /// request ids for automation round-trips across the user-agent and
    /// content event-loop boundary.
    next_automation_request_id: u64,
}

impl UserAgentWorker {
    /// starting the fetch and timer workers owned by the user-agent thread.
    fn new(
        user_agent_command_sender: Sender<UserAgentCommand>,
        command_receiver: Receiver<UserAgentCommand>,
        host: Arc<dyn Embedder>,
        webview_provider_sender: Sender<WebviewProviderMessage>,
        trace_sender: Option<TraceSender>,
    ) -> Self {
        let net_connection = crate::fetch::NetConnection::new(trace_sender.clone())
            .unwrap_or_else(|error| panic!("failed to start net extension: {error}"));
        let (timer_command_sender, timer_command_receiver) = unbounded();
        let timer_user_agent_command_sender = user_agent_command_sender.clone();
        let timer_trace_sender = trace_sender.clone();
        let timer_join_handle = thread::Builder::new()
            .name(String::from("formal-web:timer"))
            .spawn(move || {
                run_timer_thread(
                    timer_command_receiver,
                    timer_user_agent_command_sender,
                    timer_trace_sender,
                )
            })
            .unwrap_or_else(|error| panic!("failed to spawn formal-web-timer thread: {error}"));

        // Start the graphics process (always — handles composition even without media).
        let (graphics_extension_sender, graphics_event_receiver, graphics_child) = {
            use crate::ipc_manifest::GraphicsExtensionManifest;
            match ipc::ExtensionHandle::launch::<
                GraphicsExtensionManifest,
                ipc_messages::graphics::GraphicsCommand,
                ipc_messages::graphics::GraphicsEvent,
            >(&GraphicsExtensionManifest)
            {
                Ok((mut handle, connection)) => {
                    let sender = connection.sender.clone();
                    let receiver = connection.receiver;
                    let child = handle.take_child();
                    (Some(sender), ipc::crossbeam_proxy(receiver), child)
                }
                Err(error) => {
                    log::error!("failed to start graphics process: {error}");
                    let (dummy_tx, dummy_rx) = crossbeam_channel::unbounded();
                    drop(dummy_tx);
                    (None, dummy_rx, None)
                }
            }
        };

        #[cfg(feature = "media")]
        let (media_extension_sender, media_event_receiver, media_child) = {
            use crate::ipc_manifest::MediaExtensionManifest;
            let (mut handle, connection) = ipc::ExtensionHandle::launch::<
                MediaExtensionManifest,
                ipc_messages::media::MediaCommand,
                ipc_messages::media::MediaEvent,
            >(&MediaExtensionManifest)
            .unwrap_or_else(|error| panic!("failed to start media extension: {error}"));
            let sender = connection.sender.clone();
            let receiver = connection.receiver;
            let child = handle.take_child();
            (Some(sender), ipc::crossbeam_proxy(receiver), child)
        };
        #[cfg(not(feature = "media"))]
        let (media_extension_sender, media_event_receiver, media_child): (
            Option<ipc::IpcSender<ipc_messages::media::MediaCommand>>,
            crossbeam_channel::Receiver<ipc::IpcIncoming<ipc_messages::media::MediaEvent>>,
            Option<std::process::Child>,
        ) = {
            let (dummy_tx, dummy_rx) = crossbeam_channel::unbounded();
            drop(dummy_tx);
            (None, dummy_rx, None)
        };

        Self {
            state: UserAgentState::default(),
            command_sender: user_agent_command_sender.clone(),
            command_receiver,
            net_connection,
            timer_command_sender,
            timer_join_handle: Some(timer_join_handle),
            media_event_receiver,
            media_child,
            pipeline_to_webview: HashMap::new(),
            graphics_extension_sender,
            graphics_event_receiver,
            graphics_child,
            host,
            webview_provider_sender,
            navigation_tracer: TLATracer::new(
                "Navigation",
                "formal-web:user-agent",
                trace_sender.clone(),
            ),
            trace_sender,
            next_automation_request_id: 1,
            media_extension_sender,
        }
    }

    /// the top-level command loop that owns browser-global coordination.
    /// Also processes net responses (navigation fetch results) via `select!`.
    fn run(&mut self) {
        loop {
            select! {
                    recv(self.command_receiver) -> command => {
                        let Ok(command) = command else { break; };
                        match command {
                    UserAgentCommand::CreateFreshTopLevelTraversable { destination_url } => {
                        self.create_a_fresh_top_level_traversable(destination_url);
                    }
                    UserAgentCommand::Navigate {
                        event_loop_id,
                        request,
                    } => {
                        self.handle_navigate(event_loop_id, request);
                    }
                    UserAgentCommand::CompleteBeforeUnload { result } => {
                        self.handle_complete_before_unload(result);
                    }
                    UserAgentCommand::FinalizeCrossDocumentNavigation { finalized } => {
                        self.handle_finalize_cross_document_navigation(finalized);
                    }
                    UserAgentCommand::ClickElement {
                        traversable_id,
                        selector,
                        reply,
                    } => {
                        self.handle_click_element(traversable_id, selector, reply);
                    }
                    UserAgentCommand::EvaluateScript {
                        traversable_id,
                        source,
                        timeout,
                        reply,
                    } => {
                        self.handle_evaluate_script(traversable_id, source, timeout, reply);
                    }
                    UserAgentCommand::BroadcastViewport { snapshot } => {
                        self.handle_set_default_viewport(snapshot);
                    }
                    UserAgentCommand::SetTraversableViewport {
                        traversable_id,
                        snapshot,
                        offset_x,
                        offset_y,
                    } => {
                        self.handle_set_traversable_viewport(
                            traversable_id,
                            snapshot,
                            offset_x,
                            offset_y,
                        );
                    }
                    UserAgentCommand::DispatchEventFor {
                        traversable_id,
                        event,
                    } => {
                        self.handle_dispatch_event_for(traversable_id, event);
                    }
                    UserAgentCommand::RenderingOpportunityFor { traversable_id } => {
                        self.handle_rendering_opportunity_for(traversable_id);
                    }
                    UserAgentCommand::NavigationFetchCompleted { fetch_id, response } => {
                        self.handle_navigation_fetch_completed(fetch_id, response);
                    }
                    UserAgentCommand::NavigationFetchFailed { fetch_id } => {
                        self.handle_navigation_fetch_failed(fetch_id);
                    }
                    UserAgentCommand::WindowTimerTask {
                        event_loop_id,
                        document_id,
                        timer_id,
                        timer_key,
                        nesting_level,
                    } => {
                        self.handle_window_timer_task(
                            event_loop_id,
                            document_id,
                            timer_id,
                            timer_key,
                            nesting_level,
                        );
                    }

                    UserAgentCommand::MediaLoadRequested {
                        url,
                        document_id: _document_id,
                        traversable_id,
                        pipeline_id,
                        video_paint_id,
                    } => {
                        debug!(
                            "[media] registering pipeline url={} traversable={}",
                            url, traversable_id
                        );
                        self.register_media_pipeline(
                            pipeline_id,
                            traversable_id,
                            video_paint_id,
                        );
                    }
                    UserAgentCommand::IframeTraversableRemoved {
                        parent_traversable_id,
                        content_navigable_id,
                        content_frame_id,
                        reply,
                    } => {
                        self.handle_iframe_traversable_removed(
                            parent_traversable_id,
                            content_navigable_id,
                            content_frame_id,
                            reply,
                        );
                    }
                    UserAgentCommand::Shutdown { reply } => {
                        self.handle_shutdown(reply);
                        break;
                    }
                }
            }
                    recv(self.net_connection.receiver()) -> response => {
                        let Ok(incoming) = response else { break; };
                        self.handle_net_navigation_response(incoming.payload);
                    }
                    recv(self.graphics_event_receiver) -> event => {
                        let Ok(mut incoming) = event else { break; };
                        self.handle_graphics_event(&mut incoming);
                    }
                    recv(self.media_event_receiver) -> event => {
                        let Ok(mut incoming) = event else { break; };
                        // Extract video frame data from shared memory before forwarding.
                        if let ipc_messages::media::MediaEvent::Frame(video_frame) = &mut incoming.payload {
                            if let Some(region) = incoming.shmem_regions.get(&0) {
                                video_frame.data = region.as_slice().to_vec();
                            }
                        }
                        self.handle_media_event(incoming.payload);
                    }
                }
        }
    }

    /// Handle a navigation fetch response received directly from the net process.
    fn handle_net_navigation_response(&mut self, response: ipc_messages::network::Response) {
        let Some((fetch_id, result)) = self.net_connection.handle_response(response) else {
            return;
        };

        match result {
            Ok(fetch_response) => {
                self.handle_navigation_fetch_completed(fetch_id, fetch_response);
            }
            Err(error) => {
                log::error!("navigation fetch failed: {error}");
                self.handle_navigation_fetch_failed(fetch_id);
            }
        }
    }
}

impl UserAgentWorker {
    /// the request/reply path that sends one command through the owning
    /// event loop and waits for the content-side reply.
    fn send_event_loop_command(
        &self,
        command_sender: &Sender<EventLoopCommand>,
        command: ContentCommand,
    ) -> Result<Option<NavigableId>, String> {
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

    /// resolving the event-loop command channel that owns one traversable.
    fn command_sender_for_traversable(
        &self,
        traversable_id: NavigableId,
    ) -> Result<Sender<EventLoopCommand>, String> {
        let event_loop_id = self
            .state
            .traversable_handles
            .get(&traversable_id)
            .copied()
            .ok_or_else(|| format!("unknown traversable id: {traversable_id}"))?;
        self.state
            .event_loops
            .get(&event_loop_id)
            .map(|entry| entry.command_sender.clone())
            .ok_or_else(|| format!("missing event loop for id {event_loop_id}"))
    }

    /// <https://html.spec.whatwg.org/multipage/#create-an-agent>
    fn create_agent(&mut self, can_block: bool, process_label: String) -> Result<Agent, String> {
        // Step 1: Let signifier be a new unique internal value.
        let agent_id = AgentId::new();
        // Step 2: Let candidateExecution be a new candidate execution.
        // The Rust model does not surface a separate candidate-execution object because the
        // dedicated event-loop thread owns the scheduling state that HTML leaves implementation-defined.
        // Step 4: Set agent's event loop to a new event loop.
        let event_loop_id = EventLoopId::new();
        let entry = spawn_event_loop_entry(
            event_loop_id,
            process_label,
            self.command_sender.clone(),
            self.timer_command_sender.clone(),
            self.host.clone(),
            self.webview_provider_sender.clone(),
            self.trace_sender.clone(),
            self.net_connection.sender(),
            self.media_extension_sender.clone(),
            self.graphics_extension_sender.clone(),
        )?;
        self.state.event_loops.insert(event_loop_id, entry);
        // Step 3: Let agent be a new agent whose [[CanBlock]] is canBlock, [[Signifier]] is
        // signifier, [[CandidateExecution]] is candidateExecution, and [[IsLockFree1]],
        // [[IsLockFree2]], and [[LittleEndian]] are set at the implementation's discretion.
        // Note: The returned `Agent` stores the modeled Rust-visible fields directly, while the
        // implementation-defined lock-free details remain implicit.
        // Step 5: Return agent.
        Ok(Agent {
            id: agent_id,
            can_block,
            event_loop_id,
        })
    }

    /// <https://html.spec.whatwg.org/multipage/#creating-a-new-top-level-traversable>
    fn create_new_top_level_traversable(
        &mut self,
        target_name: String,
    ) -> Result<NavigableId, String> {
        let traversable_id = NavigableId::new();
        let iframe_parent_traversable_id = None;
        let frame_id = None;

        // Step 2: With a null opener, create a new top-level browsing context and document.
        let browsing_context_group_id = self.state.browsing_context_group_set.next_group_id();
        let browsing_context_id = BrowsingContextId::new();
        let agent_cluster_id = AgentClusterId::new();
        let agent = self.create_agent(false, String::from("about:blank"))?;
        let document_id = DocumentId::new();
        let command_sender = self
            .state
            .event_loops
            .get(&agent.event_loop_id)
            .map(|entry| entry.command_sender.clone())
            .ok_or_else(|| format!("missing event loop entry for id {}", agent.event_loop_id))?;

        if startup_debug_enabled() {
            trace!(
                "[startup-debug][user-agent] create_new_top_level_traversable sending CreateEmptyDocument traversable={} document={} event_loop={}",
                traversable_id, document_id, agent.event_loop_id
            );
        }

        // Step 4: Let documentState be a new document state, with
        // The Rust model splits document-state fields across `Traversable`,
        // `DocumentState`, and `traversable_target_names`.
        // Step 5: Let traversable be a new traversable navigable.
        // Step 6: Initialize the navigable traversable given documentState.
        self.send_event_loop_command(
            &command_sender,
            ContentCommand::CreateEmptyDocument {
                traversable_id,
                document_id,
                frame_id: None,
                parent_traversable_id: None,
                top_level_traversable_id: traversable_id,
            },
        )?;

        if startup_debug_enabled() {
            trace!(
                "[startup-debug][user-agent] create_new_top_level_traversable CreateEmptyDocument queued traversable={} document={} event_loop={}",
                traversable_id, document_id, agent.event_loop_id
            );
        }

        self.state
            .event_loops
            .get_mut(&agent.event_loop_id)
            .expect("event loop entry disappeared during top-level creation")
            .traversable_ids
            .insert(traversable_id);
        self.state
            .traversable_handles
            .insert(traversable_id, agent.event_loop_id);
        self.state
            .traversable_target_names
            .insert(traversable_id, target_name.clone());
        self.state
            .set_navigable_active_document(traversable_id, document_id);
        self.state
            .top_level_browsing_context_group_ids
            .insert(browsing_context_id, browsing_context_group_id);
        self.state.browsing_context_group_set.members.insert(
            browsing_context_group_id,
            BrowsingContextGroup {
                id: browsing_context_group_id,
                browsing_context_set: HashMap::from([(
                    browsing_context_id,
                    BrowsingContext {
                        id: browsing_context_id,
                        is_auxiliary: false,
                        opener_browsing_context: None,
                        is_popup: false,
                    },
                )]),
                agent_cluster_map: HashMap::from([(
                    AgentClusterKey::Site(String::from("about:blank")),
                    AgentCluster {
                        id: agent_cluster_id,
                        cross_origin_isolation_mode: CrossOriginIsolationMode::None,
                        is_origin_keyed: false,
                        similar_origin_window_agent: agent.clone(),
                    },
                )]),
                historical_agent_cluster_key_map: HashMap::new(),
                cross_origin_isolation_mode: CrossOriginIsolationMode::None,
            },
        );
        // Step 7: Let initialHistoryEntry be traversable's active session history entry.
        // Note: The initial session history entry is materialized directly in the literal below
        // instead of through a separate temporary binding.
        // Step 8: Set initialHistoryEntry's step to 0.
        // Note: The same literal below stores step `0` directly on the inserted entry.
        // Step 9: Append initialHistoryEntry to traversable's session history entries.
        // Note: The `session_history_entries` vector below performs the initial append.
        // Step 10: If opener is non-null, then legacy-clone a traversable storage shed given
        // opener's top-level traversable and traversable.
        // Note: This helper models the null-opener branch only, so it intentionally skips storage
        // shed cloning.
        // Step 11: Append traversable to the user agent's top-level traversable set.
        self.state.navigables.insert(
            traversable_id,
            Navigable {
                id: traversable_id,
                parent_navigable_id: iframe_parent_traversable_id,
                active_document_id: Some(document_id),
                is_active: false,
                target_name: target_name.clone(),
                active_browsing_context_id: Some(browsing_context_id),
                event_loop_id: Some(agent.event_loop_id),
                handle: Some(agent.event_loop_id),
                ongoing_navigation_id: None,
                has_deferred_update_the_rendering: false,
                frame_id,
                current_session_history_step: 0,
                session_history_entries: vec![SessionHistoryEntry {
                    step: 0,
                    document_id,
                    url: String::from("about:blank"),
                }],
            },
        );
        verification::tla_log!(self.navigation_tracer, "CreateNavigable", traversable_id);
        if target_name_keeps_browser_ui_focus(&target_name) {
            self.state.set_active_top_level_traversable(traversable_id);
        }
        self.state.documents.insert(
            document_id,
            DocumentState {
                traversable_id,
                browsing_context_id: Some(browsing_context_id),
                event_loop_id: agent.event_loop_id,
                url: String::from("about:blank"),
                is_initial_about_blank: true,
            },
        );

        // Step 12: Invoke WebDriver BiDi navigable created with traversable and
        // openerNavigableForWebDriver.
        // The embedder notification is the model's observable hook for a new top-level
        // traversable.
        if startup_debug_enabled() {
            trace!(
                "[startup-debug][user-agent] create_new_top_level_traversable traversable={} target_name={}",
                traversable_id, target_name
            );
        }
        self.host
            .new_webview(WebviewId(traversable_id), target_name.clone())?;
        self.webview_provider_sender
            .send(WebviewProviderMessage::NewWebview {
                webview_id: WebviewId(traversable_id),
            })
            .map_err(|error| {
                format!("failed to enqueue webview-provider new-webview message: {error}")
            })?;
        // Register the webview with the graphics process.
        if let Some(graphics_sender) = &self.graphics_extension_sender {
            if let Err(error) =
                graphics_sender.send(ipc_messages::graphics::GraphicsCommand::RegisterWebview {
                    webview_id: WebviewId(traversable_id),
                })
            {
                error!("failed to register webview with graphics process: {error}");
            }
        }
        self.host.webview_provider_sync()?;
        // Step 13: Return traversable.
        Ok(traversable_id)
    }

    /// <https://html.spec.whatwg.org/#create-a-new-child-navigable>
    /// Note: This helper materializes the user-agent state for an iframe's initial about:blank
    /// child navigable and reuses the parent's event loop until a later cross-origin navigation
    /// causes `initialise_the_document_object` to move it.
    fn create_new_child_navigable(
        &mut self,
        parent_navigable_id: NavigableId,
        content_navigable_id: NavigableId,
        content_frame_id: FrameId,
        document_id: DocumentId,
        target_name: Option<String>,
    ) -> Result<NavigableId, String> {
        let _requested_target_name = target_name;
        // TODO: Store requested iframe `name` attribute on document state once child-target
        // lookup uses document-state target names.
        let target_name =
            iframe_target_name(parent_navigable_id, content_navigable_id, content_frame_id);
        if let Some(navigable_id) = find_navigable_by_target_name(&self.state, &target_name) {
            return Ok(navigable_id);
        }

        let parent_navigable = self
            .state
            .navigables
            .get(&parent_navigable_id)
            .cloned()
            .ok_or_else(|| format!("missing parent navigable {parent_navigable_id}"))?;
        let parent_browsing_context_id =
            parent_navigable.active_browsing_context_id.ok_or_else(|| {
                format!("parent navigable {parent_navigable_id} has no active browsing context")
            })?;
        let parent_event_loop_id = parent_navigable
            .event_loop_id
            .ok_or_else(|| format!("parent navigable {parent_navigable_id} has no event loop"))?;
        let top_level_browsing_context_id = self
            .state
            .top_level_browsing_context_id(parent_browsing_context_id)
            .unwrap_or(parent_browsing_context_id);
        let browsing_context_id = BrowsingContextId::new();
        let traversable_id = content_navigable_id;

        let group_id = self
            .state
            .top_level_browsing_context_group_ids
            .get(&top_level_browsing_context_id)
            .copied()
            .ok_or_else(|| {
                format!(
                    "missing browsing context group for top-level browsing context {top_level_browsing_context_id}"
                )
            })?;
        self.state
            .browsing_context_group_set
            .members
            .get_mut(&group_id)
            .ok_or_else(|| format!("missing browsing context group {group_id}"))?
            .browsing_context_set
            .insert(
                browsing_context_id,
                BrowsingContext {
                    id: browsing_context_id,
                    is_auxiliary: false,
                    opener_browsing_context: None,
                    is_popup: false,
                },
            );

        self.state
            .traversable_handles
            .insert(traversable_id, parent_event_loop_id);
        self.state
            .traversable_target_names
            .insert(traversable_id, target_name.clone());
        self.state
            .set_navigable_active_document(traversable_id, document_id);
        // Note: The content process has already created the document and registered it.
        // The UA-side document state tracks the navigable-to-document mapping for
        // navigation and session history purposes.
        self.state.documents.insert(
            document_id,
            DocumentState {
                traversable_id,
                browsing_context_id: Some(browsing_context_id),
                event_loop_id: parent_event_loop_id,
                url: String::from("about:blank"),
                is_initial_about_blank: true,
            },
        );

        self.state.navigables.insert(
            traversable_id,
            Navigable {
                id: traversable_id,
                parent_navigable_id: Some(parent_navigable_id),
                active_document_id: Some(document_id),
                is_active: false,
                target_name: target_name.clone(),
                active_browsing_context_id: Some(browsing_context_id),
                event_loop_id: Some(parent_event_loop_id),
                handle: Some(parent_event_loop_id),
                ongoing_navigation_id: None,
                has_deferred_update_the_rendering: false,
                frame_id: Some(content_frame_id),
                current_session_history_step: 0,
                session_history_entries: vec![SessionHistoryEntry {
                    step: 0,
                    document_id,
                    url: String::from("about:blank"),
                }],
            },
        );
        verification::tla_log!(
            self.navigation_tracer,
            "CreateChildNavigable",
            traversable_id,
            parent_navigable_id
        );
        self.state
            .event_loops
            .get_mut(&parent_event_loop_id)
            .ok_or_else(|| format!("missing parent event loop {parent_event_loop_id}"))?
            .traversable_ids
            .insert(traversable_id);

        self.webview_provider_sender
            .send(WebviewProviderMessage::RegisterChildNavigableHost {
                child_webview_id: WebviewId(traversable_id),
                parent_traversable_id: WebviewId(parent_navigable_id),
                content_frame_id,
            })
            .map_err(|error| {
                format!(
                    "failed to enqueue webview-provider child-host registration message: {error}"
                )
            })?;
        // Register the child navigable with the graphics process.
        if let Some(graphics_sender) = &self.graphics_extension_sender {
            if let Err(error) = graphics_sender.send(
                ipc_messages::graphics::GraphicsCommand::RegisterChildNavigableHost {
                    child_webview_id: WebviewId(traversable_id),
                    parent_traversable_id: WebviewId(parent_navigable_id),
                    content_frame_id,
                },
            ) {
                error!("failed to register child navigable with graphics process: {error}");
            }
            // Also register the child webview itself.
            if let Err(error) =
                graphics_sender.send(ipc_messages::graphics::GraphicsCommand::RegisterWebview {
                    webview_id: WebviewId(traversable_id),
                })
            {
                error!("failed to register child webview with graphics process: {error}");
            }
        }
        self.host.webview_provider_sync()?;

        Ok(traversable_id)
    }

    /// <https://html.spec.whatwg.org/multipage/browsers.html#obtain-browsing-context-navigation>
    /// Note: The current model uses the active document URL plus a same-site check as the
    /// observable approximation for swap-group decisions before response-driven document creation.
    fn obtain_browsing_context_to_use_for_navigation_response(
        &mut self,
        traversable_id: NavigableId,
        destination_url: &str,
    ) -> Result<BrowsingContextNavigationSelection, String> {
        let navigable = self
            .state
            .navigables
            .get(&traversable_id)
            .cloned()
            .ok_or_else(|| format!("missing traversable {traversable_id}"))?;
        let browsing_context_id = navigable.active_browsing_context_id.ok_or_else(|| {
            format!("traversable {traversable_id} has no active browsing context")
        })?;

        if navigable.parent_navigable_id.is_some() {
            return Ok(BrowsingContextNavigationSelection {
                browsing_context_id,
                swapped_group: false,
            });
        }

        let source_document_url = self
            .state
            .nav_document_id(traversable_id)
            .and_then(|document_id| self.state.documents.get(&document_id))
            .map(|document| document.url.clone());
        let swap_group = match source_document_url {
            Some(source_document_url) => {
                is_cross_origin_navigation(&source_document_url, destination_url)?
            }
            None => false,
        };
        if !swap_group {
            return Ok(BrowsingContextNavigationSelection {
                browsing_context_id,
                swapped_group: false,
            });
        }

        let new_group_id = self.state.browsing_context_group_set.next_group_id();
        let new_browsing_context_id = BrowsingContextId::new();
        self.state.browsing_context_group_set.members.insert(
            new_group_id,
            BrowsingContextGroup {
                id: new_group_id,
                browsing_context_set: HashMap::from([(
                    new_browsing_context_id,
                    BrowsingContext {
                        id: new_browsing_context_id,
                        is_auxiliary: false,
                        opener_browsing_context: None,
                        is_popup: false,
                    },
                )]),
                agent_cluster_map: HashMap::new(),
                historical_agent_cluster_key_map: HashMap::new(),
                cross_origin_isolation_mode: CrossOriginIsolationMode::None,
            },
        );
        self.state
            .top_level_browsing_context_group_ids
            .insert(new_browsing_context_id, new_group_id);
        Ok(BrowsingContextNavigationSelection {
            browsing_context_id: new_browsing_context_id,
            swapped_group: true,
        })
    }

    fn discard_provisional_browsing_context(
        &mut self,
        traversable_id: NavigableId,
        browsing_context_id: Option<BrowsingContextId>,
    ) {
        let Some(browsing_context_id) = browsing_context_id else {
            return;
        };
        let is_top_level = self
            .state
            .navigables
            .get(&traversable_id)
            .is_some_and(|navigable| navigable.parent_navigable_id.is_none());
        let is_active = self
            .state
            .navigables
            .get(&traversable_id)
            .and_then(|navigable| navigable.active_browsing_context_id)
            == Some(browsing_context_id);
        if !is_top_level || is_active {
            return;
        }
        self.state
            .top_level_browsing_context_group_ids
            .remove(&browsing_context_id);
        self.state
            .browsing_context_group_set
            .remove_browsing_context(browsing_context_id);
    }

    /// <https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching>
    /// <https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching>
    fn create_navigation_params_by_fetching(
        &mut self,
        navigation_id: NavigationId,
        traversable_id: NavigableId,
        destination_url: String,
        user_involvement: UserNavigationInvolvement,
    ) -> Result<(), String> {
        // Step 1: Assert: this is running in parallel.
        // Note: The user-agent thread performs the navigation-fetch setup inline on the
        // user-agent thread; the actual network request runs in parallel in the fetch worker.
        let fetch_id = NavigationFetchId::new();
        let previous_document_id = self
            .state
            .active_documents_by_traversable
            .get(&traversable_id)
            .copied();
        // Step 2: "Let documentResource be entry's document state's resource."
        // TODO: Navigation params do not yet carry a document resource; POST navigations and
        // reload-pending are not yet supported.

        // Step 3: "Let request be a new request..."
        let request = NavigationRequest::for_destination_url(destination_url, &user_involvement);
        // `PendingNavigationFetch` keeps the request plus the source/target snapshot params
        // that the response-side continuation needs to resume the algorithm after the fetch returns.
        self.state
            .insert_pending_navigation_fetch(PendingNavigationFetch {
                fetch_id,
                navigation_id,
                traversable_id,
                previous_document_id,
                request: request.clone(),
                source_snapshot_params: SourceSnapshotParams::for_user_involvement(
                    &user_involvement,
                ),
                target_snapshot_params: TargetSnapshotParams::default(),
                navigation_timing_type: NavigationTimingType::Navigate,
                csp_navigation_type: String::from("other"),
                allow_post: false,
                user_involvement: user_involvement.clone(),
            });
        if let Err(error) = self
            .net_connection
            .start_navigation_fetch(fetch_id, request.to_navigation_fetch_request())
        {
            let _ = self
                .state
                .take_pending_navigation_fetch_by_navigation_id(navigation_id);
            self.state
                .set_navigable_ongoing_navigation(traversable_id, None);
            return Err(error);
        }

        Ok(())
    }

    /// Handle a top-level traversable that was created by the content process during
    /// `window.open`. The content process has already created the document and JS context;
    /// the user agent needs to create its own navigable state, browsing context group, agent,
    /// and event-loop registration, then notify the embedder about the new webview.
    ///
    /// This is the inverse of `create_new_top_level_traversable`: instead of the UA creating
    /// the document and sending CreateEmptyDocument to content, content creates the document
    /// and sends this event to the UA.
    /// <https://html.spec.whatwg.org/multipage/#navigate>
    /// Note: Steps 1–18 that require access to the source document or the navigable's active
    /// window (sandboxing, fragment navigation, historyHandling auto-resolution,
    /// targetSnapshotParams, and the Navigation API fire-navigate event) are executed in the
    /// content process before sending the `NavigateRequest` IPC. This function continues from
    /// step 19.
    fn navigate(
        &mut self,
        navigable_id: NavigableId,
        destination_url: String,
        user_involvement: UserNavigationInvolvement,
        navigation_id: NavigationId,
    ) -> Result<(), String> {
        let traversable_id = self.traversable_id_for_navigable(navigable_id)?;
        verification::tla_log!(
            self.navigation_tracer,
            "CreateNavigation",
            navigation_id,
            navigable_id
        );
        // Note: The inclusive-descendant navigable set needed for step 23a is pre-computed here
        // before setting the ongoing navigation so that it reflects the current tree state.
        let descendant_navigable_ids = descendant_navigable_ids(&self.state, navigable_id);
        // Step 19: "Set the ongoing navigation for navigable to navigationId."
        self.state
            .set_navigable_ongoing_navigation(traversable_id, Some(navigation_id));
        verification::tla_log!(self.navigation_tracer, "StartNavigating", navigation_id);

        // Note: The implementation always runs the beforeunload check through content,
        // even for initial about:blank documents.  This ensures the trace always contains
        // a content-side RunBeforeUnload event, making verification sensitive to whether
        // the content process's beforeunload path is functioning.

        self.check_if_unloading_is_canceled(
            navigation_id,
            navigable_id,
            destination_url,
            user_involvement,
            std::iter::once(navigable_id)
                .chain(descendant_navigable_ids.iter().copied())
                .collect(),
        )
    }

    /// <https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled>
    fn check_if_unloading_is_canceled(
        &mut self,
        navigation_id: NavigationId,
        navigable_id: NavigableId,
        destination_url: String,
        user_involvement: UserNavigationInvolvement,
        navigables_that_need_before_unload: Vec<NavigableId>,
    ) -> Result<(), String> {
        // Step 1: "Let documentsToFireBeforeunload be the active document of each item in
        // navigablesThatNeedBeforeUnload."
        let documents_to_fire_beforeunload = navigables_that_need_before_unload
            .iter()
            .filter_map(|candidate_navigable_id| {
                self.state.nav_document_id(*candidate_navigable_id)
            })
            .collect::<Vec<_>>();

        // Step 2: "Let unloadPromptShown be false."
        // Step 3: "Let finalStatus be 'continue'."
        // Note: These transient locals are replaced by a `PendingBeforeUnloadNavigation` entry
        // that accumulates per-document results asynchronously as each content event loop
        // reports its before-unload outcome.

        // Note: A document can currently be reachable through multiple candidate navigables
        // during transitional state updates. Dispatch beforeunload once per unique document id.
        let mut beforeunload_targets = HashMap::new();
        for candidate_navigable_id in &navigables_that_need_before_unload {
            let Ok(candidate_traversable_id) =
                self.traversable_id_for_navigable(*candidate_navigable_id)
            else {
                continue;
            };
            let Some(document_id) = self.state.nav_document_id(*candidate_navigable_id) else {
                continue;
            };
            beforeunload_targets
                .entry(document_id)
                .or_insert(candidate_traversable_id);
        }

        let check_id = BeforeUnloadCheckId::new();
        self.state.pending_before_unload_navigations.insert(
            check_id,
            PendingBeforeUnloadNavigation {
                check_id,
                navigation_id,
                navigable_id,
                destination_url,
                user_involvement,
                pending_document_ids: documents_to_fire_beforeunload.iter().copied().collect(),
                canceled: false,
            },
        );

        for (document_id, candidate_traversable_id) in beforeunload_targets {
            let command_sender = self.command_sender_for_traversable(candidate_traversable_id)?;
            if let Err(error) = command_sender.send(EventLoopCommand::FireAndForget {
                command: ContentCommand::RunBeforeUnload {
                    document_id,
                    check_id,
                    navigation_id,
                },
            }) {
                self.state
                    .pending_before_unload_navigations
                    .remove(&check_id);
                return Err(format!("failed to send RunBeforeUnload command: {error}"));
            }
        }

        Ok(())
    }

    /// <https://html.spec.whatwg.org/multipage/#navigate>
    /// Note: This function is the async continuation of step 23a–b of the navigate algorithm.
    /// It is invoked once all before-unload responses for the navigable's inclusive descendants
    /// have been collected, and either proceeds to step 23q (create navigation params by
    /// fetching) or abandons the navigation if it was canceled or superseded.
    fn continue_navigation_after_before_unload(
        &mut self,
        pending: PendingBeforeUnloadNavigation,
    ) -> Result<(), String> {
        let traversable_id = self.traversable_id_for_navigable(pending.navigable_id)?;
        // Step 23b: "If unloadPromptCanceled is not 'continue', or navigable's ongoing
        // navigation is no longer navigationId: ... abort these steps."
        let navigation_is_current = self
            .state
            .navigables
            .get(&traversable_id)
            .and_then(|navigable| navigable.ongoing_navigation_id)
            == Some(pending.navigation_id);
        if !navigation_is_current {
            return Ok(());
        }

        // Step 23q: "Otherwise: Let navigationParams be the result of creating navigation
        // params by fetching..."
        self.create_navigation_params_by_fetching(
            pending.navigation_id,
            traversable_id,
            pending.destination_url,
            pending.user_involvement,
        )
    }

    /// <https://html.spec.whatwg.org/multipage/#the-rules-for-choosing-a-navigable>
    ///
    /// Continuation for navigable selection when the content process could not resolve the
    /// target locally. Content resolves `_self`, `_parent`, `_top`, and some named targets
    /// before sending the request; this method handles the remaining branch: find-by-target-name
    /// for cross-process navigables and creating a new top-level traversable.
    fn choose_navigable(
        &mut self,
        _source_navigable_id: NavigableId,
        name: &str,
        noopener: bool,
    ) -> Result<(NavigableId, String), String> {
        // Step 1-2: "Let chosen be null." "Let windowType be 'existing or none'."
        // Step 3: "Let sandboxingFlagSet be ..."
        // TODO: Sandboxing checks.
        let normalized_target_name = normalize_navigation_target_name(name);

        // Step 4-6: Resolved by content. Fall through to step 7.

        // Step 7: "Otherwise, if name is not an ASCII case-insensitive match for '_blank'
        //          and noopener is false, then set chosen to the result of finding a navigable
        //          by target name given name and currentNavigable."
        if !normalized_target_name.eq_ignore_ascii_case("_blank") && !noopener {
            if let Some(chosen) =
                find_navigable_by_target_name(&self.state, &normalized_target_name)
            {
                return Ok((chosen, String::from("existing or none")));
            }
        }

        // Step 8: "If chosen is null, then a new top-level traversable is being requested."
        let new_traversable_id =
            self.create_new_top_level_traversable(normalized_target_name.clone())?;

        // Step 8 sub-step: "If noopener is true, then set windowType to 'new with no opener'.
        //                   Otherwise, set windowType to 'new and unrestricted'."
        let window_type = if noopener {
            String::from("new with no opener")
        } else {
            String::from("new and unrestricted")
        };

        // Step 9: "Return chosen and windowType."
        Ok((new_traversable_id, window_type))
    }

    fn traversable_id_for_navigable(
        &self,
        navigable_id: NavigableId,
    ) -> Result<NavigableId, String> {
        let navigable = self
            .state
            .navigables
            .get(&navigable_id)
            .ok_or_else(|| format!("unknown navigable {navigable_id}"))?;
        if navigable.event_loop_id.is_some() {
            Ok(navigable_id)
        } else {
            Err(format!(
                "navigable {navigable_id} is not a traversable navigable"
            ))
        }
    }

    /// removing an event-loop worker and every derived index owned by it.
    fn remove_event_loop_entry(&mut self, event_loop_id: EventLoopId) -> Option<EventLoopEntry> {
        let entry = self.state.event_loops.remove(&event_loop_id)?;
        let removed_traversable_ids = entry.traversable_ids.iter().copied().collect::<Vec<_>>();
        for traversable_id in &removed_traversable_ids {
            self.state.remove_traversable(*traversable_id);
            self.state
                .remove_pending_navigation_fetches_for_traversable(*traversable_id);
            let _ = self
                .state
                .remove_pending_navigation_finalizations_for_traversable(*traversable_id);
        }
        self.state
            .documents
            .retain(|_, document| !removed_traversable_ids.contains(&document.traversable_id));
        let before_unload_checks_to_remove = self
            .state
            .pending_before_unload_navigations
            .iter()
            .filter_map(|(check_id, pending)| {
                let traversable_id = self
                    .state
                    .navigables
                    .get(&pending.navigable_id)
                    .filter(|navigable| navigable.event_loop_id.is_some())
                    .map(|_| pending.navigable_id)?;
                removed_traversable_ids
                    .contains(&traversable_id)
                    .then_some(*check_id)
            })
            .collect::<Vec<_>>();
        for check_id in before_unload_checks_to_remove {
            self.state
                .pending_before_unload_navigations
                .remove(&check_id);
        }
        Some(entry)
    }

    /// stopping one owned event-loop worker by its Rust handle.
    fn stop_event_loop_handle(&mut self, event_loop_id: EventLoopId) -> Result<(), String> {
        match self.remove_event_loop_entry(event_loop_id) {
            Some(entry) => stop_event_loop_entry(entry),
            None => Ok(()),
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#create-a-fresh-top-level-traversable>
    /// Note: This helper creates the initial traversable/document shell immediately and then
    /// continues through the normal user-agent `navigate` / fetch / finalization path for the
    /// supplied startup URL.
    fn create_a_fresh_top_level_traversable(&mut self, destination_url: String) {
        if startup_debug_enabled() {
            trace!(
                "[startup-debug][user-agent] create_fresh_top_level_traversable destination_url={}",
                destination_url
            );
        }
        let result = (|| {
            // Step 1: Let traversable be the result of creating a new top-level traversable given
            // null and the empty string.
            let traversable_id = self.create_new_top_level_traversable(String::new())?;
            // Step 2: Navigate traversable to initialNavigationURL using traversable's active document.
            self.navigate(
                traversable_id,
                destination_url,
                UserNavigationInvolvement::BrowserUi,
                NavigationId::new(),
            )
        })();
        if let Err(error) = result {
            error!("failed to create a fresh top-level traversable: {error}");
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#the-rules-for-choosing-a-navigable>
    ///
    /// Resolves a navigable for a target name when the content process did not provide
    /// a chosen navigable. Handles browser-UI-originated navigations that bypass content
    /// processing, resolving `_self`, `_parent`, `_top`, and delegating to
    /// [`choose_navigable`] for named targets and new top-level traversable creation.
    fn resolve_navigable_for_target(
        &mut self,
        source_navigable_id: NavigableId,
        target: &str,
        noopener: bool,
    ) -> Result<(NavigableId, String), String> {
        let target_name = normalize_navigation_target_name(target);
        if target_name.is_empty() {
            return Ok((source_navigable_id, String::from("existing or none")));
        }

        if target_name.eq_ignore_ascii_case("_parent") {
            let navigable = self.state.navigables.get(&source_navigable_id);
            let parent = navigable
                .and_then(|n| n.parent_navigable_id)
                .unwrap_or(source_navigable_id);
            return Ok((parent, String::from("existing or none")));
        }

        if target_name.eq_ignore_ascii_case("_top") {
            let top = self
                .state
                .top_level_traversable_id(source_navigable_id)
                .unwrap_or(source_navigable_id);
            return Ok((top, String::from("existing or none")));
        }

        self.choose_navigable(source_navigable_id, target, noopener)
    }

    /// <https://html.spec.whatwg.org/#creating-a-new-top-level-traversable>
    ///
    /// Content-initiated path: the content process already created the about:blank
    /// document, Window, and JS Context.  The UA sets up its side (navigable, BCG,
    /// agent, event-loop reg, doc state, session history) without sending
    /// `CreateEmptyDocument` back to content.
    fn creating_a_new_top_level_traversable(
        &mut self,
        traversable_id: NavigableId,
        event_loop_id: EventLoopId,
        info: &NewTraversableInfo,
    ) -> Result<(), String> {
        let document_id = info.document_id;
        let target_name = &info.target_name;

        // Step 4: "Let documentState be a new document state..."
        // Step 5: "Let traversable be a new traversable navigable."
        if let Some(entry) = self.state.event_loops.get_mut(&event_loop_id) {
            entry.traversable_ids.insert(traversable_id);
        }
        self.state
            .traversable_handles
            .insert(traversable_id, event_loop_id);
        self.state
            .traversable_target_names
            .insert(traversable_id, target_name.clone());

        // Step 1 (partial): browsing context, BCG, agent cluster — UA-side state.
        let browsing_context_group_id = self.state.browsing_context_group_set.next_group_id();
        let browsing_context_id = BrowsingContextId::new();
        let agent_cluster_id = AgentClusterId::new();

        let agent = Agent {
            id: AgentId::new(),
            can_block: false,
            event_loop_id,
        };

        self.state
            .set_navigable_active_document(traversable_id, document_id);
        self.state
            .top_level_browsing_context_group_ids
            .insert(browsing_context_id, browsing_context_group_id);
        self.state.browsing_context_group_set.members.insert(
            browsing_context_group_id,
            BrowsingContextGroup {
                id: browsing_context_group_id,
                browsing_context_set: HashMap::from([(
                    browsing_context_id,
                    BrowsingContext {
                        id: browsing_context_id,
                        is_auxiliary: false,
                        opener_browsing_context: None,
                        is_popup: false,
                    },
                )]),
                agent_cluster_map: HashMap::from([(
                    AgentClusterKey::Site(String::from("about:blank")),
                    AgentCluster {
                        id: agent_cluster_id,
                        cross_origin_isolation_mode: CrossOriginIsolationMode::None,
                        is_origin_keyed: false,
                        similar_origin_window_agent: agent,
                    },
                )]),
                historical_agent_cluster_key_map: HashMap::new(),
                cross_origin_isolation_mode: CrossOriginIsolationMode::None,
            },
        );

        // Step 6: "Initialize the navigable traversable given documentState."
        self.state.navigables.insert(
            traversable_id,
            Navigable {
                id: traversable_id,
                parent_navigable_id: None,
                active_document_id: Some(document_id),
                is_active: false,
                target_name: target_name.clone(),
                active_browsing_context_id: Some(browsing_context_id),
                event_loop_id: Some(event_loop_id),
                handle: Some(event_loop_id),
                ongoing_navigation_id: None,
                has_deferred_update_the_rendering: false,
                frame_id: None,
                current_session_history_step: 0,
                session_history_entries: vec![SessionHistoryEntry {
                    step: 0,
                    document_id,
                    url: String::from("about:blank"),
                }],
            },
        );

        self.state.documents.insert(
            document_id,
            DocumentState {
                traversable_id,
                browsing_context_id: Some(browsing_context_id),
                event_loop_id,
                url: String::from("about:blank"),
                is_initial_about_blank: true,
            },
        );

        verification::tla_log!(self.navigation_tracer, "CreateNavigable", traversable_id);

        if target_name_keeps_browser_ui_focus(target_name) {
            self.state.set_active_top_level_traversable(traversable_id);
        }

        Ok(())
    }

    /// After [`choose_navigable`] creates a new top-level traversable (step 8 of
    /// <https://html.spec.whatwg.org/multipage/#the-rules-for-choosing-a-navigable>),
    /// request the embedder to create a new webview for it. This is the path where
    /// a script-initiated navigation targets `_blank` or a named target that does not
    /// exist yet. The other creation path,
    /// [`create_a_fresh_top_level_traversable`], starts after the embedder already
    /// has a webview, so this is the only place a new webview is needed.
    fn create_webview_for_new_top_level_traversable(
        &mut self,
        navigable_id: NavigableId,
        window_type: &str,
    ) -> Result<(), String> {
        let navigable = self
            .state
            .navigables
            .get(&navigable_id)
            .ok_or_else(|| format!("navigate: navigable {navigable_id} not found"))?;
        let is_new_top_level = navigable.parent_navigable_id.is_none()
            && navigable.event_loop_id.is_some()
            && window_type != "existing or none";
        if is_new_top_level {
            self.host
                .new_webview(WebviewId(navigable_id), navigable.target_name.clone())?;
            self.webview_provider_sender
                .send(WebviewProviderMessage::NewWebview {
                    webview_id: WebviewId(navigable_id),
                })
                .map_err(|error| {
                    format!("failed to enqueue webview-provider new-webview message: {error}")
                })?;
            // Register the webview with the graphics process.
            if let Some(graphics_sender) = &self.graphics_extension_sender {
                if let Err(error) =
                    graphics_sender.send(ipc_messages::graphics::GraphicsCommand::RegisterWebview {
                        webview_id: WebviewId(navigable_id),
                    })
                {
                    error!("failed to register webview with graphics process: {error}");
                }
            }
            self.host.webview_provider_sync()?;
        }
        Ok(())
    }

    /// <https://html.spec.whatwg.org/multipage/#window-open-steps>
    fn setup_opener_for_window_open(
        &mut self,
        navigable_id: NavigableId,
        window_type: &str,
        source_navigable_id: NavigableId,
        noopener: bool,
    ) -> Result<(), String> {
        let navigable = self
            .state
            .navigables
            .get(&navigable_id)
            .ok_or_else(|| format!("navigate: chosen navigable {navigable_id} not found"))?;

        let Some(browsing_context_id) = navigable.active_browsing_context_id else {
            return Ok(());
        };

        // Step 15: "If windowType is either 'new and unrestricted' or 'new with no opener':"
        if window_type == "new and unrestricted" || window_type == "new with no opener" {
            // Step 15.1: Popup detection from tokenizedFeatures.
            // TODO: Popup detection.

            // Step 15.2: Browsing context feature setup.
            // TODO: Browsing context features.

            // Step 15.3: "Set targetBrowsingContext's opener browsing context to
            //            sourceBrowsingContext."
            if window_type == "new and unrestricted" {
                let source_navigable = self.state.navigables.get(&source_navigable_id);
                if let Some(source_browsing_context_id) =
                    source_navigable.and_then(|n| n.active_browsing_context_id)
                {
                    self.state.set_opener_for_browsing_context(
                        browsing_context_id,
                        source_browsing_context_id,
                    );
                }
            }
        }

        // Step 16.2: "Set targetBrowsingContext's opener browsing context to
        //            sourceBrowsingContext."
        // Applied when reusing an existing navigable and noopener is false.
        if window_type != "new and unrestricted" && window_type != "new with no opener" && !noopener
        {
            let source_navigable = self.state.navigables.get(&source_navigable_id);
            if let Some(source_browsing_context_id) =
                source_navigable.and_then(|n| n.active_browsing_context_id)
            {
                self.state.set_opener_for_browsing_context(
                    browsing_context_id,
                    source_browsing_context_id,
                );
            }
        }

        Ok(())
    }

    /// <https://html.spec.whatwg.org/multipage/#navigate>
    fn handle_navigate(&mut self, event_loop_id: Option<EventLoopId>, request: NavigateRequest) {
        let result: Result<(), String> = (|| {
            let is_window_open = request.features_json.is_some();

            let (navigable_id, window_type) =
                // ---- Child navigable creation (iframe) ----
                if let Some(ref child_info) = request.new_child_navigable {
                    let child_navigable_id = child_info.content_navigable_id;
                    self.create_new_child_navigable(
                        child_info.parent_traversable_id,
                        child_navigable_id,
                        child_info.content_frame_id,
                        child_info.document_id,
                        child_info.target_name.clone(),
                    )?;
                    // Window type for child navigables is "existing or none" because
                    // they are not top-level and don't require opener setup.
                    (child_navigable_id, String::from("existing or none"))
                }
                // ---- Top-level traversable creation (window.open with opener) ----
                else if let Some(ref new_info) = request.new_traversable_info {
                    let traversable_id = request.chosen_navigable_id.ok_or_else(|| {
                        String::from("new_traversable_info without chosen_navigable_id")
                    })?;
                    let event_loop_id = event_loop_id.ok_or_else(|| {
                        String::from(
                            "new_traversable_info requires event_loop_id (window.open)",
                        )
                    })?;
                    self.creating_a_new_top_level_traversable(traversable_id, event_loop_id, new_info)?;
                    (traversable_id, String::from("new and unrestricted"))
                } else {
                    match request.chosen_navigable_id {
                        Some(chosen_navigable_id) => {
                            (chosen_navigable_id, String::from("existing or none"))
                        }
                        None => self.resolve_navigable_for_target(
                            request.source_navigable_id,
                            &request.target,
                            request.noopener,
                        )?,
                    }
                };

            self.create_webview_for_new_top_level_traversable(navigable_id, &window_type)?;

            if is_window_open {
                self.setup_opener_for_window_open(
                    navigable_id,
                    &window_type,
                    request.source_navigable_id,
                    request.noopener,
                )?;
            }

            let traversable_id = self.traversable_id_for_navigable(navigable_id)?;
            let navigation_id = request.navigation_id.unwrap_or_else(NavigationId::new);
            self.navigate(
                navigable_id,
                request.destination_url.clone(),
                request.user_involvement.clone(),
                navigation_id,
            )?;
            // Only notify the embedder for top-level navigations, not iframe children.
            let is_top_level = self
                .state
                .navigables
                .get(&traversable_id)
                .map(|n| n.parent_navigable_id.is_none())
                .unwrap_or(true);
            if is_top_level {
                self.host
                    .navigation_requested(WebviewId(traversable_id), request.destination_url)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            error!("failed to run navigate: {error}");
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#initialise-the-document-object>
    /// Note: Only the user-agent-owned steps of this algorithm are executed here: determining
    /// the browsing context to use (via
    /// <https://html.spec.whatwg.org/multipage/#obtain-browsing-context-navigation>) and, for
    /// cross-origin child navigables, selecting a new agent cluster and event loop (step 7).
    /// Document object construction itself runs in the content process.
    fn initialise_the_document_object(
        &mut self,
        traversable_id: NavigableId,
        final_url: &str,
    ) -> Result<Option<BrowsingContextId>, String> {
        // Step 1: "Let browsingContext be the result of obtaining a browsing context to use for
        // a navigation response given navigationParams."
        // Note: `obtain_browsing_context_to_use_for_navigation_response` implements the
        // `#obtain-browsing-context-navigation` algorithm; its `swapped_group` field captures
        // whether a browsing-context-group switch is needed for top-level traversables.
        let browsing_context_selection =
            self.obtain_browsing_context_to_use_for_navigation_response(traversable_id, final_url)?;
        let parent_traversable_id = self
            .state
            .navigables
            .get(&traversable_id)
            .and_then(|n| n.parent_navigable_id);

        let needs_new_event_loop = if let Some(parent_id) = parent_traversable_id {
            // Note: Child navigables check whether the parent document and the new document are
            // cross-origin. If they are, step 7 ("Otherwise") of the algorithm requires a new
            // agent, realized here as a new content process / event loop.

            let parent_document_url = self
                .state
                .active_documents_by_traversable
                .get(&parent_id)
                .and_then(|doc_id| self.state.documents.get(doc_id))
                .map(|doc| doc.url.clone())
                .ok_or_else(|| format!("missing parent document for traversable {parent_id}"))?;
            if parent_document_url == "about:blank" {
                return Err(format!(
                    "unexpected initial about:blank parent while initialising child traversable {traversable_id}"
                ));
            }

            // Step 7: "Otherwise:" — the active document is not initial about:blank or is not
            // same-origin-domain with the navigation origin, so a new agent is required.
            // Note: The cross-origin check here approximates the same-origin-domain condition
            // used in step 6 of the spec.
            is_cross_origin_navigation(&parent_document_url, final_url)?
        } else {
            let is_initial_about_blank = self
                .state
                .active_documents_by_traversable
                .get(&traversable_id)
                .and_then(|document_id| self.state.documents.get(document_id))
                .is_some_and(|document| {
                    document.is_initial_about_blank && document.url == "about:blank"
                });
            if is_initial_about_blank {
                return Ok(Some(browsing_context_selection.browsing_context_id));
            }
            browsing_context_selection.swapped_group
        };

        if !needs_new_event_loop {
            return Ok(Some(browsing_context_selection.browsing_context_id));
        }

        // Step 7 (continued): A new agent/event loop is required. In this architecture that means
        // spawning a new content process and reassigning the traversable to its event loop before
        // `CreateLoadedDocument` is dispatched.
        // Note: The model materializes this by creating a new agent and reassigning the
        // traversable to that new event loop before dispatching CreateLoadedDocument.
        let old_event_loop_id = self.state.traversable_handles.get(&traversable_id).copied();
        let agent = self.create_agent(false, content_process_label_from_url(final_url))?;
        let new_event_loop_id = agent.event_loop_id;
        let mut old_event_loop_to_stop = None;
        if let Some(old_event_loop_id) = old_event_loop_id {
            if let Some(old_entry) = self.state.event_loops.get_mut(&old_event_loop_id) {
                old_entry.traversable_ids.remove(&traversable_id);
            }
            if old_event_loop_id != new_event_loop_id
                && self
                    .state
                    .event_loops
                    .get(&old_event_loop_id)
                    .is_some_and(|entry| entry.traversable_ids.is_empty())
            {
                old_event_loop_to_stop = Some(old_event_loop_id);
            }
        }
        if let Some(new_entry) = self.state.event_loops.get_mut(&new_event_loop_id) {
            new_entry.traversable_ids.insert(traversable_id);
        }
        self.state
            .traversable_handles
            .insert(traversable_id, new_event_loop_id);
        if let Some(navigable) = self.state.navigables.get_mut(&traversable_id) {
            navigable.event_loop_id = Some(agent.event_loop_id);
            navigable.handle = Some(new_event_loop_id);
        }
        if let Some((snapshot, offset_x, offset_y)) = self
            .state
            .traversable_viewports
            .get(&traversable_id)
            .copied()
        {
            // Keep cross-origin child documents from booting with fallback viewport state
            // after event-loop migration.
            self.handle_set_traversable_viewport(traversable_id, snapshot, offset_x, offset_y);
        }
        if let Some(old_event_loop_id) = old_event_loop_to_stop {
            self.stop_event_loop_handle(old_event_loop_id)?;
        }
        Ok(Some(browsing_context_selection.browsing_context_id))
    }

    /// <https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled>
    fn handle_complete_before_unload_result(
        &mut self,
        result: BeforeUnloadResult,
    ) -> Result<(), String> {
        let mut completed_navigation_id = None;
        let mut waiting_for_more_results = false;
        if let Some(pending) = self
            .state
            .pending_before_unload_navigations
            .get_mut(&result.check_id)
        {
            if !pending.pending_document_ids.remove(&result.document_id) {
                return Ok(());
            }
            pending.canceled |= result.canceled;
            completed_navigation_id = Some(pending.navigation_id);
            if !pending.pending_document_ids.is_empty() {
                waiting_for_more_results = true;
            }
        }

        if waiting_for_more_results {
            return Ok(());
        }

        if completed_navigation_id.is_none() {
            return Ok(());
        }

        if let Some(pending) = self
            .state
            .pending_before_unload_navigations
            .remove(&result.check_id)
        {
            if pending.canceled {
                verification::tla_log!(
                    self.navigation_tracer,
                    "ContinueNavigation",
                    pending.navigation_id,
                    "aborted"
                );
                let traversable_id = self.traversable_id_for_navigable(pending.navigable_id)?;
                let navigation_is_current = self
                    .state
                    .navigables
                    .get(&traversable_id)
                    .and_then(|navigable| navigable.ongoing_navigation_id)
                    == Some(pending.navigation_id);
                if navigation_is_current {
                    self.state
                        .set_navigable_ongoing_navigation(traversable_id, None);
                }
                self.host.navigation_completed(NavigationCompleted {
                    webview_id: WebviewId(pending.navigable_id),
                    status: NavigationCompletion::Aborted {
                        message: String::from("navigation was canceled by beforeunload"),
                    },
                })
            } else {
                self.continue_navigation_after_before_unload(pending)
            }
        } else {
            Ok(())
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled>
    fn handle_complete_before_unload(&mut self, result: BeforeUnloadResult) {
        if let Err(error) = self.handle_complete_before_unload_result(result) {
            error!("failed to complete beforeunload: {error}");
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation>
    fn finalize_cross_document_navigation(
        &mut self,
        finalized: ContentFinalizeNavigation,
    ) -> Result<(), String> {
        // Step 1: "Assert: this is running on navigable's traversable navigable's session
        // history traversal queue."
        // Note: The user-agent thread serializes all IPC events; there is no separate
        // session-history traversal queue in this architecture.

        // Step 2: "Set navigable's is delaying load events to false."
        // Note: The content event loop owns the actual load-event delay flag. The
        // `ContentFinalizeNavigation` IPC arriving here is the commit signal that content
        // has finished loading the document and fired the `load` event.

        // Step 3: "If historyEntry's document is null, then return."
        // Note: A null pending finalization record corresponds to a null historyEntry document
        // (the navigation was canceled or the document was never successfully loaded).
        let Some(pending) = self
            .state
            .take_pending_navigation_finalization_by_document_id(finalized.document_id)
        else {
            return Ok(());
        };

        let navigation_is_current = self
            .state
            .navigables
            .get(&pending.traversable_id)
            .and_then(|navigable| navigable.ongoing_navigation_id)
            == Some(pending.navigation_id);
        // Note: Stale finalization signals are dropped when a newer navigation has already
        // replaced this continuation or the loaded document committed a different final URL.
        if pending.history_entry.url != finalized.url || !navigation_is_current {
            self.discard_provisional_browsing_context(
                pending.traversable_id,
                pending.browsing_context_id,
            );
            return Ok(());
        }

        let previous_browsing_context_id = self
            .state
            .navigables
            .get(&pending.traversable_id)
            .and_then(|navigable| navigable.active_browsing_context_id);

        // Step 4: "If all of the following are true: navigable's parent is null; historyEntry's
        // document's browsing context is not an auxiliary browsing context whose opener browsing
        // context is non-null; and historyEntry's document's origin is not navigable's active
        // document's origin, then set historyEntry's document state's navigable target name to
        // the empty string."
        // TODO: `SessionHistoryEntry` does not yet carry a per-entry navigable target name
        // field; this branch is not executed.

        self.state.set_navigable_active_browsing_context(
            pending.traversable_id,
            pending.browsing_context_id,
        );
        self.state
            .set_navigable_active_document(pending.traversable_id, finalized.document_id);

        // Step 5: "Let entryToReplace be navigable's active session history entry if
        // historyHandling is 'replace', otherwise null."
        // Note: `commit_session_history_entry` derives the replace-versus-push behavior
        // internally from `history_handling` rather than storing a separate `entryToReplace`.

        // Step 6: "Let traversable be navigable's traversable navigable."
        // Note: `pending.traversable_id` is the traversable navigable's identifier.

        // Step 7: "Let targetStep be null."

        // Step 8: "Let targetEntries be the result of getting session history entries for
        // navigable."

        // Step 9: "If entryToReplace is null: [push case]. Otherwise: [replace case]."
        // Note: `commit_session_history_entry` computes the push/replace step and mutates
        // the target entries list accordingly.

        // Step 10: "Apply the push/replace history step targetStep to traversable given
        // historyHandling and userInvolvement."
        self.state.commit_session_history_entry(
            pending.traversable_id,
            pending.history_entry.clone(),
            pending.history_handling,
        );
        verification::tla_log!(
            self.navigation_tracer,
            "ContinueNavigation",
            pending.navigation_id,
            "finalized"
        );
        self.state
            .set_navigable_ongoing_navigation(pending.traversable_id, None);
        if let Some(document) = self.state.documents.get_mut(&finalized.document_id) {
            document.url = finalized.url.clone();
            document.is_initial_about_blank = finalized.url == "about:blank";
        }
        self.handle_rendering_opportunity_for(pending.traversable_id);
        let notify_result = self.host.navigation_completed(NavigationCompleted {
            webview_id: WebviewId(pending.traversable_id),
            status: NavigationCompletion::Committed {
                url: finalized.url.clone(),
            },
        });

        if let Some(previous_document_id) = pending.previous_document_id {
            if previous_document_id != finalized.document_id {
                // The old document is destroyed after the new document commits so stale
                // content-side traffic cannot revive it after the traversable has advanced.
                if let Ok(command_sender) =
                    self.command_sender_for_traversable(pending.traversable_id)
                {
                    if let Err(error) = self.send_event_loop_command(
                        &command_sender,
                        ContentCommand::DestroyDocument {
                            document_id: previous_document_id,
                        },
                    ) {
                        error!("[user-agent] failed to destroy previous document: {error}");
                    }
                }
                self.state.documents.remove(&previous_document_id);
            }
        }

        if let Some(new_browsing_context_id) = pending.browsing_context_id {
            let is_top_level = self
                .state
                .navigables
                .get(&pending.traversable_id)
                .is_some_and(|navigable| navigable.parent_navigable_id.is_none());
            if is_top_level {
                if let Some(previous_browsing_context_id) = previous_browsing_context_id
                    && previous_browsing_context_id != new_browsing_context_id
                {
                    self.state
                        .top_level_browsing_context_group_ids
                        .remove(&previous_browsing_context_id);
                    self.state
                        .browsing_context_group_set
                        .remove_browsing_context(previous_browsing_context_id);
                }
            }
        }

        notify_result
    }

    /// <https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation>
    fn handle_finalize_cross_document_navigation(&mut self, finalized: ContentFinalizeNavigation) {
        if let Err(error) = self.finalize_cross_document_navigation(finalized) {
            error!("failed to finalize cross-document navigation: {error}");
        }
    }

    /// the automation-only script-evaluation bridge into the owning event loop.
    fn handle_evaluate_script(
        &mut self,
        traversable_id: NavigableId,
        source: String,
        _timeout: Duration,
        reply: Sender<Result<serde_json::Value, String>>,
    ) {
        let error_reply = reply.clone();
        let send_result = match self.state.traversable_handles.get(&traversable_id).copied() {
            Some(event_loop_id) => match self.state.event_loops.get(&event_loop_id) {
                Some(entry) => {
                    let request_id = self.next_automation_request_id;
                    self.next_automation_request_id =
                        self.next_automation_request_id.wrapping_add(1);
                    entry.command_sender
                            .send(EventLoopCommand::EvaluateScript {
                                traversable_id,
                                request_id,
                                source,
                                reply,
                            })
                            .map_err(|error| {
                                format!(
                                    "failed to send script evaluation to event loop {event_loop_id}: {error}"
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

        if let Err(error) = send_result {
            let _ = error_reply.send(Err(error));
        }
    }

    /// the automation-only selector-click bridge into the owning event loop.
    fn handle_click_element(
        &mut self,
        traversable_id: NavigableId,
        selector: String,
        reply: Sender<Result<(), String>>,
    ) {
        let error_reply = reply.clone();
        let send_result = match self.state.traversable_handles.get(&traversable_id).copied() {
            Some(event_loop_id) => match self.state.event_loops.get(&event_loop_id) {
                Some(entry) => {
                    let request_id = self.next_automation_request_id;
                    self.next_automation_request_id =
                        self.next_automation_request_id.wrapping_add(1);
                    entry.command_sender
                        .send(EventLoopCommand::ClickElement {
                            traversable_id,
                            request_id,
                            selector,
                            reply,
                        })
                        .map_err(|error| {
                            format!(
                                "failed to send selector click to event loop {event_loop_id}: {error}"
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

        if let Err(error) = send_result {
            let _ = error_reply.send(Err(error));
        }
    }

    /// applying the default viewport to the active traversable and its descendants.
    fn handle_set_default_viewport(&mut self, snapshot: (u32, u32, f32, ColorScheme)) {
        // This follows the embedder's active top-level selection only; inactive top-level
        // traversables keep their last published viewport until they become active again.
        //
        // Child traversables are updated from compositor-derived iframe geometry via
        // SetTraversableViewport. Reapplying the default viewport to descendants here can
        // transiently reset iframe offsets to (0,0), which leaves child hit testing and
        // scale wrong until a later parent composition pass republishes child viewports.
        let active_top_level_traversable_id =
            self.state
                .navigables
                .iter()
                .find_map(|(navigable_id, navigable)| {
                    (navigable.parent_navigable_id.is_none() && navigable.is_active)
                        .then_some(*navigable_id)
                });
        let Some(traversable_id) = active_top_level_traversable_id else {
            return;
        };

        self.handle_set_traversable_viewport(traversable_id, snapshot, 0.0, 0.0);
    }

    /// sending a per-traversable viewport update to the owning event loop.
    fn handle_set_traversable_viewport(
        &mut self,
        traversable_id: NavigableId,
        snapshot: (u32, u32, f32, ColorScheme),
        offset_x: f32,
        offset_y: f32,
    ) {
        self.state
            .traversable_viewports
            .insert(traversable_id, (snapshot, offset_x, offset_y));

        let Some(handle) = self.state.traversable_handles.get(&traversable_id).copied() else {
            return;
        };
        let Some(entry) = self.state.event_loops.get(&handle) else {
            return;
        };
        let command = traversable_viewport_command(traversable_id, snapshot, offset_x, offset_y);
        let _ = entry
            .command_sender
            .send(EventLoopCommand::FireAndForget { command });
    }

    /// queuing DOM event dispatch on the traversable's owning
    /// <https://html.spec.whatwg.org/multipage/#event-loop>.
    fn handle_dispatch_event_for(&mut self, traversable_id: NavigableId, event: String) {
        let Some(handle) = self.state.traversable_handles.get(&traversable_id).copied() else {
            return;
        };
        let Some(document_id) = self
            .state
            .active_documents_by_traversable
            .get(&traversable_id)
        else {
            return;
        };
        let Some(entry) = self.state.event_loops.get(&handle) else {
            return;
        };

        if input_debug_enabled() {
            trace!(
                "[input-debug][user-agent] dispatch_event traversable={} event_loop={} document={} bytes={}",
                traversable_id,
                handle,
                document_id,
                event.len(),
            );
        }

        let command = ContentCommand::DispatchEvent {
            events: vec![DispatchEventEntry {
                document_id: *document_id,
                event,
                prefetched_clipboard_text: None,
            }],
        };
        let _ = entry
            .command_sender
            .send(EventLoopCommand::FireAndForget { command });
    }

    /// <https://html.spec.whatwg.org/multipage/#update-the-rendering>
    fn handle_rendering_opportunity_for(&mut self, traversable_id: NavigableId) {
        let Some(handle) = self.state.traversable_handles.get(&traversable_id).copied() else {
            return;
        };
        let Some(document_id) = self
            .state
            .active_documents_by_traversable
            .get(&traversable_id)
        else {
            return;
        };
        let Some(entry) = self.state.event_loops.get(&handle) else {
            return;
        };

        if input_debug_enabled() {
            trace!(
                "[input-debug][user-agent] rendering_opportunity traversable={} event_loop={} document={}",
                traversable_id, handle, document_id,
            );
        }

        log_render_state_debug(format!(
            "send rendering opportunity traversable={} document={} event_loop={}",
            traversable_id, document_id, handle,
        ));
        let command = ContentCommand::UpdateTheRendering {
            traversable_id,
            document_id: *document_id,
        };
        let _ = entry
            .command_sender
            .send(EventLoopCommand::FireAndForget { command });
    }

    /// resuming an event-loop-local document fetch after the fetch worker succeeds.
    /// <https://html.spec.whatwg.org/multipage/#attempt-to-populate-the-history-entry's-document>
    fn handle_navigation_fetch_completed(
        &mut self,
        fetch_id: NavigationFetchId,
        response: ContentFetchResponse,
    ) {
        let Some(pending) = self
            .state
            .take_pending_navigation_fetch_by_fetch_id(fetch_id)
        else {
            return;
        };
        let navigation_is_current = self
            .state
            .navigables
            .get(&pending.traversable_id)
            .and_then(|navigable| navigable.ongoing_navigation_id)
            == Some(pending.navigation_id);
        if !navigation_is_current {
            return;
        }
        // Step 5.1: "If navigable's ongoing navigation no longer equals navigationId, then run
        // completionSteps and abort these steps."
        // Note: The navigation-is-current check above covers this guard.

        // Step 5.6: "Otherwise, load the document..."
        // Note: For a successful HTML response the load path goes through
        // <https://html.spec.whatwg.org/multipage/#navigate-html> and then
        // <https://html.spec.whatwg.org/multipage/#initialise-the-document-object>.
        // In this architecture the user-agent selects the browsing context and event loop
        // placement first, then delegates document construction to the content process.
        let final_url = response.final_url.clone();

        // Note: `initialise_the_document_object` selects the browsing context and event loop for
        // the new document, creating a new process for cross-origin child navigables and
        // swap-group top-level navigations.
        let browsing_context_id =
            match self.initialise_the_document_object(pending.traversable_id, &final_url) {
                Ok(browsing_context_id) => browsing_context_id,
                Err(error) => {
                    self.state
                        .set_navigable_ongoing_navigation(pending.traversable_id, None);
                    if let Err(error) = self.host.navigation_completed(NavigationCompleted {
                        webview_id: WebviewId(pending.traversable_id),
                        status: NavigationCompletion::Aborted { message: error },
                    }) {
                        error!(
                            "[user-agent] failed to report navigation completed (init doc): {error}"
                        );
                    }
                    return;
                }
            };

        // Note: After `initialise_the_document_object` the traversable may have been moved to a
        // new event loop; re-fetch the command sender so the `CreateLoadedDocument` command is
        // delivered to the correct content process.
        let command_sender = match self.command_sender_for_traversable(pending.traversable_id) {
            Ok(command_sender) => command_sender,
            Err(error) => {
                self.state
                    .set_navigable_ongoing_navigation(pending.traversable_id, None);
                if let Err(error) = self.host.navigation_completed(NavigationCompleted {
                    webview_id: WebviewId(pending.traversable_id),
                    status: NavigationCompletion::Aborted { message: error },
                }) {
                    error!(
                        "[user-agent] failed to report navigation completed (command sender): {error}"
                    );
                }
                return;
            }
        };
        let document_id = DocumentId::new();
        // Note: For child navigables the compositor frame_id is forwarded so the content process
        // can identify which iframe slot this document renders into.
        let frame_id = self
            .state
            .navigables
            .get(&pending.traversable_id)
            .and_then(|n| n.frame_id);
        let loaded_response = LoadedDocumentResponse {
            final_url: final_url.clone(),
            status: response.status,
            content_type: response.content_type.clone(),
            body: String::from_utf8_lossy(&response.body).into_owned(),
        };
        let (traversable_event_loop_id, parent_traversable_id) = self
            .state
            .navigables
            .get(&pending.traversable_id)
            .map(|n| {
                (
                    n.event_loop_id.unwrap_or_else(EventLoopId::new),
                    n.parent_navigable_id,
                )
            })
            .unwrap_or((EventLoopId::new(), None));
        let top_level_traversable_id = self
            .state
            .top_level_traversable_id(pending.traversable_id)
            .unwrap_or(pending.traversable_id);
        let document_state = DocumentState {
            traversable_id: pending.traversable_id,
            browsing_context_id,
            event_loop_id: traversable_event_loop_id,
            url: final_url.clone(),
            is_initial_about_blank: false,
        };
        self.state
            .documents
            .insert(document_id, document_state.clone());
        match self.send_event_loop_command(
            &command_sender,
            ContentCommand::CreateLoadedDocument {
                traversable_id: pending.traversable_id,
                document_id,
                frame_id,
                response: loaded_response,
                parent_traversable_id,
                top_level_traversable_id,
            },
        ) {
            Ok(_) => {
                self.state
                    .insert_pending_navigation_finalization(PendingNavigationFinalization {
                        document_id,
                        navigation_id: pending.navigation_id,
                        traversable_id: pending.traversable_id,
                        previous_document_id: pending.previous_document_id,
                        browsing_context_id,
                        history_entry: SessionHistoryEntry {
                            step: 0,
                            document_id,
                            url: final_url,
                        },
                        history_handling: HistoryHandlingBehavior::Push,
                        user_involvement: pending.user_involvement,
                    });
            }
            Err(error) => {
                self.state.documents.remove(&document_id);
                self.discard_provisional_browsing_context(
                    pending.traversable_id,
                    browsing_context_id,
                );
                self.state
                    .set_navigable_ongoing_navigation(pending.traversable_id, None);
                if let Err(error) = self.host.navigation_completed(NavigationCompleted {
                    webview_id: WebviewId(pending.traversable_id),
                    status: NavigationCompletion::Aborted { message: error },
                }) {
                    error!("[user-agent] failed to report navigation completed (send): {error}");
                }
            }
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#attempt-to-populate-the-history-entry's-document>
    fn handle_navigation_fetch_failed(&mut self, fetch_id: NavigationFetchId) {
        let Some(pending) = self
            .state
            .take_pending_navigation_fetch_by_fetch_id(fetch_id)
        else {
            return;
        };
        self.state
            .set_navigable_ongoing_navigation(pending.traversable_id, None);
        if let Err(error) = self.host.navigation_completed(NavigationCompleted {
            webview_id: WebviewId(pending.traversable_id),
            status: NavigationCompletion::Aborted {
                message: format!("navigation fetch failed for {}", pending.request.url),
            },
        }) {
            error!("[user-agent] failed to report navigation completed (fetch failed): {error}");
        }
    }

    /// the document-fetch watchdog fired by the timer worker.
    /// <https://html.spec.whatwg.org/multipage/#timers>
    fn handle_window_timer_task(
        &mut self,
        event_loop_id: EventLoopId,
        document_id: DocumentId,
        timer_id: u32,
        timer_key: WindowTimerKey,
        nesting_level: u32,
    ) {
        let Some(entry) = self.state.event_loops.get(&event_loop_id) else {
            return;
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

    /// removing a child-navigable mapping and stopping any synthetic
    /// traversable that represented that iframe in the user-agent registry.
    fn handle_iframe_traversable_removed(
        &mut self,
        parent_traversable_id: NavigableId,
        content_navigable_id: NavigableId,
        content_frame_id: FrameId,
        reply: Sender<Result<(), String>>,
    ) {
        let target_name = iframe_target_name(
            parent_traversable_id,
            content_navigable_id,
            content_frame_id,
        );
        let traversable_ids = self
            .state
            .traversable_target_names
            .iter()
            .filter_map(|(traversable_id, traversable_target_name)| {
                (traversable_target_name == &target_name).then_some(*traversable_id)
            })
            .collect::<Vec<_>>();

        let mut event_loops_to_maybe_stop = HashSet::new();
        let mut removed_document_ids = HashSet::new();

        for traversable_id in &traversable_ids {
            if let Some(event_loop_id) = self.state.traversable_handles.get(traversable_id).copied()
            {
                event_loops_to_maybe_stop.insert(event_loop_id);
                if let Some(entry) = self.state.event_loops.get_mut(&event_loop_id) {
                    entry.traversable_ids.remove(traversable_id);
                }
            }

            self.state
                .remove_pending_navigation_fetches_for_traversable(*traversable_id);
            let _ = self
                .state
                .remove_pending_navigation_finalizations_for_traversable(*traversable_id);
            if let Some(document_id) = self
                .state
                .active_documents_by_traversable
                .get(traversable_id)
                .copied()
            {
                removed_document_ids.insert(document_id);
            }
            self.state.remove_traversable(*traversable_id);
        }

        if !removed_document_ids.is_empty() {
            self.state.documents.retain(|document_id, document| {
                !removed_document_ids.contains(document_id)
                    && !traversable_ids.contains(&document.traversable_id)
            });
            let checks_to_remove = self
                .state
                .pending_before_unload_navigations
                .iter_mut()
                .filter_map(|(check_id, pending)| {
                    pending
                        .pending_document_ids
                        .retain(|document_id| !removed_document_ids.contains(document_id));
                    (pending.pending_document_ids.is_empty()
                        || traversable_ids.contains(&pending.navigable_id))
                    .then_some(*check_id)
                })
                .collect::<Vec<_>>();
            for check_id in checks_to_remove {
                self.state
                    .pending_before_unload_navigations
                    .remove(&check_id);
            }
        }

        let mut result = Ok(());
        for event_loop_id in event_loops_to_maybe_stop {
            let should_stop = self
                .state
                .event_loops
                .get(&event_loop_id)
                .is_some_and(|entry| entry.traversable_ids.is_empty());
            if !should_stop {
                continue;
            }
            if let Err(error) = self.stop_event_loop_handle(event_loop_id) {
                result = Err(error);
                break;
            }
        }

        let _ = reply.send(result);
    }

    /// shutting down the user-agent thread and every worker it owns.
    fn handle_shutdown(&mut self, reply: Sender<Result<(), String>>) {
        let entries = self
            .state
            .event_loops
            .drain()
            .map(|(_, entry)| entry)
            .collect::<Vec<_>>();
        self.state.browsing_context_group_set.members.clear();
        self.state.navigables.clear();
        self.state.top_level_browsing_context_group_ids.clear();
        self.state.traversable_handles.clear();
        self.state.traversable_target_names.clear();
        self.state.active_documents_by_traversable.clear();
        self.state.documents.clear();
        self.state.pending_before_unload_navigations.clear();
        self.state.pending_navigation_fetches.clear();
        self.state.pending_navigation_fetch_ids_by_fetch_id.clear();
        self.state.pending_navigation_finalizations.clear();
        self.state
            .pending_navigation_finalization_ids_by_navigation_id
            .clear();

        let mut shutdown_result = Ok(());
        for entry in entries {
            if let Err(error) = stop_event_loop_entry(entry) {
                shutdown_result = Err(error);
                break;
            }
        }

        self.net_connection.shutdown();

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

        // Shut down the media extension directly.
        if let Some(media_sender) = &self.media_extension_sender {
            if let Err(error) = media_sender.send(ipc_messages::media::MediaCommand::Shutdown) {
                shutdown_result = Err(format!("failed to request media shutdown: {error}"));
            }

            if let Some(mut media_child) = self.media_child.take() {
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(150);
                loop {
                    match media_child.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) => {
                            if std::time::Instant::now() >= deadline {
                                let _ = media_child.kill();
                                let _ = media_child.wait();
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(error) => {
                            log::error!("failed to poll media process exit: {error}");
                            let _ = media_child.kill();
                            let _ = media_child.wait();
                            break;
                        }
                    }
                }
            }
        }

        let _ = reply.send(shutdown_result);
    }

    /// Register the pipeline→webview mapping for video frame routing.
    fn register_media_pipeline(
        &mut self,
        pipeline_id: MediaPipelineId,
        traversable_id: NavigableId,
        video_paint_id: VideoPaintId,
    ) {
        debug!(
            "[media] registering pipeline mapping: pipeline={:?} traversable={} paint={:?}",
            pipeline_id, traversable_id.0, video_paint_id,
        );
        let webview_id = WebviewId(traversable_id);
        self.pipeline_to_webview
            .insert(pipeline_id, (webview_id, video_paint_id));
    }

    /// Handle a GraphicsEvent (composed scene) from the graphics process.
    fn handle_graphics_event(
        &mut self,
        incoming: &mut ipc::IpcIncoming<ipc_messages::graphics::GraphicsEvent>,
    ) {
        use ipc_messages::graphics::GraphicsEvent;
        match &incoming.payload {
            GraphicsEvent::ComposedSceneReady {
                webview_id,
                scene_shmem_key,
                font_registrations,
                frame_hit_info,
            } => {
                debug!(
                    "[graphics] received composed scene for webview {:?} key={} fonts={} hit_info={}",
                    webview_id,
                    scene_shmem_key,
                    font_registrations.len(),
                    frame_hit_info.len(),
                );

                // Extract scene bytes from shared memory.
                let scene_bytes = incoming
                    .shmem_regions
                    .get(scene_shmem_key)
                    .map(|region| region.as_slice().to_vec())
                    .unwrap_or_default();

                // Extract font data from shared memory.
                let mut font_data: std::collections::HashMap<usize, Vec<u8>> =
                    std::collections::HashMap::new();
                for font in font_registrations {
                    if let Some(region) = incoming.shmem_regions.get(&font.data_shmem_key) {
                        font_data.insert(font.data_shmem_key, region.as_slice().to_vec());
                    }
                }

                // Store hit-testing info for ui event routing.
                self.state
                    .frame_hit_info
                    .insert(*webview_id, frame_hit_info.clone());

                // Forward to the embedder host with font data.
                if let Err(error) = self.host.new_web_content_scene(
                    *webview_id,
                    scene_bytes,
                    font_registrations.clone(),
                    font_data,
                    frame_hit_info.clone(),
                ) {
                    error!("[graphics] failed to forward composed scene: {error}");
                }

                // Note: Do NOT trigger a rendering opportunity here — that would
                // create a feedback loop (compose → render → PaintFrame → compose → ...),
                // flooding the graphics process with hundreds of PaintFrames per second.
                // The content process drives its own rendering via the HTML event loop;
                // rendering opportunities come from user input and viewport changes only.
            }
            GraphicsEvent::ShutdownComplete => {
                debug!("[graphics] graphics process shutdown complete");
            }
        }
    }

    /// Handle a MediaEvent from the media process.
    fn handle_media_event(&mut self, event: ipc_messages::media::MediaEvent) {
        use ipc_messages::media::MediaEvent;
        match event {
            MediaEvent::Frame(video_frame) => {
                let mut video_frame = video_frame;
                let pipeline_id = video_frame.pipeline_id;
                let Some(&(webview_id, paint_id)) = self.pipeline_to_webview.get(&pipeline_id)
                else {
                    debug!(
                        "[media] received frame for unknown pipeline {:?}",
                        pipeline_id
                    );
                    return;
                };
                debug!(
                    "[media] received video frame: {}x{} pipeline={:?}",
                    video_frame.width, video_frame.height, pipeline_id,
                );

                // Media now runs inside the graphics process — video frames from the
                // media backend go directly to the compositor within the graphics process.
                // Keep forwarding to the webview provider for the local composition path.
                debug!(
                    "[media] forwarding frame to compositor: {}x{} paint={:?}",
                    video_frame.width, video_frame.height, paint_id
                );
                if let Err(error) =
                    self.webview_provider_sender
                        .send(WebviewProviderMessage::VideoFrameReady {
                            webview_id,
                            paint_id,
                            data: video_frame,
                        })
                {
                    error!("[media] failed to enqueue video frame: {error}");
                } else {
                    debug!(
                        "[media] frame enqueued, requesting redraw+render for webview {:?}",
                        webview_id
                    );
                    let _ = self.host.request_redraw(webview_id);
                    let _ = self
                        .command_sender
                        .send(UserAgentCommand::RenderingOpportunityFor {
                            traversable_id: webview_id.0,
                        });
                    let _ = self.host.webview_provider_sync();
                }
            }
            MediaEvent::Eos { pipeline_id } => {
                debug!("[media] pipeline {:?} reached end of stream", pipeline_id);
            }
            MediaEvent::Error {
                pipeline_id,
                message,
            } => {
                error!("[media] pipeline {:?} error: {}", pipeline_id, message);
            }
            MediaEvent::DurationChanged {
                pipeline_id,
                duration_secs,
            } => {
                debug!(
                    "[media] pipeline {:?} duration: {}s",
                    pipeline_id, duration_secs
                );
            }
        }
    }
}
