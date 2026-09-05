mod agent;
mod channel_messaging;
mod event_loops;
mod fetch;
pub(crate) mod ipc_manifest;
pub(crate) mod ui_event;

use blitz_traits::shell::ColorScheme;
use channel_messaging::PortEvent;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use ipc_messages::content::{
    AgentClusterId, AgentId, BeforeUnloadCheckId, BeforeUnloadResult, BrowsingContextGroupId,
    BrowsingContextId, Command as ContentCommand, DispatchEventEntry, DocumentId,
    Event as ContentEvent, EventLoopId, FetchResponse as ContentFetchResponse,
    FinalizeNavigation as ContentFinalizeNavigation, FrameId, LoadedDocumentResponse, NavigableId,
    NavigateRequest, NavigationFetchId, NavigationId, NewTraversableInfo,
    UserNavigationInvolvement, WebviewId, iframe_target_name,
};
use ipc_messages::safe_passing_of_structured_data::PostMessageRequest;
use log::{debug, error, info, trace};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;
use verification::{TLATracer, TraceSender};

fn startup_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_STARTUP").is_some()
}

use crate::agent::{
    Agent, AgentCluster, AgentClusterKey, CrossOriginIsolationMode, DedicatedWorkerAgent,
    SimilarOriginWindowAgent,
};
use crate::event_loops::{WorkerEventLoop, spawn_window_event_loop, traversable_viewport_command};

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
    fn request_redraw(&self, webview_id: WebviewId);
    fn viewport_scale_factor(&self) -> f32;
    fn window_viewport_snapshot(&self) -> Option<(u32, u32, f32, ColorScheme)>;
    fn clipboard_get_text(&self) -> Result<String, String>;
    fn clipboard_set_text(&self, text: String) -> Result<(), String>;
    /// The parsed title of a top-level document, reported by the content
    /// process after parsing; the embedder labels the tab and window with it.
    /// <https://html.spec.whatwg.org/#the-title-element>
    fn title_changed(&self, webview_id: WebviewId, title: String) -> Result<(), String>;
    /// Forward a composed web content scene from the graphics process to the
    /// embedder for rendering.
    fn new_web_content_scene(
        &self,
        webview_id: WebviewId,
        scene_bytes: Vec<u8>,
        font_registrations: Vec<ipc_messages::content::RegisteredFont>,
        font_data: std::collections::HashMap<usize, Vec<u8>>,
    ) -> Result<(), String>;
    /// Forward the per-layer rendered frame from the graphics process. Each
    /// layer carries its wire `topology` (transform, clip, z-order) plus the
    /// actual `frame` only when the layer was re-rendered this cycle; a clean
    /// layer keeps its last surface and carries `frame: None`. `animating`
    /// reports whether the composed scene contains animated content (video,
    /// CSS animations) that needs the next frame at display cadence.
    fn new_web_content_layers(
        &self,
        webview_id: WebviewId,
        layers: Vec<ipc_messages::graphics::LayerFrame>,
        animating: bool,
    ) -> Result<(), String>;
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

/// One traversable's last-published viewport — the viewport snapshot
/// (`width`, `height`, `scale`, color scheme) and scroll offset — replayed
/// when the traversable moves to a new content event loop.
pub type TraversableViewportSnapshot = ((u32, u32, f32, ColorScheme), f32, f32);

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
    /// <https://html.spec.whatwg.org/multipage/webappapis.html#similar-origin-window-agent>
    /// <https://html.spec.whatwg.org/multipage/webappapis.html#dedicated-worker-agent>
    /// The agents the user agent has obtained — the similar-origin window
    /// agents it creates as the single window agent of each agent cluster
    /// (one content process, see the browsing context group's agent cluster
    /// map), and the dedicated worker agents the clusters' content
    /// processes report as obtained
    /// (`ContentEvent::DedicatedWorkerAgentObtained`) — keyed by the event
    /// loop each agent owns: its window event loop for a window agent, its
    /// own worker event loop for a worker agent.  The event loop id is how
    /// an agent is addressed (every event loop belongs to exactly one
    /// agent, and content identifies realms by the loop they run on), so
    /// the registry lookup from an event loop id is direct.  A worker
    /// agent is created and destroyed while the window agent of its
    /// cluster lives on; its record is registered flat, holding the
    /// signifier of its hosting window agent so it is dropped when the
    /// hosting content process exits.
    pub agents: HashMap<EventLoopId, Agent>,
    /// reverse index from top-level traversable ids to the owning event-loop id.
    pub traversable_handles: HashMap<NavigableId, EventLoopId>,
    /// last published viewport per traversable; replayed when ownership moves to a new
    /// content event loop (for example cross-origin child navigations).
    pub traversable_viewports: HashMap<NavigableId, TraversableViewportSnapshot>,
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
    /// Mapping from content frame_id to child webview_id for each root
    /// webview. Published by the graphics process alongside each composed
    /// scene. Used by route_ui_event to route pointer events to child
    /// traversables (iframes).
    pub child_frame_to_webview: HashMap<WebviewId, HashMap<FrameId, WebviewId>>,
    /// The last frame that received a pointer-down event, used to route
    /// non-positional events (keyboard, IME) to the correct frame even when
    /// no pointer is active.
    pub focused_frame_id: HashMap<WebviewId, Option<FrameId>>,
    /// Cached child viewport publications to avoid re-publishing unchanged
    /// viewports. Each entry is (width, height, offset_x, offset_y).
    /// Without this cache, every ComposedSceneReady triggers
    /// set_traversable_viewport → note_rendering_opportunity → UpdateTheRendering →
    /// PaintFrame → Compose → ComposedSceneReady → ... creating a render cascade.
    published_child_viewports: HashMap<WebviewId, (u32, u32, f32, f32)>,
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
            agents: HashMap::new(),
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
            child_frame_to_webview: HashMap::new(),
            focused_frame_id: HashMap::new(),
            published_child_viewports: HashMap::new(),
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

/// Commands that enter the user-agent thread from the embedder and webview
/// layers: each is dispatched directly on the user-agent thread, which owns
/// all browser-global state.
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
    /// The embedder is about to paint a frame; the UA queues update the
    /// rendering if a rendering opportunity was noted.
    FrameNeeded {
        webview_id: WebviewId,
    },
    DispatchEventFor {
        traversable_id: NavigableId,
        event: Vec<u8>,
    },
    RenderingOpportunityFor {
        navigable_id: NavigableId,
    },
    SendUiEvent {
        webview_id: WebviewId,
        event_message: Vec<u8>,
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
        trace_sender: Option<TraceSender>,
    ) -> Result<Self, String> {
        let (command_sender, command_receiver) = unbounded();
        let mut worker = UserAgentWorker::new(command_receiver, host, trace_sender);
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
        reply_receiver.recv_timeout(timeout).map_err(|error| {
            format!(
                "timed out after {} ms waiting for script evaluation reply: {error}",
                timeout.as_millis()
            )
        })?
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
        event: Vec<u8>,
    ) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::DispatchEventFor {
                traversable_id,
                event,
            })
            .map_err(|error| format!("failed to send dispatch-event request: {error}"))
    }

    /// Send a UI event to the user agent for hit-testing and dispatch to content.
    pub fn send_ui_event(
        &self,
        webview_id: WebviewId,
        event_message: Vec<u8>,
    ) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::SendUiEvent {
                webview_id,
                event_message,
            })
            .map_err(|error| format!("failed to send ui event: {error}"))
    }

    /// <https://html.spec.whatwg.org/multipage/#update-the-rendering>
    pub fn note_rendering_opportunity(&self, navigable_id: NavigableId) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::RenderingOpportunityFor { navigable_id })
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

    /// The embedder is about to paint a frame; the UA queues update the
    /// rendering if a rendering opportunity was noted.
    pub fn frame_needed(&self, webview_id: WebviewId) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::FrameNeeded { webview_id })
            .map_err(|error| format!("failed to forward frame needed: {error}"))
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

/// The similar-origin window agent owning the window event loop
/// `event_loop_id`, when the registry holds one: `None` for an unknown
/// event loop and for the worker event loop of a dedicated worker agent.
fn window_agent(
    state: &UserAgentState,
    event_loop_id: EventLoopId,
) -> Option<&SimilarOriginWindowAgent> {
    match state.agents.get(&event_loop_id)? {
        Agent::Window(window_agent) => Some(window_agent),
        Agent::DedicatedWorker(_) => None,
    }
}

/// Mutating counterpart of [`window_agent`].
fn window_agent_mut(
    state: &mut UserAgentState,
    event_loop_id: EventLoopId,
) -> Option<&mut SimilarOriginWindowAgent> {
    match state.agents.get_mut(&event_loop_id)? {
        Agent::Window(window_agent) => Some(window_agent),
        Agent::DedicatedWorker(_) => None,
    }
}

/// Resolve the content command channel of the agent owning `event_loop_id`:
/// the similar-origin window agent's channel (its content process's command
/// channel) for a window event loop, or the dedicated worker agent's own
/// channel (see `DedicatedWorkerAgent::event_loop`) for a worker event
/// loop, so the task is delivered straight to the worker.  Returns
/// `None` for an event loop the user agent no longer owns (a removed
/// window agent or a closed worker agent).
fn event_loop_command_sender(
    state: &UserAgentState,
    event_loop_id: EventLoopId,
) -> Option<ipc::IpcSender<ContentCommand>> {
    // Every agent owns exactly one event loop; its loop id is the registry
    // key, so the owning agent is a direct lookup.
    match state.agents.get(&event_loop_id)? {
        Agent::Window(window_agent) => Some(window_agent.event_loop.command_sender.clone()),
        Agent::DedicatedWorker(worker_agent) => {
            Some(worker_agent.event_loop.command_sender.clone())
        }
    }
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

/// An inbound event on one of the user-agent worker's channels.
enum Inbound {
    Command(UserAgentCommand),
    Net(ipc_messages::network::Response),
    Graphics(ipc::IpcIncoming<ipc_messages::graphics::GraphicsEvent>),
    Content(EventLoopId, ipc::IpcIncoming<ContentEvent>),
    ContentDisconnected(EventLoopId),
}

/// user-agent thread coordinates.
struct UserAgentWorker {
    state: UserAgentState,
    command_receiver: Receiver<UserAgentCommand>,
    /// Owns the IPC connection to the net extension and tracks pending navigation fetches.
    net_connection: crate::fetch::NetConnection,

    /// IPC sender to the graphics process.
    graphics_extension_sender: Option<ipc::IpcSender<ipc_messages::graphics::GraphicsCommand>>,
    /// Crossbeam proxy for graphics extension events (composed scenes).
    graphics_event_receiver:
        crossbeam_channel::Receiver<ipc::IpcIncoming<ipc_messages::graphics::GraphicsEvent>>,
    /// Child process handle for the graphics process.
    /// Used during shutdown: sends Shutdown command, waits for
    /// ShutdownComplete, then joins the child.
    graphics_child: Option<std::process::Child>,

    /// Host integration used to surface navigation, paint, clipboard, and viewport state.
    host: Arc<dyn Embedder>,
    /// Trace logger for the Navigation TLA+ spec.
    tla_tracer: TLATracer,
    /// Monotonic-clock reading captured at the same moment as
    /// `epoch_anchor_wall_ms`; together they convert monotonic readings to
    /// epoch-relative milliseconds on the clock shared with the content
    /// processes (HR Time "estimated monotonic time of the Unix epoch").
    epoch_anchor: Instant,
    /// Wall-clock milliseconds since the Unix epoch at the moment
    /// `epoch_anchor` was captured.
    epoch_anchor_wall_ms: f64,
    /// Tracks which navigables have a queued update the rendering in
    /// flight (TLA spec: pending[f] > composed[f]).
    pending_update_the_rendering: HashSet<NavigableId>,
    /// The user-agent-side channel messaging state: the routing queue and
    /// per-port transfer state of the MessagePortExtraFG model.
    channel_messaging: crate::channel_messaging::ChannelMessaging,
    /// Batched rendering opportunities noted while an update was pending
    /// or no frame was needed yet (TLA spec op_count), with the epoch
    /// millisecond time the opportunity was noted (the event loop's "last
    /// render opportunity time"). Set semantics: multiple notes while
    /// pending collapse to one re-render; the latest note's timestamp wins.
    queued_rendering_opportunities: HashMap<NavigableId, f64>,
    /// Top-level traversables for which the embedder needs a frame
    /// (FrameNeeded, sent at each paint). Update the rendering is queued
    /// only when a frame is needed AND a rendering opportunity was noted.
    frame_needed: HashSet<NavigableId>,
    /// Sender cloned into child workers and sidecars when TLA tracing is enabled.
    trace_sender: Option<TraceSender>,
    /// request ids for automation round-trips across the user-agent and
    /// content event-loop boundary.
    next_automation_request_id: u64,
}

impl UserAgentWorker {
    /// starting the fetch worker owned by the user-agent thread.
    fn new(
        command_receiver: Receiver<UserAgentCommand>,
        host: Arc<dyn Embedder>,
        trace_sender: Option<TraceSender>,
    ) -> Self {
        let net_connection = crate::fetch::NetConnection::new(trace_sender.clone())
            .unwrap_or_else(|error| panic!("failed to start net extension: {error}"));

        // Start the graphics process (handles composition + media playback).
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
                    // Forward the trace sender to the graphics process.
                    if let Err(error) =
                        sender.send(ipc_messages::graphics::GraphicsCommand::SetTraceSender(
                            trace_sender.clone(),
                        ))
                    {
                        log::error!("failed to send trace sender to graphics: {error}");
                    }
                    let receiver = connection.receiver;
                    let child = handle.take_child();
                    (Some(sender), ipc::crossbeam_proxy(receiver), child)
                }
                Err(error) => {
                    log::error!("failed to start graphics process: {error}");
                    (None, crossbeam_channel::never(), None)
                }
            }
        };

        // HR Time "estimated monotonic time of the Unix epoch": simultaneous
        // wall-clock and monotonic readings at process start, so monotonic
        // instants can be converted to epoch-relative milliseconds
        // consistently across the user-agent and content processes.
        let epoch_anchor = Instant::now();
        let epoch_anchor_wall_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);

        Self {
            state: UserAgentState::default(),
            command_receiver,
            net_connection,

            graphics_extension_sender,
            graphics_event_receiver,
            graphics_child,
            host,
            tla_tracer: TLATracer::new("Navigation", "formal-web:user-agent", trace_sender.clone()),
            epoch_anchor,
            epoch_anchor_wall_ms,
            pending_update_the_rendering: HashSet::new(),
            channel_messaging: crate::channel_messaging::ChannelMessaging::new(
                trace_sender.clone(),
            ),
            queued_rendering_opportunities: HashMap::new(),
            frame_needed: HashSet::new(),
            trace_sender,
            next_automation_request_id: 1,
        }
    }

    /// the top-level command loop that owns browser-global coordination.
    /// Also processes net responses, graphics events, and content events from
    /// every owned event loop, selected over directly by this thread.
    fn run(&mut self) {
        loop {
            match self.select_any() {
                Some(Inbound::Command(command)) => {
                    if !self.handle_ua_command(command) {
                        break;
                    }
                }
                Some(Inbound::Net(response)) => {
                    self.handle_net_navigation_response(response);
                }
                Some(Inbound::Graphics(mut incoming)) => {
                    self.handle_graphics_event(&mut incoming);
                }
                Some(Inbound::Content(event_loop_id, incoming)) => {
                    match self.handle_content_event(event_loop_id, incoming) {
                        Ok(true) => {}
                        Ok(false) => self.forget_exited_event_loop(event_loop_id),
                        Err(error) => {
                            error!("content event handling error: {error}");
                        }
                    }
                }
                Some(Inbound::ContentDisconnected(event_loop_id)) => {
                    self.forget_exited_event_loop(event_loop_id);
                }
                None => break,
            }
        }
    }

    /// dropping one agent whose content process has exited, together with the
    /// traversables it owned.
    fn forget_exited_event_loop(&mut self, event_loop_id: EventLoopId) {
        if let Some(mut entry) = self.remove_event_loop_entry(event_loop_id) {
            entry.event_loop.reap_exited_child();
        }
    }

    /// Block until one of the user-agent, net, graphics, or content channels
    /// delivers an event.  Returns the kind of event plus its payload.
    fn select_any(&self) -> Option<Inbound> {
        let mut select = crossbeam_channel::Select::new();
        let cmd_handle = select.recv(&self.command_receiver);
        let net_handle = select.recv(self.net_connection.receiver());
        let gfx_handle = select.recv(&self.graphics_event_receiver);
        let mut content_handles: Vec<(usize, EventLoopId)> = Vec::new();
        for (event_loop_id, agent) in &self.state.agents {
            // Only a window agent owns an event channel: a dedicated worker
            // agent's events arrive over its hosting window agent's channel.
            let Agent::Window(window_agent) = agent else {
                continue;
            };
            let handle = select.recv(&window_agent.event_loop.event_receiver);
            content_handles.push((handle, *event_loop_id));
        }
        let oper = select.select();
        let idx = oper.index();
        if idx == cmd_handle {
            match oper.recv(&self.command_receiver) {
                Ok(command) => Some(Inbound::Command(command)),
                Err(_) => None,
            }
        } else if idx == net_handle {
            match oper.recv(self.net_connection.receiver()) {
                Ok(incoming) => Some(Inbound::Net(incoming.payload)),
                Err(_) => None,
            }
        } else if idx == gfx_handle {
            match oper.recv(&self.graphics_event_receiver) {
                Ok(incoming) => Some(Inbound::Graphics(incoming)),
                Err(_) => None,
            }
        } else {
            let (_, event_loop_id) = content_handles.iter().find(|(handle, _)| *handle == idx)?;
            let Agent::Window(window_agent) = self.state.agents.get(event_loop_id)? else {
                return None;
            };
            match oper.recv(&window_agent.event_loop.event_receiver) {
                Ok(incoming) => Some(Inbound::Content(*event_loop_id, incoming)),
                Err(_) => Some(Inbound::ContentDisconnected(*event_loop_id)),
            }
        }
    }

    /// Dispatch one user-agent command.  Returns `false` when the worker has
    /// been asked to shut down.
    fn handle_ua_command(&mut self, command: UserAgentCommand) -> bool {
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
                self.handle_set_traversable_viewport(traversable_id, snapshot, offset_x, offset_y);
            }
            UserAgentCommand::FrameNeeded { webview_id } => {
                self.handle_frame_needed(webview_id);
            }
            UserAgentCommand::SendUiEvent {
                webview_id,
                event_message,
            } => {
                self.handle_send_ui_event(webview_id, event_message);
            }
            UserAgentCommand::DispatchEventFor {
                traversable_id,
                event,
            } => {
                self.handle_dispatch_event_for(traversable_id, event);
            }
            UserAgentCommand::RenderingOpportunityFor { navigable_id } => {
                self.note_rendering_opportunity(navigable_id);
            }
            UserAgentCommand::Shutdown { reply } => {
                self.handle_shutdown(reply);
                return false;
            }
        }
        true
    }

    /// Handle one content-originated event from an event loop of one of this
    /// user agent's content processes.  The events are handled directly on
    /// the user-agent thread — there is no re-queue over the command channel:
    /// an event loop's state is a struct owned by this thread, and the
    /// handling for navigation, channel messaging and rendering work runs in
    /// the methods below, which mirror the spec steps the events stand for.
    ///
    /// `event_loop_id` is the event loop the event arrived on: the channel
    /// of a content process belongs to its similar-origin window agent's
    /// event loop, and every agent of the cluster (window and worker alike)
    /// sends its events over that same channel.  Events whose semantics
    /// depend on the sending agent therefore carry the agent's event loop id
    /// in their payload (the port-registration events), while events only a
    /// window agent's event loop emits (navigation, rendering, automation)
    /// are attributed to `event_loop_id` itself.
    ///
    /// Returns `Ok(false)` when the content process has shut down.
    fn handle_content_event(
        &mut self,
        event_loop_id: EventLoopId,
        incoming: ipc::IpcIncoming<ContentEvent>,
    ) -> Result<bool, String> {
        // A stale event for an event loop that was already removed is
        // dropped; its payload was superseded by the loop's teardown.
        let known_window_loop = matches!(
            self.state.agents.get(&event_loop_id),
            Some(Agent::Window(_))
        );
        if !known_window_loop {
            return Ok(true);
        }
        match incoming.payload {
            ContentEvent::NavigationRequested(request) => {
                // Navigation start leaves the window event loop and reenters
                // the user-agent navigation algorithm immediately.
                self.handle_navigate(Some(event_loop_id), request);
            }
            ContentEvent::PostMessageRequested(request) => {
                self.handle_post_message(request);
            }
            ContentEvent::PortChannelCreated {
                event_loop,
                port1,
                port2,
            } => {
                // The channel was created in the realm of `event_loop` (the
                // window event loop or a dedicated worker agent's event loop
                // of this cluster); register both ports with that loop as
                // their owner.
                self.handle_port_event(PortEvent::ChannelCreated {
                    port1,
                    port2,
                    event_loop,
                });
            }
            ContentEvent::PortTransferStarted { port } => {
                self.handle_port_event(PortEvent::TransferStarted { port });
            }
            ContentEvent::PortTransferReceived { event_loop, port } => {
                // The port was received in the realm of `event_loop` (the
                // window event loop or a dedicated worker agent's event loop
                // of this cluster); the transfer completes there.
                self.handle_port_event(PortEvent::TransferReceived { port, event_loop });
            }
            ContentEvent::PortMessageRouted { tgt, msg } => {
                self.handle_port_event(PortEvent::MessageRouted { tgt, msg });
            }
            ContentEvent::PortBufferReturned { tgt, buf } => {
                self.handle_port_event(PortEvent::BufferReturned { tgt, buf });
            }
            ContentEvent::PortTransferCompleted { tgt } => {
                self.handle_port_event(PortEvent::TransferCompleted { tgt });
            }
            ContentEvent::BeforeUnloadCompleted(result) => {
                self.handle_complete_before_unload(result);
            }
            ContentEvent::FinalizeNavigation(finalized) => {
                self.handle_finalize_cross_document_navigation(finalized);
            }
            ContentEvent::IframeTraversableRemoved(removal) => {
                self.handle_iframe_traversable_removed(
                    removal.parent_traversable_id,
                    removal.content_navigable_id,
                    removal.content_frame_id,
                );
            }
            ContentEvent::ScriptEvaluated(result) => {
                self.resolve_script_evaluation(event_loop_id, result);
            }
            ContentEvent::ElementClicked(result) => {
                self.resolve_click_evaluation(event_loop_id, result);
            }
            ContentEvent::ClipboardWriteRequested(
                ipc_messages::content::ClipboardWriteRequested { text },
            ) => {
                // Fire-and-forget: write to system clipboard, no reply expected.
                if let Err(error) = self.host.clipboard_set_text(text) {
                    error!("clipboard write failed: {error}");
                }
            }
            ContentEvent::TitleChanged(ipc_messages::content::TitleChanged {
                traversable_id,
                title,
            }) => {
                // The content process reports the parsed title of the
                // top-level document; forward it to the embedder.
                if let Err(error) = self.host.title_changed(WebviewId(traversable_id), title) {
                    error!("failed to forward document title: {error}");
                }
            }
            ContentEvent::RegisterMediaPipeline(_) => {
                // Content sends CreateMediaPipeline directly to the graphics process.
                // No UA bookkeeping needed.
            }
            ContentEvent::RenderingOpRequested(navigable_id) => {
                self.note_rendering_opportunity(navigable_id);
            }
            ContentEvent::DedicatedWorkerAgentObtained {
                worker_id,
                event_loop_id: worker_event_loop_id,
                owner,
                ua_command_sender,
            } => {
                // Run-a-worker step 4's user-agent half: a dedicated worker
                // agent of this cluster was obtained (its thread and worker
                // event loop were created in the content process).  Record
                // the agent under its own worker event loop id, beside the
                // cluster's similar-origin window agent, so port tasks
                // routed to a port of the agent's event loop are sent
                // directly to the worker over the agent's own command
                // channel.  The event arrived on the event channel of the
                // window agent hosting the worker's thread (the cluster's
                // content process); that window agent's signifier is
                // recorded on the worker agent as its host, so the record
                // is dropped when the process exits.
                // <https://html.spec.whatwg.org/multipage/webappapis.html#dedicated-worker-agent>
                let Some(host_agent_id) =
                    window_agent(&self.state, event_loop_id).map(|agent| agent.id)
                else {
                    error!(
                        "dedicated worker agent {worker_id} reported for unknown window agent {event_loop_id}"
                    );
                    return Ok(true);
                };
                self.state.agents.insert(
                    worker_event_loop_id,
                    Agent::DedicatedWorker(DedicatedWorkerAgent {
                        worker_id,
                        owner,
                        host_agent_id,
                        event_loop: WorkerEventLoop {
                            event_loop_id: worker_event_loop_id,
                            command_sender: ua_command_sender,
                        },
                    }),
                );
            }
            ContentEvent::DedicatedWorkerAgentClosed { worker_id } => {
                // The worker agent's event loop was destroyed (run-a-worker
                // steps 12.19-12.21 ran in the content process); drop the
                // agent record — it is registered under its worker event
                // loop id — so its event loop is no longer a port-task
                // destination.
                let closed_worker_loop_id =
                    self.state
                        .agents
                        .iter()
                        .find_map(|(event_loop_id, agent)| match agent {
                            Agent::DedicatedWorker(worker_agent)
                                if worker_agent.worker_id == worker_id =>
                            {
                                Some(*event_loop_id)
                            }
                            _ => None,
                        });
                if let Some(event_loop_id) = closed_worker_loop_id {
                    self.state.agents.remove(&event_loop_id);
                }
            }
            ContentEvent::ShutdownCompleted => return Ok(false),
        }
        Ok(true)
    }

    /// Resolve the automation waiter of a content script-evaluation reply
    /// (the agent owning `event_loop_id` stores it).
    fn resolve_script_evaluation(
        &mut self,
        event_loop_id: EventLoopId,
        result: ipc_messages::content::ScriptEvaluationResult,
    ) {
        let Some(agent) = window_agent_mut(&mut self.state, event_loop_id) else {
            return;
        };
        if let Some(waiter) = agent.event_loop.script_waiters.remove(&result.request_id) {
            let send_result = match result.error {
                Some(error) => Err(error),
                None => serde_json::from_str(&result.value_json).map_err(|error| {
                    format!("failed to decode content script evaluation result: {error}")
                }),
            };
            let _ = waiter.send(send_result);
        }
    }

    /// Resolve the automation waiter of a content click reply (the agent
    /// owning `event_loop_id` stores it).
    fn resolve_click_evaluation(
        &mut self,
        event_loop_id: EventLoopId,
        result: ipc_messages::content::ElementClickResult,
    ) {
        let Some(agent) = window_agent_mut(&mut self.state, event_loop_id) else {
            return;
        };
        if let Some(waiter) = agent.event_loop.click_waiters.remove(&result.request_id) {
            let _ = waiter.send(result.error.map_or(Ok(()), Err));
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
    /// Sending one command directly to a content process's event loop.
    fn send_event_loop_command(
        &self,
        command_sender: &ipc::IpcSender<ContentCommand>,
        command: ContentCommand,
    ) -> Result<(), String> {
        command_sender
            .send(command)
            .map_err(|error| format!("failed to send event-loop command: {error}"))
    }

    /// resolving the content command sender that owns one traversable.
    fn command_sender_for_traversable(
        &self,
        traversable_id: NavigableId,
    ) -> Result<ipc::IpcSender<ContentCommand>, String> {
        let event_loop_id = self
            .state
            .traversable_handles
            .get(&traversable_id)
            .copied()
            .ok_or_else(|| format!("unknown traversable id: {traversable_id}"))?;
        window_agent(&self.state, event_loop_id)
            .map(|agent| agent.event_loop.command_sender.clone())
            .ok_or_else(|| format!("missing agent for event loop id {event_loop_id}"))
    }

    /// Resolve the content command sender for the event loop that owns a
    /// document.  After a cross-process navigation the traversable has moved
    /// to a new event loop while the outgoing document is still owned by the
    /// old one; document-routed commands (e.g. DestroyDocument) must reach the
    /// process that actually holds the document so its teardown can run there.
    fn command_sender_for_document(
        &self,
        document_id: DocumentId,
    ) -> Result<ipc::IpcSender<ContentCommand>, String> {
        let event_loop_id = self
            .state
            .documents
            .get(&document_id)
            .map(|document| document.event_loop_id)
            .ok_or_else(|| format!("unknown document id: {document_id}"))?;
        window_agent(&self.state, event_loop_id)
            .map(|agent| agent.event_loop.command_sender.clone())
            .ok_or_else(|| format!("missing agent for event loop id {event_loop_id}"))
    }

    /// <https://html.spec.whatwg.org/multipage/#create-an-agent>
    fn create_an_agent(
        &mut self,
        can_block: bool,
        process_label: String,
    ) -> Result<SimilarOriginWindowAgent, String> {
        // Step 1: Let signifier be a new unique internal value.
        let agent_id = AgentId::new();
        // Step 2: Let candidateExecution be a new candidate execution.
        // Note: The Rust model does not surface a separate candidate-execution object because the
        // user-agent thread owns the scheduling state that HTML leaves implementation-defined.
        // Step 4: Set agent's event loop to a new event loop.
        // Note: The event loop of a similar-origin window agent is a window
        // event loop; it runs on the main thread of the agent cluster's
        // content process, so creating it here spawns that process (the
        // agent cluster the agent is added to) and bootstraps the loop
        // inside it via ContentBootstrap.  The returned record is owned
        // directly by the user-agent thread.
        let event_loop_id = EventLoopId::new();
        let event_loop = spawn_window_event_loop(
            event_loop_id,
            process_label,
            self.host.clone(),
            self.trace_sender.clone(),
            self.net_connection.sender(),
            self.graphics_extension_sender.clone(),
        )?;
        // Step 3: Let agent be a new agent whose [[CanBlock]] is canBlock, [[Signifier]] is
        // signifier, [[CandidateExecution]] is candidateExecution, and [[IsLockFree1]],
        // [[IsLockFree2]], and [[LittleEndian]] are set at the implementation's discretion.
        // Note: The lock-free details remain implicit.
        // Step 5: Return agent.
        Ok(SimilarOriginWindowAgent {
            id: agent_id,
            can_block,
            event_loop,
            traversable_ids: HashSet::new(),
        })
    }

    /// <https://html.spec.whatwg.org/multipage/#obtain-similar-origin-window-agent>
    fn obtain_similar_origin_window_agent(
        &mut self,
        origin_site: &str,
        browsing_context_group_id: BrowsingContextGroupId,
        process_label: String,
    ) -> Result<EventLoopId, String> {
        // Step 1: Let site be the result of obtaining a site with origin.
        // Step 2: Let key be site.
        // Step 3: If group's cross-origin isolation mode is not "none", then
        // set key to origin.
        // Step 4: Otherwise, if group's historical agent cluster key
        // map[origin] exists, then set key to group's historical agent
        // cluster key map[origin].
        // Step 5: Otherwise, if requestsOAC is true, then set key to origin.
        // Note: Steps 1-5 resolve the agent cluster key.  Only site keys are
        // reached: the paths that obtain a window agent pass a site and run
        // with requestsOAC false, the group's cross-origin isolation mode is
        // always "none", and the historical agent cluster key map is not
        // modeled.
        let key = AgentClusterKey::Site(origin_site.to_owned());
        // Step 6: If group's agent cluster map[key] does not exist:
        // Note: A cluster whose content process was stopped is not reusable:
        // its recorded window agent is gone, so the record counts as a miss
        // and is replaced below.  The window agent is referenced by its
        // signifier; its registry key (the event loop id of its window
        // event loop) is resolved here so the returned handle addresses the
        // `state.agents` registry directly.
        let recorded_agent_event_loop = self
            .state
            .browsing_context_group_set
            .members
            .get(&browsing_context_group_id)
            .and_then(|group| group.agent_cluster_map.get(&key))
            .and_then(|cluster| {
                self.state.agents.values().find_map(|agent| match agent {
                    Agent::Window(window_agent)
                        if window_agent.id == cluster.similar_origin_window_agent =>
                    {
                        Some(window_agent.event_loop.event_loop_id)
                    }
                    _ => None,
                })
            });
        let event_loop_id = if let Some(event_loop_id) = recorded_agent_event_loop {
            event_loop_id
        } else {
            // Step 6.1: Let agentCluster be a new agent cluster.
            // Step 6.2: Set agentCluster's cross-origin isolation mode to
            // group's cross-origin isolation mode.
            // Step 6.3: If key is an origin, ... (not reached: key is a
            // site, so the cluster is not origin-keyed).
            // Step 6.4: Add the result of creating an agent, given false, to
            // agentCluster.
            // Note: Creating the agent realizes its window event loop by
            // spawning the agent cluster's content process (see
            // `create_an_agent`); inserting the agent into `state.agents`
            // under its event loop id is the user-agent-side "adding the
            // agent to the cluster".
            let agent = self.create_an_agent(false, process_label)?;
            let agent_id = agent.id;
            let event_loop_id = agent.event_loop.event_loop_id;
            self.state
                .agents
                .insert(event_loop_id, Agent::Window(agent));
            // Step 6.5: Set group's agent cluster map[key] to agentCluster.
            let group = self
                .state
                .browsing_context_group_set
                .members
                .get_mut(&browsing_context_group_id)
                .ok_or_else(|| {
                    format!("missing browsing context group {browsing_context_group_id}")
                })?;
            group.agent_cluster_map.insert(
                key,
                AgentCluster {
                    id: AgentClusterId::new(),
                    cross_origin_isolation_mode: group.cross_origin_isolation_mode,
                    is_origin_keyed: false,
                    similar_origin_window_agent: agent_id,
                },
            );
            event_loop_id
        };
        // Step 7: Return the single similar-origin window agent contained in
        // group's agent cluster map[key].
        Ok(event_loop_id)
    }

    /// <https://html.spec.whatwg.org/multipage/#creating-a-new-top-level-traversable>
    fn create_new_top_level_traversable(
        &mut self,
        target_name: String,
    ) -> Result<NavigableId, String> {
        let traversable_id = NavigableId::new();
        let iframe_parent_traversable_id = None;
        let frame_id = None;

        // Step 1: Let document be null.
        // Step 2: If opener is null, then set document to the second return
        // value of creating a new top-level browsing context and document.
        // Note: Null-opener branch.  The UA-side of "creating a new top-level
        // browsing context and document" runs below and in
        // `create_a_new_browsing_context` (the browsing context's group
        // membership and the active-document mapping, steps 1, 9, 23 of
        // "creating a new browsing context and document"); the
        // document-owning steps (10, 13, 15, 22) run in the content process
        // via the CreateEmptyDocument IPC below, and step 24 ("completely
        // finish loading") runs there too.
        let browsing_context_group_id = self.state.browsing_context_group_set.next_group_id();
        let browsing_context_id = BrowsingContextId::new();
        let document_id = DocumentId::new();
        // Step 9 (of "creating a new browsing context and document"): Let
        // agent be the result of obtaining a similar-origin window agent
        // given origin, group, and false.
        // Note: The fresh browsing context group is created empty below; it
        // has no agent cluster for the about:blank site key, so obtaining
        // the agent creates the cluster — spawning the content process — and
        // creates the window agent inside it, recording the cluster in the
        // group's agent cluster map (see
        // `obtain_similar_origin_window_agent`).
        self.state.browsing_context_group_set.members.insert(
            browsing_context_group_id,
            BrowsingContextGroup {
                id: browsing_context_group_id,
                browsing_context_set: HashMap::new(),
                agent_cluster_map: HashMap::new(),
                historical_agent_cluster_key_map: HashMap::new(),
                cross_origin_isolation_mode: CrossOriginIsolationMode::None,
            },
        );
        let agent_event_loop_id = self.obtain_similar_origin_window_agent(
            "about:blank",
            browsing_context_group_id,
            String::from("about:blank"),
        )?;
        let command_sender = window_agent(&self.state, agent_event_loop_id)
            .expect("obtained window agent missing from state")
            .event_loop
            .command_sender
            .clone();

        if startup_debug_enabled() {
            trace!(
                "[startup-debug][user-agent] create_new_top_level_traversable sending CreateEmptyDocument traversable={} document={} event_loop={}",
                traversable_id, document_id, agent_event_loop_id
            );
        }

        // Step 3: Let documentState be a new document state, with document,
        // initiator origin, origin, navigable target name, and about base URL.
        // Note: The Rust model splits document-state fields across the
        // traversable maps, `DocumentState`, and `traversable_target_names`;
        // the fields that live in content (document reference, origin) are
        // created by the content process when it handles CreateEmptyDocument.
        // Step 4: Let traversable be a new traversable navigable.
        // Step 5: Initialize the navigable traversable given documentState.
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
                traversable_id, document_id, agent_event_loop_id
            );
        }

        window_agent_mut(&mut self.state, agent_event_loop_id)
            .expect("agent disappeared during top-level creation")
            .traversable_ids
            .insert(traversable_id);
        self.state
            .traversable_handles
            .insert(traversable_id, agent_event_loop_id);
        self.state
            .traversable_target_names
            .insert(traversable_id, target_name.clone());
        self.state
            .top_level_browsing_context_group_ids
            .insert(browsing_context_id, browsing_context_group_id);
        self.create_a_new_browsing_context(
            traversable_id,
            document_id,
            agent_event_loop_id,
            browsing_context_group_id,
            browsing_context_id,
            false,
        )?;
        // Step 6: Let initialHistoryEntry be traversable's active session history entry.
        // Note: The initial session history entry is materialized directly in the literal below
        // instead of through a separate temporary binding.
        // Step 7: Set initialHistoryEntry's step to 0.
        // Note: The same literal below stores step `0` directly on the inserted entry.
        // Step 8: Append initialHistoryEntry to traversable's session history entries.
        // Note: The `session_history_entries` vector below performs the initial append.
        // Step 9: If opener is non-null, then legacy-clone a traversable storage shed given
        // opener's top-level traversable and traversable.
        // Note: This helper models the null-opener branch only, so it intentionally skips storage
        // shed cloning.
        // Step 10: Append traversable to the user agent's top-level traversable set.
        self.state.navigables.insert(
            traversable_id,
            Navigable {
                id: traversable_id,
                parent_navigable_id: iframe_parent_traversable_id,
                active_document_id: Some(document_id),
                is_active: false,
                target_name: target_name.clone(),
                active_browsing_context_id: Some(browsing_context_id),
                event_loop_id: Some(agent_event_loop_id),
                handle: Some(agent_event_loop_id),
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
        verification::tla_log!(self.tla_tracer, "CreateNavigable", traversable_id);
        // The frame is the graphics-side effect of the navigable's creation;
        // traced for the RenderingOpportunity spec too so the model's frame
        // set can grow from the trace alone (top-level frame: no parent).
        verification::tla_log!(
            self.tla_tracer,
            -> "RenderingOpportunity",
            "CreateFrame",
            traversable_id
        );
        if target_name_keeps_browser_ui_focus(&target_name) {
            self.state.set_active_top_level_traversable(traversable_id);
        }

        // Step 11: Invoke WebDriver BiDi navigable created with traversable and
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
        // Register the webview with the graphics process.
        if let Some(graphics_sender) = &self.graphics_extension_sender
            && let Err(error) =
                graphics_sender.send(ipc_messages::graphics::GraphicsCommand::RegisterWebview {
                    webview_id: WebviewId(traversable_id),
                })
        {
            error!("failed to register webview with graphics process: {error}");
        }
        // Step 12: Return traversable.
        Ok(traversable_id)
    }

    /// <https://html.spec.whatwg.org/#creating-a-new-browsing-context>
    fn create_a_new_browsing_context(
        &mut self,
        navigable_id: NavigableId,
        document_id: DocumentId,
        event_loop_id: EventLoopId,
        browsing_context_group_id: BrowsingContextGroupId,
        browsing_context_id: BrowsingContextId,
        is_auxiliary: bool,
    ) -> Result<(), String> {
        // Step 1: Let browsingContext be a new browsing context.
        // Note: The browsing context id was allocated by the caller (it also
        // needs it for the navigable record); the record is registered in its
        // browsing context group below.
        // Step 2: Let unsafeContextCreationTime be the unsafe shared current
        //         time.
        // Step 3: Let creatorOrigin be null.
        // Step 4: Let creatorBaseURL be null.
        // Step 5: If creator is non-null:
        // Step 5.1: Set creatorOrigin to creator's origin.
        // Step 5.2: Set creatorBaseURL to creator's document base URL.
        // Step 5.3: Set browsingContext's virtual browsing context group ID to
        //           creator's browsing context's top-level browsing context's
        //           virtual browsing context group ID.
        // Step 6: Let sandboxFlags be the result of determining the creation
        //         sandboxing flags given browsingContext and embedder.
        // Step 7: Let origin be the result of determining the origin given
        //         about:blank, sandboxFlags, and creatorOrigin.
        // Step 8: Let permissionsPolicy be the result of creating a permissions
        //         policy given embedder and origin.
        // Note: Steps 2-8 are not implemented: creation time, creator state,
        // sandboxing and permissions policy are not tracked.
        // Step 9: Let agent be the result of obtaining a similar-origin window
        //         agent given origin, group, and false.
        // Note: The caller resolved the agent: a new agent and event loop for a
        // fresh top-level traversable, the content process's event loop for a
        // content-initiated traversable, or the parent's event loop for a child
        // navigable.
        // Step 10: Let realm execution context be the result of creating a new
        // realm given agent and the following customizations: for the global
        // object, create a new Window object; for the global this binding, use
        // browsingContext's WindowProxy object.
        // Step 13: Set up a window environment settings object with about:blank,
        // realm execution context, null, topLevelCreationURL, and topLevelOrigin.
        // Step 15: Let document be a new Document, with: type "html"; content
        // type "text/html"; mode "quirks"; origin origin; browsing context
        // browsingContext; permissions policy permissionsPolicy; active
        // sandboxing flag set sandboxFlags; load timing info loadTimingInfo; is
        // initial about:blank true; about base URL creatorBaseURL; allow
        // declarative shadow roots true; custom element registry a new
        // CustomElementRegistry object.
        // Step 22: Populate with html/head/body given document.
        // Note: Steps 10, 13, 15 and 22 ran in the content process before this
        // method: the document, realm, Window and environment settings object
        // were created by `create_a_new_browsing_context_and_document` in
        // content/src/html.rs (via the CreateEmptyDocument IPC for the
        // UA-initiated path, before the NavigateRequest was sent for the
        // content-initiated paths).
        // Step 11: Let topLevelCreationURL be about:blank if embedder is null;
        //         otherwise embedder's relevant settings object's top-level
        //         creation URL.
        // Step 12: Let topLevelOrigin be origin if embedder is null; otherwise
        //         embedder's relevant settings object's top-level origin.
        // Step 14: Let loadTimingInfo be a new document load timing info with
        //         its navigation start time set to the result of calling
        //         coarsen time with unsafeContextCreationTime and the new
        //         environment settings object's cross-origin isolated
        //         capability.
        // Step 16: Let iframeReferrerPolicy be the result of determining the
        //         iframe element referrer policy given embedder.
        // Step 17: Set document's internal ancestor origin objects list to the
        //         result of running the internal ancestor origin objects list
        //         creation steps given document and iframeReferrerPolicy.
        // Step 18: Set document's ancestor origins list to the result of
        //         running the ancestor origins list creation steps given
        //         document.
        // Step 19: If creator is non-null:
        // Step 19.1: Set document's referrer to the serialization of creator's
        //            URL.
        // Step 19.2: Set document's policy container to a clone of creator's
        //            policy container.
        // Step 19.3: If creator's origin is same origin with creator's
        //            relevant settings object's top-level origin, then set
        //            document's opener policy to creator's browsing context's
        //            top-level browsing context's active document's opener
        //            policy.
        // Step 20: Assert: document's URL and document's relevant settings
        //         object's creation URL are about:blank.
        // Step 21: Mark document as ready for post-load tasks.
        // Note: Steps 11-12, 14 and 16-21 are not implemented.
        // Step 23: Make active document.
        let group = self
            .state
            .browsing_context_group_set
            .members
            .get_mut(&browsing_context_group_id)
            .ok_or_else(|| format!("missing browsing context group {browsing_context_group_id}"))?;
        // Note: The is-auxiliary flag (step 5 of "creating a new auxiliary
        // browsing context and document") and the group append (step 6 of
        // that algorithm) are realized here.
        group.browsing_context_set.insert(
            browsing_context_id,
            BrowsingContext {
                id: browsing_context_id,
                is_auxiliary,
                opener_browsing_context: None,
                is_popup: false,
            },
        );
        // Step 23: Make active document.
        // Note: The UA-side active-document mapping; the content process tracks
        // its own via `active_documents_by_traversable`.  The UA-side document
        // state maps the navigable to the content-created document for
        // navigation and session history purposes.
        self.state
            .set_navigable_active_document(navigable_id, document_id);
        self.state.documents.insert(
            document_id,
            DocumentState {
                traversable_id: navigable_id,
                browsing_context_id: Some(browsing_context_id),
                event_loop_id,
                url: String::from("about:blank"),
                is_initial_about_blank: true,
            },
        );
        // Step 24: Completely finish loading document.
        // Note: Ran in the content process: the UA-initiated path's
        // CreateEmptyDocument handling executes parser-discovered scripts; the
        // content-initiated paths already ran it.
        // Step 25: Return browsingContext and document.
        // Note: The document is identified by `document_id`; the browsing
        // context by `browsing_context_id`.
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#create-a-new-child-navigable>
    fn create_a_new_child_navigable(
        &mut self,
        parent_navigable_id: NavigableId,
        content_navigable_id: NavigableId,
        content_frame_id: FrameId,
        document_id: DocumentId,
        target_name: Option<String>,
    ) -> Result<NavigableId, String> {
        // Step 1: Let parentNavigable be element's node navigable.
        // Note: Ran in content: the iframe element's node navigable, passed as
        // `parent_navigable_id`.
        // Step 4: Let targetName be null.
        // Step 5: If element has a name content attribute, then set targetName
        // to the value of that attribute.
        // Note: Ran in content: the iframe element's name attribute, received
        // as `target_name`.
        let _requested_target_name = target_name;
        // TODO: Store requested iframe `name` attribute on document state once child-target
        // lookup uses document-state target names.
        let target_name =
            iframe_target_name(parent_navigable_id, content_navigable_id, content_frame_id);
        if let Some(navigable_id) = find_navigable_by_target_name(&self.state, &target_name) {
            return Ok(navigable_id);
        }

        // Step 2: Let group be element's node document's browsing context's
        //         top-level browsing context's group.
        // Note: The group already exists — it is the parent navigable's browsing
        // context group, resolved below from the parent's top-level browsing
        // context.  The content process has no group state: being in the same
        // process is its only notion of "same group".
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

        // Step 3: Let browsingContext and document be the result of creating a
        // new browsing context and document given element's node document,
        // element, and group.
        // Note: User-agent portion of this step: the browsing context's group
        // membership, document state and active-document mapping (steps 1, 9,
        // 23 of "creating a new browsing context and document") run in
        // `create_a_new_browsing_context` below; the document-owning steps
        // (10, 13, 15, 22) already ran in content via
        // `create_a_new_browsing_context_and_document`.  Child navigables
        // reuse the parent's event loop ("obtaining a similar-origin window
        // agent", step 9) until a cross-origin navigation moves them via
        // `initialise_the_document_object`.
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

        // Step 6: Let documentState be a new document state, with document,
        //         initiator origin, origin, navigable target name, and about base
        //         URL.
        // Step 7: Let navigable be a new navigable.
        // Step 8: Initialize the navigable navigable given documentState and
        //         parentNavigable.
        // <https://html.spec.whatwg.org/#initialize-the-navigable>
        // Note: The document reference and origin live in content; the
        // navigable-target-name and about-base-URL fields are not tracked.  The
        // navigable's parent, active document, current/active session history
        // entry, event loop and target name (steps 7-8) are set up below; the
        // navigable id `content_navigable_id` was allocated by content.
        // Step 9: Set element's content navigable to navigable.
        // Note: Ran in content: the element's content navigable was set when
        // the CreateChildNavigable IPC was prepared.
        self.state
            .traversable_handles
            .insert(traversable_id, parent_event_loop_id);
        self.state
            .traversable_target_names
            .insert(traversable_id, target_name.clone());
        self.create_a_new_browsing_context(
            traversable_id,
            document_id,
            parent_event_loop_id,
            group_id,
            browsing_context_id,
            false,
        )?;

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
        // Step 10: Let historyEntry be navigable's active session history entry.
        // Step 11: Let traversable be parentNavigable's traversable navigable.
        // Note: This codebase routes every navigable through the traversable
        // maps, so the child navigable's own id serves as its traversable id;
        // the parent's traversable is the `parent_traversable_id` carried by
        // the CreateChildNavigable IPC.
        // Step 12: Append the following session history traversal steps to
        //          traversable.
        // Note: Partial: the initial session history entry (step 0, above)
        // stands in for the appended traversal steps; nested histories and
        // "update for navigable creation/destruction" are not modeled.
        verification::tla_log!(
            self.tla_tracer,
            "CreateChildNavigable",
            traversable_id,
            parent_navigable_id
        );
        // The child frame is the graphics-side effect of the child
        // navigable's creation; traced for the RenderingOpportunity spec
        // too (child frame with its parent) so the model's frame set can
        // grow from the trace alone.
        verification::tla_log!(
            self.tla_tracer,
            -> "RenderingOpportunity",
            "CreateFrame",
            traversable_id,
            parent_navigable_id
        );
        window_agent_mut(&mut self.state, parent_event_loop_id)
            .ok_or_else(|| format!("missing parent event loop {parent_event_loop_id}"))?
            .traversable_ids
            .insert(traversable_id);

        // Step 13: Invoke WebDriver BiDi navigable created with traversable.
        // Note: Not performed for child navigables: the embedder notification
        // (`new_webview`) and the graphics registration below are the
        // observable creation hooks; the WebDriver notification exists only
        // for top-level traversables (`create_new_top_level_traversable`).
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
        // Store immediately in the UA state so event routing works before
        // the first ComposedSceneReady arrives from the graphics process.
        self.state
            .child_frame_to_webview
            .entry(WebviewId(parent_navigable_id))
            .or_default()
            .insert(content_frame_id, WebviewId(traversable_id));
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
        let navigation_event_loop_id = self
            .state
            .traversable_handles
            .get(&traversable_id)
            .copied()
            .ok_or_else(|| format!("no event loop owns traversable {traversable_id}"))?;
        if let Err(error) = self.net_connection.start_navigation_fetch(
            fetch_id,
            navigation_event_loop_id,
            request.to_navigation_fetch_request(),
        ) {
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
            self.tla_tracer,
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
        verification::tla_log!(self.tla_tracer, "StartNavigating", navigation_id);

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
            if let Err(error) = command_sender.send(ContentCommand::RunBeforeUnload {
                document_id,
                check_id,
                navigation_id,
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
        if !normalized_target_name.eq_ignore_ascii_case("_blank")
            && !noopener
            && let Some(chosen) =
                find_navigable_by_target_name(&self.state, &normalized_target_name)
        {
            return Ok((chosen, String::from("existing or none")));
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

    /// Removing one window agent (its event-loop worker) and every derived
    /// index owned by it: its traversables, and the dedicated worker agents
    /// of the workers its content process hosted (the process exit takes
    /// their threads down).
    fn remove_event_loop_entry(
        &mut self,
        event_loop_id: EventLoopId,
    ) -> Option<SimilarOriginWindowAgent> {
        // Only a window agent's event loop owns traversables and a process
        // to stop; a worker event loop id (or an already-removed loop) is a
        // no-op here.
        let entry = match self.state.agents.remove(&event_loop_id) {
            Some(Agent::Window(entry)) => entry,
            Some(Agent::DedicatedWorker(_)) | None => return None,
        };
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
        // Note: The removed record is the similar-origin window agent of an
        // agent cluster whose content process has exited; the worker agents
        // of the workers that process hosted (see
        // `DedicatedWorkerAgent::host_agent_id`) are dropped with it, so
        // their event loops are no longer port-task destinations.
        let host_agent_id = entry.id;
        self.state.agents.retain(|_, agent| {
            !matches!(
                agent,
                Agent::DedicatedWorker(worker) if worker.host_agent_id == host_agent_id
            )
        });
        Some(entry)
    }

    /// stopping one owned agent's event loop by its Rust handle.
    fn stop_event_loop_handle(&mut self, event_loop_id: EventLoopId) {
        if let Some(mut entry) = self.remove_event_loop_entry(event_loop_id) {
            entry.event_loop.shutdown();
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#create-a-fresh-top-level-traversable>
    fn create_a_fresh_top_level_traversable(&mut self, destination_url: String) {
        if startup_debug_enabled() {
            trace!(
                "[startup-debug][user-agent] create_fresh_top_level_traversable destination_url={}",
                destination_url
            );
        }
        let result: Result<(), String> = (|| {
            // Step 1: Let traversable be the result of creating a new top-level traversable given
            // null and the empty string.
            // Note: `create_new_top_level_traversable` implements the UA-side of "creating a
            // new top-level traversable": the new browsing context group, browsing context and
            // traversable state, plus the CreateEmptyDocument IPC that runs the document-owning
            // steps in the content process.
            let traversable_id = self.create_new_top_level_traversable(String::new())?;
            // Step 2: Navigate traversable to initialNavigationURL using traversable's active
            // document, with documentResource set to initialNavigationPostResource.
            // Note: The navigate call below is the UA-side navigate; the documentResource
            // parameter is not modeled.
            self.navigate(
                traversable_id,
                destination_url,
                UserNavigationInvolvement::BrowserUi,
                NavigationId::new(),
            )?;
            // Step 3: Return traversable.
            Ok(())
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
    fn creating_a_new_top_level_traversable(
        &mut self,
        traversable_id: NavigableId,
        event_loop_id: EventLoopId,
        info: &NewTraversableInfo,
    ) -> Result<(), String> {
        let document_id = info.document_id;
        let target_name = &info.target_name;

        // Step 1: Let document be null.
        // Note: The document is the content-created about:blank document
        // (`document_id`); the opener branch of step 2 fills the document.
        // Step 2: Otherwise, set document to the second return value of
        // creating a new auxiliary browsing context and document given opener.
        // Note: Opener branch (window.open without noopener).  The UA-side of
        // "creating a new auxiliary browsing context and document" runs below
        // and in `create_a_new_browsing_context`: a new browsing context in a
        // browsing context group (step 6 of that algorithm appends it to the
        // group), the is-auxiliary flag (step 5), and the opener relationship
        // (step 7, set afterwards by `setup_opener_for_window_open`).  The
        // document-owning steps (10, 13, 15, 22 of "creating a new browsing
        // context and document") already ran in content before the
        // NavigateRequest was sent.
        // Note: The spec appends the auxiliary browsing context to the
        // opener's browsing context group; here a fresh browsing context
        // group is created instead, because the opener's group is not
        // threaded through the `new_traversable_info` on NavigateRequest.
        let browsing_context_group_id = self.state.browsing_context_group_set.next_group_id();
        let browsing_context_id = BrowsingContextId::new();
        let agent_cluster_id = AgentClusterId::new();

        // Step 9: Let agent be the result of obtaining a similar-origin window
        // agent given origin, group, and false.
        // Note: The new traversable runs on the opener's event loop — the
        // content process that created its document — so obtaining the agent
        // resolves the existing window agent of that event loop (it is not
        // re-created); the fresh browsing context group's cluster records
        // that agent's signifier below.
        let agent_id = window_agent(&self.state, event_loop_id)
            .map(|agent| agent.id)
            .ok_or_else(|| format!("missing window agent for event loop {event_loop_id}"))?;

        // Step 3: Let documentState be a new document state, with document,
        // initiator origin, origin, navigable target name, and about base URL.
        // Step 4: Let traversable be a new traversable navigable.
        // Step 5: Initialize the navigable traversable given documentState.
        // Note: The traversable's UA-side state (event loop registration,
        // traversable maps) is set up below; the document-state fields that
        // live in content (document reference, origin) were created by the
        // content process.
        if let Some(agent) = window_agent_mut(&mut self.state, event_loop_id) {
            agent.traversable_ids.insert(traversable_id);
        }
        self.state
            .traversable_handles
            .insert(traversable_id, event_loop_id);
        self.state
            .traversable_target_names
            .insert(traversable_id, target_name.clone());
        self.state
            .top_level_browsing_context_group_ids
            .insert(browsing_context_id, browsing_context_group_id);
        self.state.browsing_context_group_set.members.insert(
            browsing_context_group_id,
            BrowsingContextGroup {
                id: browsing_context_group_id,
                browsing_context_set: HashMap::new(),
                agent_cluster_map: HashMap::from([(
                    AgentClusterKey::Site(String::from("about:blank")),
                    AgentCluster {
                        id: agent_cluster_id,
                        cross_origin_isolation_mode: CrossOriginIsolationMode::None,
                        is_origin_keyed: false,
                        similar_origin_window_agent: agent_id,
                    },
                )]),
                historical_agent_cluster_key_map: HashMap::new(),
                cross_origin_isolation_mode: CrossOriginIsolationMode::None,
            },
        );
        self.create_a_new_browsing_context(
            traversable_id,
            document_id,
            event_loop_id,
            browsing_context_group_id,
            browsing_context_id,
            true,
        )?;

        // Step 6: Let initialHistoryEntry be traversable's active session history entry.
        // Step 7: Set initialHistoryEntry's step to 0.
        // Step 8: Append initialHistoryEntry to traversable's session history entries.
        // Note: The initial session history entry is materialized directly in
        // the literal below (step 0).
        // Step 9: If opener is non-null, then legacy-clone a traversable
        // storage shed given opener's top-level traversable and traversable.
        // Note: Not implemented: storage sheds are not modeled.
        // Step 10: Append traversable to the user agent's top-level traversable set.
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

        verification::tla_log!(self.tla_tracer, "CreateNavigable", traversable_id);
        // The frame is the graphics-side effect of the navigable's creation;
        // traced for the RenderingOpportunity spec too (top-level frame: no
        // parent).
        verification::tla_log!(
            self.tla_tracer,
            -> "RenderingOpportunity",
            "CreateFrame",
            traversable_id
        );

        if target_name_keeps_browser_ui_focus(target_name) {
            self.state.set_active_top_level_traversable(traversable_id);
        }
        // Step 11: Invoke WebDriver BiDi navigable created with traversable
        // and openerNavigableForWebDriver.
        // Note: The embedder notification (`new_webview`) is invoked by the
        // caller — `handle_navigate` calls
        // `create_webview_for_new_top_level_traversable` after this returns.
        // Step 12: Return traversable.
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

            // Register the webview with the graphics process.
            if let Some(graphics_sender) = &self.graphics_extension_sender
                && let Err(error) =
                    graphics_sender.send(ipc_messages::graphics::GraphicsCommand::RegisterWebview {
                        webview_id: WebviewId(navigable_id),
                    })
            {
                error!("failed to register webview with graphics process: {error}");
            }
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
        info!(
            "[nav] navigate request source={} url={} target={:?} new_child={} new_traversable={}",
            request.source_navigable_id,
            request.destination_url,
            request.target,
            request.new_child_navigable.is_some(),
            request.new_traversable_info.is_some(),
        );
        let result: Result<(), String> = (|| {
            let is_window_open = request.features_json.is_some();

            // Phase 1 — navigable creation, before navigation.
            // "create a new child navigable" / "create a new top-level
            // traversable" are separate spec steps that run on the HTML event
            // loop (the content process) before the navigate step; in this
            // architecture their navigable-owning steps run here, in the user
            // agent, ahead of the navigation that follows.  The content
            // process already created the document, realm, Window and
            // environment settings object (steps 10, 13, 15, 22 of "creating a
            // new browsing context and document"), and the methods below catch
            // up the browsing context, group membership, document state and
            // session history — internally via the UA-side
            // `create_a_new_browsing_context` — so that by the time
            // `self.navigate` runs below, the navigable and its browsing
            // context fully exist.
            let (navigable_id, window_type) =
                // ---- Child navigable creation (iframe) ----
                if let Some(ref child_info) = request.new_child_navigable {
                    let child_navigable_id = child_info.content_navigable_id;
                    self.create_a_new_child_navigable(
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

        // Step 2: "Let permissionsPolicy be the result of creating a permissions policy from a
        // response given navigationParams's navigable's container, navigationParams's origin,
        // and navigationParams's response."
        // Step 3: "Let creationURL be navigationParams's response's URL."
        // Step 4: "If navigationParams's request is non-null, then set creationURL to
        // navigationParams's request's current URL."
        // Step 5: "Let window be null."
        // Note: Steps 2-5 are not implemented and the state they feed (permissions policy,
        // creation URL) is document-owning: it is set up by the content process's
        // `initialise_the_document_object` when it handles the CreateLoadedDocument command.

        // Step 6: "If browsingContext's active document's is initial about:blank is true, and
        // browsingContext's active document's origin is same origin-domain with
        // navigationParams's origin, then set window to browsingContext's active window."
        // Note: The process-placement consequence of step 6 is approximated here: a
        // top-level traversable whose active document is initial about:blank stays on its
        // current event loop for the first navigation, so the new document lands in the same
        // content process as the initial about:blank document, where the window reuse itself
        // is implemented in `ContentProcess::initialise_the_document_object` (the initial
        // about:blank realm/Window is re-pointed at the new document when the destination is
        // same-origin with the initial about:blank's origin).  A cross-origin destination
        // also keeps the same event loop in this branch — `swapped_group` is not consulted —
        // so the spec's step-7 new-agent branch is not run for the initial-about:blank
        // top-level case; the content-side origin check then falls through to a fresh realm
        // there.
        let needs_new_event_loop = if let Some(parent_id) = parent_traversable_id {
            // Step 7: "Otherwise:" — the active document is not initial about:blank or is not
            // same-origin-domain with the navigation origin, so a new agent is required.
            // Note: Child navigables realize step 7 as a cross-origin comparison between the
            // parent document and the new document.  The child's initial about:blank document
            // inherits the parent's origin, so the parent-document URL approximates step 6's
            // same-origin-domain condition; whether the child's own active document is
            // initial about:blank is not consulted.  A cross-origin child navigation moves
            // the traversable to a new agent, realized here as a new content process/event
            // loop.
            let parent_document_url = self
                .state
                .active_documents_by_traversable
                .get(&parent_id)
                .and_then(|doc_id| self.state.documents.get(doc_id))
                .map(|doc| doc.url.clone())
                .ok_or_else(|| format!("missing parent document for traversable {parent_id}"))?;
            if parent_document_url == "about:blank" {
                // Note: A parent whose active document is still the initial about:blank has no
                // origin to compare the destination against; such a child navigation is
                // rejected instead of approximated.
                return Err(format!(
                    "unexpected initial about:blank parent while initialising child traversable {traversable_id}"
                ));
            }
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

        // Step 7.1: "Let oacHeader be the result of getting a structured field value given
        // `Origin-Agent-Cluster` and "item" from navigationParams's response's header list."
        // Step 7.2: "Let requestsOAC be true if oacHeader is not null and oacHeader[0] is the
        // boolean true; otherwise false."
        // Step 7.3: "If navigationParams's reserved environment is a non-secure context, then
        // set requestsOAC to false."
        // Note: Steps 7.1-7.3 are not implemented: Origin-Agent-Cluster is not tracked.
        // Step 7.4: "Let agent be the result of obtaining a similar-origin window agent given
        // navigationParams's origin, browsingContext's group, and requestsOAC."
        // Note: Runs here: obtaining the agent creates the agent cluster (a
        // content process) when the traversable's browsing context group has
        // no cluster for the destination site yet, or reuses the group's
        // existing cluster for that site, then reassigns the traversable to
        // the agent's event loop before `CreateLoadedDocument` is dispatched.
        // Steps 7.5-7.10 (realm, Window and environment settings object) run
        // in the content process
        // (`ContentProcess::initialise_the_document_object`).
        let destination_site = content_process_label_from_url(final_url);
        let process_label = destination_site.clone();
        let browsing_context_group_id = {
            let top_level_browsing_context_id = self
                .state
                .top_level_browsing_context_id(browsing_context_selection.browsing_context_id)
                .unwrap_or(browsing_context_selection.browsing_context_id);
            self.state
                .top_level_browsing_context_group_ids
                .get(&top_level_browsing_context_id)
                .copied()
                .ok_or_else(|| {
                    format!(
                        "missing browsing context group for browsing context {}",
                        browsing_context_selection.browsing_context_id
                    )
                })?
        };
        let old_event_loop_id = self.state.traversable_handles.get(&traversable_id).copied();
        let new_event_loop_id = self.obtain_similar_origin_window_agent(
            &destination_site,
            browsing_context_group_id,
            process_label,
        )?;
        let mut old_event_loop_to_stop = None;
        if let Some(old_event_loop_id) = old_event_loop_id {
            if let Some(old_agent) = window_agent_mut(&mut self.state, old_event_loop_id) {
                old_agent.traversable_ids.remove(&traversable_id);
            }
            if old_event_loop_id != new_event_loop_id
                && window_agent(&self.state, old_event_loop_id)
                    .is_some_and(|agent| agent.traversable_ids.is_empty())
            {
                old_event_loop_to_stop = Some(old_event_loop_id);
            }
        }
        if let Some(new_agent) = window_agent_mut(&mut self.state, new_event_loop_id) {
            new_agent.traversable_ids.insert(traversable_id);
        }
        self.state
            .traversable_handles
            .insert(traversable_id, new_event_loop_id);
        if let Some(navigable) = self.state.navigables.get_mut(&traversable_id) {
            navigable.event_loop_id = Some(new_event_loop_id);
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
            self.stop_event_loop_handle(old_event_loop_id);
        }
        // Step 8: "Let loadTimingInfo be a new document load timing info with its navigation
        // start time set to navigationParams's response's timing info's start time."
        // Step 9: "Let document be a new Document, with: type type; content type contentType;
        // origin navigationParams's origin; browsing context browsingContext; ..."
        // Step 10: "Set window's associated Document to document."
        // Step 11: "Set document's internal ancestor origin objects list ..."
        // Step 12: "Set document's ancestor origins list ..."
        // Step 13: "Run CSP initialization for a Document given document."
        // Step 14: "If navigationParams's request is non-null: ... Set document's referrer ..."
        // Step 15: "If navigationParams's fetch controller is not null: ... Create the
        // navigation timing entry ..."
        // Step 16: "Create the navigation timing entry for document ..."
        // Step 17: "If navigationParams's response has a `Refresh` header: ..."
        // Step 18: "If navigationParams's commit early hints is not null, then call
        // navigationParams's commit early hints with document."
        // Step 19: "Process link headers given document, navigationParams's response, and
        // "pre-media"."
        // Step 20: "If navigationParams's navigable is a top-level traversable, then process
        // the `Speculation-Rules` header given document and navigationParams's response."
        // Step 21: "Potentially free deferred fetch quota for document."
        // Step 22: "Return document."
        // Note: Steps 8-22 run in the content process
        // (`ContentProcess::initialise_the_document_object`) when it handles the
        // CreateLoadedDocument command; steps 8 and 11-21 are not implemented there either.
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
                    self.tla_tracer,
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
                self.report_navigation_completed(
                    pending.navigable_id,
                    NavigationCompletion::Aborted {
                        message: String::from("navigation was canceled by beforeunload"),
                    },
                )
            } else {
                self.continue_navigation_after_before_unload(pending)
            }
        } else {
            Ok(())
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled>
    fn handle_complete_before_unload(&mut self, result: BeforeUnloadResult) {
        info!(
            "[nav] beforeunload completed document={} check={} canceled={}",
            result.document_id, result.check_id, result.canceled
        );
        if let Err(error) = self.handle_complete_before_unload_result(result) {
            error!("failed to complete beforeunload: {error}");
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation>
    fn finalize_cross_document_navigation(
        &mut self,
        finalized: ContentFinalizeNavigation,
    ) -> Result<(), String> {
        info!(
            "[nav] finalize cross-document navigation document={} url={}",
            finalized.document_id, finalized.url
        );
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
            self.tla_tracer,
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
        self.note_rendering_opportunity(pending.traversable_id);
        // Notify the graphics process that a top-level navigation finalized.
        // This sets replace_root_on_next_paint so the next PaintFrame replaces
        // the old about:blank scene with the new page's scene.
        if let Some(graphics_sender) = &self.graphics_extension_sender
            && let Err(error) = graphics_sender.send(
                ipc_messages::graphics::GraphicsCommand::NavigationFinalized {
                    webview_id: WebviewId(pending.traversable_id),
                },
            )
        {
            log::error!("failed to notify graphics of navigation finalization: {error}");
        }
        let notify_result = self.report_navigation_completed(
            pending.traversable_id,
            NavigationCompletion::Committed {
                url: finalized.url.clone(),
            },
        );

        if let Some(previous_document_id) = pending.previous_document_id
            && previous_document_id != finalized.document_id
        {
            // The old document is destroyed after the new document commits so stale
            // content-side traffic cannot revive it after the traversable has advanced.
            // It is destroyed on the event loop that owns it: after a cross-process
            // navigation the traversable has moved to a new event loop, and routing by
            // traversable would send the destroy to the wrong content process.
            let command_sender = self
                .command_sender_for_document(previous_document_id)
                .or_else(|_| self.command_sender_for_traversable(pending.traversable_id));
            if let Ok(command_sender) = command_sender
                && let Err(error) = self.send_event_loop_command(
                    &command_sender,
                    ContentCommand::DestroyDocument {
                        document_id: previous_document_id,
                    },
                )
            {
                error!("[user-agent] failed to destroy previous document: {error}");
            }
            self.state.documents.remove(&previous_document_id);
        }

        if let Some(new_browsing_context_id) = pending.browsing_context_id {
            let is_top_level = self
                .state
                .navigables
                .get(&pending.traversable_id)
                .is_some_and(|navigable| navigable.parent_navigable_id.is_none());
            if is_top_level
                && let Some(previous_browsing_context_id) = previous_browsing_context_id
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
            Some(event_loop_id) => match window_agent_mut(&mut self.state, event_loop_id) {
                Some(agent) => {
                    let request_id = self.next_automation_request_id;
                    self.next_automation_request_id =
                        self.next_automation_request_id.wrapping_add(1);
                    agent.event_loop.script_waiters.insert(request_id, reply);
                    agent
                        .event_loop
                        .command_sender
                        .send(ContentCommand::EvaluateScript {
                            traversable_id,
                            request_id,
                            source,
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

        // Note a rendering opportunity so the content renders the state
        // produced by the script (DOM mutations, React commits, ...). The
        // EvaluateScript command was queued ahead of the update-the-rendering
        // on the same event loop, so the render runs after the script.
        // Automation script evaluation is a NoteRenderingOpportunity per the
        // TLA spec, mirroring handle_click_element.
        self.note_rendering_opportunity(traversable_id);
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
            Some(event_loop_id) => match window_agent_mut(&mut self.state, event_loop_id) {
                Some(agent) => {
                    let request_id = self.next_automation_request_id;
                    self.next_automation_request_id =
                        self.next_automation_request_id.wrapping_add(1);
                    agent.event_loop.click_waiters.insert(request_id, reply);
                    agent
                        .event_loop
                        .command_sender
                        .send(ContentCommand::ClickElement {
                            traversable_id,
                            request_id,
                            selector,
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

        // Note a rendering opportunity so the content renders the clicked state.
        // Automation click is a NoteRenderingOpportunity per the TLA spec.
        self.note_rendering_opportunity(traversable_id);
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

    /// Send a per-traversable viewport update to the owning event loop.
    ///
    /// Returns `true` when the update was delivered to a registered
    /// `traversable_handle` (callers use this to decide whether a computed
    /// child viewport was reliably received); `false` when the traversable's
    /// content process has not registered a handle yet. The viewport is still
    /// recorded in `traversable_viewports` in both cases so the event-loop
    /// migration path can re-send it once the handle registers.
    fn handle_set_traversable_viewport(
        &mut self,
        traversable_id: NavigableId,
        snapshot: (u32, u32, f32, ColorScheme),
        offset_x: f32,
        offset_y: f32,
    ) -> bool {
        self.state
            .traversable_viewports
            .insert(traversable_id, (snapshot, offset_x, offset_y));

        let Some(handle) = self.state.traversable_handles.get(&traversable_id).copied() else {
            return false;
        };
        let Some(agent) = window_agent(&self.state, handle) else {
            return false;
        };
        let command = traversable_viewport_command(traversable_id, snapshot, offset_x, offset_y);
        if let Err(error) = agent.event_loop.command_sender.send(command) {
            error!("failed to send traversable viewport to event loop {handle}: {error}");
        }
        // The UA notes a rendering opportunity so the content process will
        // receive UpdateTheRendering and repaint with the new viewport.
        self.note_rendering_opportunity(traversable_id);
        true
    }

    /// Report a navigation completion to the host for a top-level
    /// traversable only; child navigable (iframe) completions are not
    /// reported.
    fn report_navigation_completed(
        &self,
        navigable_id: NavigableId,
        status: NavigationCompletion,
    ) -> Result<(), String> {
        let is_top_level = self
            .state
            .navigables
            .get(&navigable_id)
            .is_some_and(|navigable| navigable.parent_navigable_id.is_none());
        if !is_top_level {
            return Ok(());
        }
        self.host.navigation_completed(NavigationCompleted {
            webview_id: WebviewId(navigable_id),
            status,
        })
    }

    /// The embedder is about to paint a frame for the top-level traversable
    /// `webview_id`. Update the rendering is queued when a frame is needed
    /// AND a rendering opportunity was noted; the flag stays set until then.
    fn handle_frame_needed(&mut self, webview_id: WebviewId) {
        let navigable_id = webview_id.0;
        verification::tla_log!(
            self.tla_tracer,
            -> "RenderingOpportunity",
            "FrameNeeded",
            navigable_id
        );
        self.frame_needed.insert(navigable_id);
        info!(
            "[render-pipe] UA frame needed navigable={} pending={:?} queued={:?}",
            navigable_id, self.pending_update_the_rendering, self.queued_rendering_opportunities
        );
        self.queue_update_the_rendering_for_navigables(navigable_id);
    }

    /// True when the embedder needs a frame for the top-level traversable
    /// containing `navigable_id` (child navigables render only when the
    /// traversable is painted, since their frames are composed into its
    /// texture).
    fn traversable_frame_needed(&self, navigable_id: NavigableId) -> bool {
        self.state
            .top_level_traversable_id(navigable_id)
            .is_some_and(|traversable_id| self.frame_needed.contains(&traversable_id))
    }

    /// Queue update the rendering for every navigable of
    /// `traversable_id` that has a batched rendering opportunity.
    fn queue_update_the_rendering_for_navigables(&mut self, traversable_id: NavigableId) {
        // Drain each candidate independently. A candidate that is already
        // pending is a no-op (the per-navigable guard in
        // `queue_update_the_rendering` handles it), so a child iframe's queued
        // opportunity is drained even when the top-level traversable's own
        // update is still in flight. The old batch-level early return on
        // `pending_update_the_rendering` left a static child's change stranded
        // in `queued_rendering_opportunities` until an unrelated input event
        // happened to request a frame.
        for candidate in self
            .queued_rendering_opportunities
            .keys()
            .copied()
            .collect::<Vec<_>>()
        {
            if self.state.top_level_traversable_id(candidate) == Some(traversable_id) {
                self.queue_update_the_rendering(candidate);
            }
        }
    }

    /// Track which frame was last focused via pointer-down, for routing
    /// non-positional events (keyboard, IME).
    fn update_focused_frame(
        &mut self,
        root_webview_id: WebviewId,
        event: &blitz_traits::events::UiEvent,
    ) {
        if !matches!(event, blitz_traits::events::UiEvent::PointerDown(_)) {
            return;
        }
        let Some((coords_x, coords_y)) = pointer_coords(event) else {
            return;
        };
        let viewport_scale = self.host.viewport_scale_factor().max(1.0);
        let phys_x = coords_x * viewport_scale as f64;
        let phys_y = coords_y * viewport_scale as f64;
        let Some(hit_info_list) = self.state.frame_hit_info.get(&root_webview_id) else {
            return;
        };
        for info in hit_info_list.iter().rev() {
            if phys_x >= info.root_clip_bounds[0]
                && phys_y >= info.root_clip_bounds[1]
                && phys_x <= info.root_clip_bounds[2]
                && phys_y <= info.root_clip_bounds[3]
            {
                self.state
                    .focused_frame_id
                    .insert(root_webview_id, Some(info.frame_id));
                return;
            }
        }
        self.state.focused_frame_id.insert(root_webview_id, None);
    }

    /// queuing DOM event dispatch on the traversable's owning
    /// <https://html.spec.whatwg.org/multipage/#event-loop>.
    fn handle_send_ui_event(&mut self, webview_id: WebviewId, event_message: Vec<u8>) {
        if input_debug_enabled() {
            trace!(
                "[input-debug][user-agent] send_ui_event webview={:?} bytes={}",
                webview_id,
                event_message.len(),
            );
        }

        let Ok(event) = crate::ui_event::deserialize_ui_event(&event_message) else {
            return;
        };

        // Track which frame is focused based on pointer-down events.
        self.update_focused_frame(webview_id, &event);

        let (target_webview_id, routed_event, composed_frame_ids) =
            self.route_ui_event(webview_id, event.clone());
        debug!(
            "[input-debug][user-agent] send_ui_event webview={} routed={} composed_frames={:?} child_map={:?}",
            webview_id.0,
            target_webview_id.0,
            composed_frame_ids
                .iter()
                .map(|frame_id| frame_id.0)
                .collect::<Vec<_>>(),
            self.state
                .child_frame_to_webview
                .get(&webview_id)
                .map(|m| m.iter().map(|(k, v)| (k.0, v.0)).collect::<Vec<_>>()),
        );

        if let Ok(routed_message) = crate::ui_event::serialize_ui_event(&routed_event) {
            self.handle_dispatch_event_for(target_webview_id.0, routed_message);
        }

        // Note a rendering opportunity for every frame involved in this
        // composition (from hit-testing), not just the hit-tested target.
        // This ensures child frames (iframes) get their scene rendered and
        // composed together with the root.  When no hit-test info exists yet
        // (first render), fall back to noting for just the target.
        if composed_frame_ids.is_empty() {
            self.note_rendering_opportunity(target_webview_id.0);
        } else {
            let navigable_ids: Vec<NavigableId> = {
                let child_frame_map = self.state.child_frame_to_webview.get(&webview_id);
                composed_frame_ids
                    .iter()
                    .map(|frame_id| {
                        child_frame_map
                            .and_then(|map| map.get(frame_id))
                            .copied()
                            .unwrap_or(webview_id)
                            .0
                    })
                    .collect()
            };
            for navigable_id in navigable_ids {
                self.note_rendering_opportunity(navigable_id);
            }
        }
    }

    /// Route a UI event to the correct frame using frame hit info from the
    /// graphics process. Returns (target_webview, routed_event, composed_frame_ids)
    /// where composed_frame_ids lists all frames that need rendering opportunities.
    ///
    /// The frame_hit_info root_clip_bounds are in physical/device pixels (matching
    /// the compositor's internal coordinate space), but event coordinates from the
    /// embedder are in logical/CSS pixels. We multiply by viewport_scale before hit
    /// testing, matching the old WebviewProvider::route_ui_event which did the same.
    fn route_ui_event(
        &self,
        root_webview_id: WebviewId,
        event: blitz_traits::events::UiEvent,
    ) -> (WebviewId, blitz_traits::events::UiEvent, Vec<FrameId>) {
        let viewport_scale = self.host.viewport_scale_factor().max(1.0);

        // For non-positional events (keyboard, IME), route to the focused frame
        // if one exists from a previous pointer-down event.
        let Some((coords_x, coords_y)) = pointer_coords(&event) else {
            // Non-positional event: route via focused frame if one exists.
            let ids: Vec<FrameId> = self
                .state
                .frame_hit_info
                .get(&root_webview_id)
                .map(|list| list.iter().map(|info| info.frame_id).collect())
                .unwrap_or_default();

            if let Some(Some(focused_frame_id)) = self.state.focused_frame_id.get(&root_webview_id)
                && let Some(map) = self.state.child_frame_to_webview.get(&root_webview_id)
                && let Some(&focused_wv) = map.get(focused_frame_id)
            {
                return (focused_wv, event, ids);
            }
            return (root_webview_id, event, ids);
        };

        let Some(hit_info_list) = self.state.frame_hit_info.get(&root_webview_id) else {
            return (root_webview_id, event, Vec::new());
        };

        // Convert logical event coords to physical for hit testing against
        // root_clip_bounds (which are in physical pixels from the compositor).
        let phys_x = coords_x * viewport_scale as f64;
        let phys_y = coords_y * viewport_scale as f64;

        // Collect all frame IDs for rendering opportunities.
        let composed_frame_ids: Vec<FrameId> =
            hit_info_list.iter().map(|info| info.frame_id).collect();

        // Find the deepest frame that contains the pointer, in physical coords.
        for info in hit_info_list.iter().rev() {
            if phys_x >= info.root_clip_bounds[0]
                && phys_y >= info.root_clip_bounds[1]
                && phys_x <= info.root_clip_bounds[2]
                && phys_y <= info.root_clip_bounds[3]
            {
                // Compute position within this frame in physical pixels, matching
                // the old compositor's hit_test local_x/y.
                let local_phys_x = phys_x - info.root_clip_bounds[0];
                let local_phys_y = phys_y - info.root_clip_bounds[1];

                // Match old retarget_ui_event_for_hit which computed:
                //   routed_client_x = (viewport.offset_x + hit.local_x) / viewport_scale
                // where viewport.offset_x = root_clip_bounds.x0 (physical pixels).
                // This gives the root-space CSS position including iframe offset.
                let routed_css_x =
                    ((info.root_clip_bounds[0] + local_phys_x) / viewport_scale as f64) as f32;
                let routed_css_y =
                    ((info.root_clip_bounds[1] + local_phys_y) / viewport_scale as f64) as f32;

                let routed_event = set_event_local_coords(&event, routed_css_x, routed_css_y);

                // Route to child webview if this frame belongs to an iframe.
                if let Some(map) = self.state.child_frame_to_webview.get(&root_webview_id)
                    && let Some(&child_wv) = map.get(&info.frame_id)
                {
                    return (child_wv, routed_event, composed_frame_ids);
                }

                return (root_webview_id, routed_event, composed_frame_ids);
            }
        }

        // No frame matched; pass through unchanged.
        (root_webview_id, event, composed_frame_ids)
    }

    fn handle_dispatch_event_for(&mut self, traversable_id: NavigableId, event: Vec<u8>) {
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
        let Some(agent) = window_agent(&self.state, handle) else {
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
        if let Err(error) = agent.event_loop.command_sender.send(command) {
            error!("failed to send dispatch-event to event loop {handle}: {error}");
        }
    }

    /// <https://html.spec.whatwg.org/#window-post-message-steps> step 8:
    /// queue a global task on the posted message task source given targetWindow
    /// by routing the message to the target window's event loop, even when the
    /// target window lives in the same event loop as the source (no
    /// same-process optimization at this stage).
    fn handle_post_message(&mut self, request: PostMessageRequest) {
        let Ok(command_sender) = self.command_sender_for_traversable(request.target_navigable_id)
        else {
            error!(
                "postMessage: no event loop for target navigable {}",
                request.target_navigable_id
            );
            return;
        };
        if let Err(error) = command_sender.send(ContentCommand::PostMessage(request)) {
            error!("postMessage: failed to queue message task: {error}");
        }
    }

    /// The user-agent half of the port workflow: register channels and
    /// transfers and process the routing queue (the model's `NewChannel`,
    /// `Transfer`, `TransferReceive`, and `RouteMessage` actions).  A task
    /// routed to a port's owning event loop is delivered over that event
    /// loop's own user-agent channel: the similar-origin window agent's
    /// channel (its content process's command channel) for the window event
    /// loop, or the dedicated worker agent's own command channel (held in
    /// `DedicatedWorkerAgent::event_loop`) for a worker event loop.
    fn handle_port_event(&mut self, event: crate::channel_messaging::PortEvent) {
        let state = &self.state;
        let mut send_task = |event_loop_id: EventLoopId, command: ContentCommand| {
            let Some(command_sender) = event_loop_command_sender(state, event_loop_id) else {
                error!("port routing: missing event loop {event_loop_id}");
                return;
            };
            if let Err(error) = command_sender.send(command) {
                error!("port routing: failed to queue task: {error}");
            }
        };
        crate::channel_messaging::handle_port_event(
            &mut self.channel_messaging,
            event,
            &mut send_task,
        );
    }

    /// Milliseconds since the Unix epoch of `instant`, measured on the
    /// monotonic clock shared with the content processes.
    fn epoch_millis(&self, instant: Instant) -> f64 {
        self.epoch_anchor_wall_ms
            + instant
                .saturating_duration_since(self.epoch_anchor)
                .as_secs_f64()
                * 1000.0
    }

    /// <https://html.spec.whatwg.org/#rendering-opportunity>
    /// Update the rendering is queued only when the embedder needs a frame
    /// (FrameNeeded) AND there is something to render.
    /// Otherwise the opportunity is batched and an embedder redraw is
    /// requested, so the next paint sends FrameNeeded and queues it.
    fn note_rendering_opportunity(&mut self, navigable_id: NavigableId) {
        verification::tla_log!(
            self.tla_tracer,
            -> "RenderingOpportunity",
            "NoteRenderingOpportunity",
            navigable_id
        );
        // Stamp the opportunity time once; the queued update-the-rendering
        // command carries it as the event loop's "last render opportunity
        // time" (HTML Step 1 of update-the-rendering).
        let frame_timestamp_epoch_ms = self.epoch_millis(Instant::now());
        if self.pending_update_the_rendering.contains(&navigable_id) {
            self.queued_rendering_opportunities
                .insert(navigable_id, frame_timestamp_epoch_ms);
            info!(
                "[render-pipe] UA queue navigable={} (already pending, queued)",
                navigable_id
            );
            return;
        }
        if !self.traversable_frame_needed(navigable_id) {
            self.queued_rendering_opportunities
                .insert(navigable_id, frame_timestamp_epoch_ms);
            info!(
                "[render-pipe] UA queue navigable={} (no frame needed, queued)",
                navigable_id
            );
            // Ask the embedder to paint; the paint sends FrameNeeded, which
            // queues the batched opportunity's update.
            if let Some(traversable_id) = self.state.top_level_traversable_id(navigable_id) {
                self.host.request_redraw(WebviewId(traversable_id));
            }
            return;
        }
        self.queued_rendering_opportunities
            .insert(navigable_id, frame_timestamp_epoch_ms);
        self.queue_update_the_rendering(navigable_id);
    }

    /// Queue update the rendering for `navigable_id`: mark it in flight,
    /// drain its batched opportunities, clear the top-level traversable's
    /// frame-needed flag, and command the content process. A child also
    /// queues the top-level traversable's update, since the graphics
    /// process composes only when the top-level frame arrives.
    fn queue_update_the_rendering(&mut self, navigable_id: NavigableId) {
        if self.pending_update_the_rendering.contains(&navigable_id) {
            return;
        }
        self.pending_update_the_rendering.insert(navigable_id);
        // Drain the opportunity stamp; the latest note for this navigable
        // wins (the event loop's "last render opportunity time").
        let frame_timestamp_epoch_ms = self
            .queued_rendering_opportunities
            .remove(&navigable_id)
            .unwrap_or_else(|| self.epoch_millis(Instant::now()));
        if let Some(traversable_id) = self.state.top_level_traversable_id(navigable_id) {
            self.frame_needed.remove(&traversable_id);
        }
        info!(
            "[render-pipe] UA queue update the rendering navigable={} pending={:?} queued={:?}",
            navigable_id, self.pending_update_the_rendering, self.queued_rendering_opportunities
        );

        let Some(handle) = self.state.traversable_handles.get(&navigable_id).copied() else {
            info!(
                "[render-pipe] UA note_rendering_opportunity: no handle for navigable={}",
                navigable_id
            );
            self.pending_update_the_rendering.remove(&navigable_id);
            return;
        };
        let Some(document_id) = self
            .state
            .active_documents_by_traversable
            .get(&navigable_id)
        else {
            info!(
                "[render-pipe] UA note_rendering_opportunity: no active document for navigable={}",
                navigable_id
            );
            self.pending_update_the_rendering.remove(&navigable_id);
            return;
        };
        // A top-level traversable moves to a new event loop (a fresh content
        // process) when a cross-origin navigation is prepared, while the
        // UA-side active document only switches at finalization. In that
        // window the active document is owned by the old event loop;
        // commanding update-the-rendering for it on the new process fails
        // there with "unknown document id", and since no paint frame is
        // produced the pending flag is never cleared — the navigable stops
        // rendering permanently. Skip the update when the active document is
        // not owned by the navigable's current event loop; the finalization's
        // rendering-opportunity note resumes rendering with the committed
        // document.
        let document_owned_by_target_event_loop = self
            .state
            .documents
            .get(document_id)
            .is_some_and(|document| document.event_loop_id == handle);
        if !document_owned_by_target_event_loop {
            info!(
                "[render-pipe] UA note_rendering_opportunity: active document {} not owned by event loop {} for navigable={}",
                document_id, handle, navigable_id
            );
            self.pending_update_the_rendering.remove(&navigable_id);
            return;
        }
        let Some(agent) = window_agent(&self.state, handle) else {
            info!(
                "[render-pipe] UA note_rendering_opportunity: no agent for event loop handle={}",
                handle
            );
            self.pending_update_the_rendering.remove(&navigable_id);
            return;
        };

        if input_debug_enabled() {
            trace!(
                "[input-debug][user-agent] note_rendering_opportunity navigable={} event_loop={} document={}",
                navigable_id, handle, document_id,
            );
        }

        let command = ContentCommand::UpdateTheRendering {
            traversable_id: navigable_id,
            document_id: *document_id,
            frame_timestamp_epoch_ms,
        };
        if let Err(error) = agent.event_loop.command_sender.send(command) {
            error!(
                "[render-pipe] failed to send update-the-rendering to event loop {handle}: {error}"
            );
        }

        // When a child navigable queues update the rendering, also queue
        // the top-level traversable so the graphics process composes the
        // child's frame into the scene. The compositor only composes when
        // the top-level frame arrives; child PaintFrames are stored but
        // never trigger composition.
        if let Some(traversable_id) = self.state.top_level_traversable_id(navigable_id)
            && traversable_id != navigable_id
        {
            self.queue_update_the_rendering(traversable_id);
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#attempt-to-populate-the-history-entry's-document>
    fn handle_navigation_fetch_completed(
        &mut self,
        fetch_id: NavigationFetchId,
        response: ContentFetchResponse,
    ) {
        info!(
            "[nav] navigation fetch completed fetch={} url={} status={}",
            fetch_id, response.final_url, response.status
        );
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
                    if let Err(error) = self.report_navigation_completed(
                        pending.traversable_id,
                        NavigationCompletion::Aborted { message: error },
                    ) {
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
                if let Err(error) = self.report_navigation_completed(
                    pending.traversable_id,
                    NavigationCompletion::Aborted { message: error },
                ) {
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
                if let Err(error) = self.report_navigation_completed(
                    pending.traversable_id,
                    NavigationCompletion::Aborted { message: error },
                ) {
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
        if let Err(error) = self.report_navigation_completed(
            pending.traversable_id,
            NavigationCompletion::Aborted {
                message: format!("navigation fetch failed for {}", pending.request.url),
            },
        ) {
            error!("[user-agent] failed to report navigation completed (fetch failed): {error}");
        }
    }

    /// removing a child-navigable mapping and stopping any synthetic
    /// traversable that represented that iframe in the user-agent registry.
    fn handle_iframe_traversable_removed(
        &mut self,
        parent_traversable_id: NavigableId,
        content_navigable_id: NavigableId,
        content_frame_id: FrameId,
    ) {
        info!(
            "[nav] iframe traversable removed parent={} child={} frame={}",
            parent_traversable_id, content_navigable_id, content_frame_id.0
        );
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
                if let Some(agent) = window_agent_mut(&mut self.state, event_loop_id) {
                    agent.traversable_ids.remove(traversable_id);
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
            self.pending_update_the_rendering.remove(traversable_id);
            self.queued_rendering_opportunities.remove(traversable_id);
        }

        // Release the graphics-process state for the removed webviews
        // (compositor, GPU buffers, shared surfaces), symmetric with the
        // RegisterWebview sent when the child navigable was created.
        if let Some(graphics_sender) = &self.graphics_extension_sender {
            for traversable_id in &traversable_ids {
                if let Err(error) = graphics_sender.send(
                    ipc_messages::graphics::GraphicsCommand::UnregisterWebview {
                        webview_id: WebviewId(*traversable_id),
                    },
                ) {
                    error!("failed to unregister webview with graphics process: {error}");
                }
            }
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

        for event_loop_id in event_loops_to_maybe_stop {
            let should_stop = window_agent(&self.state, event_loop_id)
                .is_some_and(|agent| agent.traversable_ids.is_empty());
            if !should_stop {
                continue;
            }
            self.stop_event_loop_handle(event_loop_id);
        }
    }

    /// shutting down the user-agent thread and every worker it owns.
    fn handle_shutdown(&mut self, reply: Sender<Result<(), String>>) {
        // The window agents' records own their content processes; the
        // dedicated worker agents of the workers those processes hosted are
        // drained with them — their threads die with their content
        // processes, which are shut down below.
        let entries = self
            .state
            .agents
            .drain()
            .filter_map(|(_, agent)| match agent {
                Agent::Window(window_agent) => Some(window_agent),
                Agent::DedicatedWorker(_) => None,
            })
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

        for mut entry in entries {
            entry.event_loop.shutdown();
        }

        // Shut down the graphics process: send Shutdown, wait for
        // ShutdownComplete, then join the child process.
        if let Some(sender) = &self.graphics_extension_sender
            && let Err(error) = sender.send(ipc_messages::graphics::GraphicsCommand::Shutdown)
        {
            log::error!("failed to send shutdown to graphics process: {error}");
        }
        // Drain events until ShutdownComplete arrives.
        while let Ok(incoming) = self.graphics_event_receiver.recv() {
            if matches!(
                incoming.payload,
                ipc_messages::graphics::GraphicsEvent::ShutdownComplete
            ) {
                break;
            }
        }
        if let Some(mut child) = self.graphics_child.take()
            && let Err(error) = child.wait()
        {
            log::error!("failed to wait for graphics process exit: {error}");
        }

        self.net_connection.shutdown();

        let _ = reply.send(Ok(()));
    }

    /// Handle a GraphicsEvent (composed scene) from the graphics process.
    fn handle_graphics_event(
        &mut self,
        incoming: &mut ipc::IpcIncoming<ipc_messages::graphics::GraphicsEvent>,
    ) {
        use ipc_messages::graphics::GraphicsEvent;
        match &incoming.payload {
            GraphicsEvent::PixelFrameReady {
                webview_id,
                layers,
                animating,
                animating_frame_ids,
                generation: _,
                frame_hit_info,
                child_viewports,
                child_frame_to_webview,
            } => {
                debug!(
                    "[graphics] received surface frame for {:?} ({} layers)",
                    webview_id,
                    layers.len()
                );
                // Forward every layer's topology plus the surface for the
                // layers re-rendered this cycle. The embedder keeps the
                // last surface of a clean layer (frame: None) and only draws
                // the layers it is told about.
                let layer_frames: Vec<ipc_messages::graphics::LayerFrame> = layers
                    .iter()
                    .map(|topology| {
                        let mut topology = topology.clone();
                        let frame = match topology.surface.take() {
                            Some(ipc_messages::graphics::SurfacePayload::CpuShmem {
                                shmem_key,
                            }) => {
                                let region = incoming
                                    .shmem_regions
                                    .remove(&shmem_key)
                                    .unwrap_or_else(|| ipc::IpcSharedRegion::from_bytes(&[]));
                                Some(ipc_messages::graphics::SurfaceFrame::CpuShmem(region))
                            }
                            #[cfg(target_os = "macos")]
                            Some(ipc_messages::graphics::SurfacePayload::SharedTexture {
                                texture_id,
                                surface_id,
                                port,
                            }) => Some(ipc_messages::graphics::SurfaceFrame::SharedTexture {
                                texture_id,
                                surface_id,
                                port,
                            }),
                            None => None,
                        };
                        ipc_messages::graphics::LayerFrame { topology, frame }
                    })
                    .collect();
                debug!(
                    "[graphics] forwarded {} layers for {:?}",
                    layer_frames.len(),
                    webview_id
                );

                // When the top-level traversable's composed scene completes,
                // all child frames included in the composition have also been
                // rendered and composed.  Clear their pending state so they
                // can receive new rendering opportunities.  Queued
                // opportunities are kept: they are requests for a new render
                // (e.g. a hover state change in the child) that were batched
                // while no frame was needed and must survive this
                // composition, or the child never repaints.
                for (_child_frame_id, child_wv) in child_frame_to_webview.iter() {
                    self.pending_update_the_rendering.remove(&child_wv.0);
                }

                // The frame was produced; the update for the top-level
                // traversable is complete. The texture is forwarded to the
                // embedder below.
                self.pending_update_the_rendering.remove(&webview_id.0);
                info!(
                    "[render-pipe] UA composed scene navigable={} animating={} pending_remaining={}",
                    webview_id.0,
                    animating,
                    self.pending_update_the_rendering.len()
                );
                // Animated content wants the next frame: queue an
                // opportunity for the top-level and every composing child
                // frame that is animating, so the next FrameNeeded (the
                // next paint) starts the next frame. Only the animating
                // frames are noted; static composing siblings are not
                // re-rendered.
                if *animating {
                    // All animating frames share one opportunity time, so
                    // their update-the-rendering commands carry the same
                    // event-loop "last render opportunity time".
                    let frame_timestamp_epoch_ms = self.epoch_millis(Instant::now());
                    self.queued_rendering_opportunities
                        .insert(webview_id.0, frame_timestamp_epoch_ms);
                    for frame_id in animating_frame_ids {
                        if let Some(child_wv) = child_frame_to_webview.get(frame_id) {
                            self.queued_rendering_opportunities
                                .insert(child_wv.0, frame_timestamp_epoch_ms);
                        }
                        // Frame ids not in the child map belong to the
                        // top-level frame itself, already noted above.
                    }
                }
                // If the embedder already needs the next frame (a
                // FrameNeeded arrived while this render was in flight),
                // start it now.
                if self.frame_needed.contains(&webview_id.0) {
                    self.queue_update_the_rendering_for_navigables(webview_id.0);
                } else if !*animating
                    && self.queued_rendering_opportunities.keys().any(|candidate| {
                        self.state.top_level_traversable_id(*candidate) == Some(webview_id.0)
                    })
                {
                    // A static content-only change (a DOM mutation from an
                    // input event or script that landed while the previous
                    // frame was in flight) is stranded in
                    // `queued_rendering_opportunities`: the scene is not
                    // animating, so nothing re-notes it and the display link
                    // is stopped. Without a FrameNeeded the queued opportunity
                    // is never drained, and the change only appears when the
                    // next unrelated input event happens to request a frame.
                    // Request a redraw so the next FrameNeeded drains it.
                    self.host.request_redraw(*webview_id);
                }
                self.state
                    .frame_hit_info
                    .insert(*webview_id, frame_hit_info.clone());
                self.state
                    .child_frame_to_webview
                    .insert(*webview_id, child_frame_to_webview.clone());
                if let Err(e) =
                    self.host
                        .new_web_content_layers(*webview_id, layer_frames, *animating)
                {
                    error!("[graphics] forward layers: {e}");
                }

                // Publish child viewports so child traversables know their
                // visible viewport dimensions (iframe size and position).
                for cv in child_viewports {
                    let child_traversable_id = cv.child_webview_id.0;
                    let cw = (cv.root_clip_bounds[2] - cv.root_clip_bounds[0]) as u32;
                    let ch = (cv.root_clip_bounds[3] - cv.root_clip_bounds[1]) as u32;
                    if let Some(&((_vw, _vh, scale, ref cs), _ox, _oy)) =
                        self.state.traversable_viewports.get(&webview_id.0)
                    {
                        let viewport_scale = scale.max(1.0);
                        let offset_x = (cv.root_clip_bounds[0] as f32) / viewport_scale;
                        let offset_y = (cv.root_clip_bounds[1] as f32) / viewport_scale;
                        let key = (cw.max(1), ch.max(1), offset_x, offset_y);
                        let child_wv = ipc_messages::content::WebviewId(child_traversable_id);
                        if self.state.published_child_viewports.get(&child_wv) == Some(&key) {
                            continue;
                        }
                        // Always record the viewport so the child's event-loop
                        // migration path can re-send it once its handle registers,
                        // but only mark it *published* when the push actually
                        // reached a registered traversable handle. Marking it
                        // published before delivery means a child whose content
                        // process had not registered yet (the common cross-origin
                        // iframe bootstrap) swallows the correct size forever: the
                        // key is stable for a static iframe box, so the size is
                        // never re-sent and the child paints at its fallback
                        // dimensions until something changes the key (a resize).
                        if self.handle_set_traversable_viewport(
                            child_traversable_id,
                            (cw.max(1), ch.max(1), scale, *cs),
                            offset_x,
                            offset_y,
                        ) {
                            self.state.published_child_viewports.insert(child_wv, key);
                        }
                    }
                }
            }
            GraphicsEvent::VideoEnded {
                webview_id,
                video_paint_id,
            } => {
                let event_loop_id = match self.state.traversable_handles.get(&webview_id.0) {
                    Some(event_loop_id) => *event_loop_id,
                    None => return,
                };
                let Some(agent) = window_agent(&self.state, event_loop_id) else {
                    return;
                };
                if let Err(error) =
                    agent
                        .event_loop
                        .command_sender
                        .send(ContentCommand::NotifyVideoEnded {
                            video_paint_id: *video_paint_id,
                        })
                {
                    error!("failed to send video-ended to event loop {event_loop_id}: {error}");
                }
            }
            GraphicsEvent::CompositionChanged { webview_id } => {
                // A cross-origin iframe's content changed (or a video frame
                // arrived) without the top-level content process driving a
                // render. Note a rendering opportunity for the traversable so
                // its render cycle re-composes and includes the latest
                // embedded frame. `note_rendering_opportunity` batches the
                // note and requests a redraw when the embedder is not already
                // painting, so a static parent gets one repaint per content
                // change instead of deferring to the next unrelated input.
                info!(
                    "[render-pipe] UA composition changed webview={:?}",
                    webview_id
                );
                self.note_rendering_opportunity(webview_id.0);
            }
            GraphicsEvent::ShutdownComplete => {
                debug!("[graphics] graphics process shutdown complete");
            }
        }
    }
}

/// Extract pointer coordinates from a UI event, if applicable.
fn pointer_coords(event: &blitz_traits::events::UiEvent) -> Option<(f64, f64)> {
    match event {
        blitz_traits::events::UiEvent::PointerMove(e)
        | blitz_traits::events::UiEvent::PointerUp(e)
        | blitz_traits::events::UiEvent::PointerDown(e) => {
            Some((f64::from(e.coords.client_x), f64::from(e.coords.client_y)))
        }
        blitz_traits::events::UiEvent::Wheel(e) => {
            Some((f64::from(e.coords.client_x), f64::from(e.coords.client_y)))
        }
        _ => None,
    }
}

/// Translate event coordinates by an offset (for hit-tested child frames).
/// Set event coordinates to a local frame position.
/// The embedder sends coordinates in root-viewport space; this converts
/// them to the target frame's local coordinate space by setting client
/// and page coordinates to the given local (x, y).
fn set_event_local_coords(
    event: &blitz_traits::events::UiEvent,
    local_x: f32,
    local_y: f32,
) -> blitz_traits::events::UiEvent {
    let mut routed = event.clone();
    match &mut routed {
        blitz_traits::events::UiEvent::PointerMove(e)
        | blitz_traits::events::UiEvent::PointerUp(e)
        | blitz_traits::events::UiEvent::PointerDown(e) => {
            e.coords.client_x = local_x;
            e.coords.client_y = local_y;
            e.coords.page_x = local_x;
            e.coords.page_y = local_y;
        }
        blitz_traits::events::UiEvent::Wheel(e) => {
            e.coords.client_x = local_x;
            e.coords.client_y = local_y;
            e.coords.page_x = local_x;
            e.coords.page_y = local_y;
        }
        _ => {}
    }
    routed
}
