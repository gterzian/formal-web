mod event_loop;
mod fetch;
mod id;
mod timer;

use blitz_traits::shell::ColorScheme;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use embedder::{FinalizeNavigation, FormalWebUserEvent, UserEventDispatcher};
use ipc_messages::{
    content::{
        BeforeUnloadResult, Command as ContentCommand, ContentNavigableId,
        DispatchEventEntry, DocumentFetchId,
        FetchRequest as ContentFetchRequest, FetchResponse as ContentFetchResponse,
        FinalizeNavigation as ContentFinalizeNavigation, FrameId, LoadedDocumentResponse,
        NavigableId, NavigateRequest, NavigationFetchId, NavigationId, UserNavigationInvolvement, WebviewId,
        WindowTimerKey, iframe_target_name, parse_iframe_target_name,
    },
};
use std::collections::{HashMap, HashSet};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use url::Url;

use crate::id::UserAgentIds;
use crate::event_loop::{
    EventLoopCommand, EventLoopEntry, spawn_event_loop_entry, stop_event_loop_entry,
    traversable_viewport_command,
};
use crate::fetch::{FetchCommand, run_fetch_thread};
use crate::timer::{TimerCommand, run_timer_thread};

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
    pub id: u64,
    /// <https://tc39.es/ecma262/#sec-agents>
    pub can_block: bool,
    /// <https://html.spec.whatwg.org/multipage/#concept-agent-event-loop>
    pub event_loop_id: usize,
}

/// <https://html.spec.whatwg.org/multipage/#agent-cluster-cross-origin-isolation>
#[derive(Clone, Debug)]
pub struct AgentCluster {
    /// identifier for <https://html.spec.whatwg.org/multipage/#agent-cluster>
    pub id: u64,
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
    pub id: u64,
}

/// <https://html.spec.whatwg.org/multipage/#browsing-context-group>
#[derive(Clone, Debug, Default)]
pub struct BrowsingContextGroup {
    /// identifier for <https://html.spec.whatwg.org/multipage/#browsing-context-group>
    pub id: u64,
    /// <https://html.spec.whatwg.org/multipage/#browsing-context-set>
    pub browsing_context_set: HashMap<u64, BrowsingContext>,
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
    pub members: HashMap<u64, BrowsingContextGroup>,
}

impl BrowsingContextGroupSet {
    /// allocating the next browser-global browsing-context-group id.
    fn next_group_id(&self) -> u64 {
        self.members.keys().copied().max().map_or(0, |group_id| group_id + 1)
    }

    /// removing one <https://html.spec.whatwg.org/multipage/#browsing-context>
    /// from the user agent's browsing-context-group set.
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

/// <https://html.spec.whatwg.org/multipage/#top-level-traversable>
#[derive(Clone, Debug)]
pub struct Traversable {
    /// Identifier for <https://html.spec.whatwg.org/multipage/#traversable-navigable>
    pub id: u64,
    /// <https://html.spec.whatwg.org/multipage/#nav-parent>
    pub parent_traversable_id: Option<u64>,
    /// Browser-UI flag selecting the active traversable.
    pub is_active: bool,
    /// <https://html.spec.whatwg.org/multipage/#document-state-nav-target-name>
    pub target_name: String,
    /// <https://html.spec.whatwg.org/multipage/#nav-bc>
    pub active_browsing_context_id: Option<u64>,
    /// Active document exposed by the current session history entry.
    pub active_document_id: Option<u64>,
    /// Owning event loop.
    pub event_loop_id: usize,
    /// Owning handle.
    pub handle: usize,
    /// <https://html.spec.whatwg.org/multipage/#ongoing-navigation>
    pub ongoing_navigation_id: Option<NavigationId>,
    /// Marker for deferred update-the-rendering work while navigation is still ongoing.
    pub has_deferred_update_the_rendering: bool,
    /// Compositor frame slot for child navigables; None for top-level traversables.
    pub frame_id: Option<FrameId>,
    /// <https://html.spec.whatwg.org/multipage/#tn-current-session-history-step>
    pub current_session_history_step: usize,
    /// <https://html.spec.whatwg.org/multipage/#tn-session-history-entries>
    pub session_history_entries: Vec<SessionHistoryEntry>,
}

/// <https://html.spec.whatwg.org/multipage/#navigable>
#[derive(Clone, Debug)]
pub struct Navigable {
    /// Identifier for <https://html.spec.whatwg.org/multipage/#navigable-id>.
    pub id: u64,
    /// <https://html.spec.whatwg.org/multipage/#nav-parent>
    pub parent_navigable_id: Option<u64>,
    /// Active document exposed by this navigable.
    pub active_document_id: Option<u64>,
    /// Present when this navigable also controls current/active session-history selection.
    pub traversable: Option<Traversable>,
}

/// <https://html.spec.whatwg.org/multipage/#session-history-entry>
#[derive(Clone, Debug)]
pub struct SessionHistoryEntry {
    /// <https://html.spec.whatwg.org/multipage/#she-step>
    pub step: usize,
    /// reference to <https://dom.spec.whatwg.org/#concept-document>
    pub document_id: u64,
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
            has_transient_activation: matches!(user_involvement, UserNavigationInvolvement::Activation),
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
    browsing_context_id: u64,
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

    /// translating the user-agent's navigation request model into the
    /// content-side fetch request transport. The handler_id in ContentFetchRequest is
    /// unused for navigation fetches; a placeholder DocumentFetchId is generated.
    fn to_content_fetch_request(&self) -> ContentFetchRequest {
        ContentFetchRequest {
            handler_id: DocumentFetchId::new(),
            url: self.url.clone(),
            method: self.method.clone(),
            body: self.body.clone().unwrap_or_default(),
        }
    }
}

/// <https://html.spec.whatwg.org/multipage/#top-level-traversable-set>
#[derive(Clone, Debug, Default)]
pub struct TraversableSet {
    /// <https://html.spec.whatwg.org/multipage/#top-level-traversable-set>
    pub members: HashMap<u64, Traversable>,
}

impl TraversableSet {
}

/// Top-level state for the Rust user-agent thread.
///
/// This mirrors the role of `FormalWeb.UserAgent`: allocator state, spec-facing global sets,
/// worker ownership, and the pending navigation/fetch continuations that connect the embedder,
/// content, fetch, and timer threads.
pub struct UserAgentState {
    /// Allocator block mirroring the counters in `FormalWeb.UserAgent`.
    pub ids: UserAgentIds,
    /// <https://html.spec.whatwg.org/multipage/#browsing-context-group-set>
    pub browsing_context_group_set: BrowsingContextGroupSet,
    /// <https://html.spec.whatwg.org/multipage/#top-level-traversable-set>
    pub traversable_set: TraversableSet,
    /// <https://html.spec.whatwg.org/multipage/#navigable>
    pub navigables: HashMap<u64, Navigable>,
    /// Lookup from IPC-facing navigable UUIDs to user-agent internal traversable ids.
    pub internal_navigable_ids_by_public_id: HashMap<NavigableId, u64>,
    /// <https://html.spec.whatwg.org/multipage/#tlbc-group>
    pub top_level_browsing_context_group_ids: HashMap<u64, u64>,
    /// map from Rust worker handles to the owned event-loop workers.
    pub event_loops: HashMap<usize, EventLoopEntry>,
    /// reverse index from <https://html.spec.whatwg.org/multipage/#event-loop> ids to
    /// the owning Rust handle.
    pub handles_by_event_loop_id: HashMap<usize, usize>,
    /// reverse index from top-level traversable ids to the owning Rust handle.
    pub traversable_handles: HashMap<u64, usize>,
    /// cache of each traversable's active target name derived from
    /// `traversable_set`.
    pub traversable_target_names: HashMap<u64, String>,
    /// cache of each traversable's active document derived from
    /// `traversable_set`.
    pub active_documents_by_traversable: HashMap<u64, u64>,
    /// cache of active and pending documents keyed by
    /// <https://dom.spec.whatwg.org/#concept-document> identifiers.
    pub documents: HashMap<u64, DocumentState>,
    /// queue of navigations paused while content runs `beforeunload`.
    pub pending_before_unload_navigations: HashMap<u64, PendingBeforeUnloadNavigation>,
    /// queue of fetch-backed navigations suspended at the response wait point.
    pub pending_navigation_fetches: HashMap<NavigationId, PendingNavigationFetch>,
    /// reverse index from <https://fetch.spec.whatwg.org/#fetch-controller> ids to
    /// pending navigation ids.
    pub pending_navigation_fetch_ids_by_fetch_id: HashMap<NavigationFetchId, NavigationId>,
    /// queue of documents waiting for
    /// <https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation>
    pub pending_navigation_finalizations: HashMap<u64, PendingNavigationFinalization>,
    /// reverse index from <https://html.spec.whatwg.org/multipage/#navigation-params-id>
    /// to pending finalization document ids.
    pub pending_navigation_finalization_ids_by_navigation_id: HashMap<NavigationId, u64>,
}

/// cache of the active document state held by the user agent.
#[derive(Clone)]
pub struct DocumentState {
    /// reference back to the top-level traversable that currently presents the
    /// document.
    pub traversable_id: u64,
    /// reference to the active browsing context for the document.
    pub browsing_context_id: Option<u64>,
    /// owner event loop for the document's content implementation.
    pub event_loop_id: usize,
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
    pub check_id: u64,
    /// identifier corresponding to
    /// <https://html.spec.whatwg.org/multipage/#navigation-params-id>
    pub navigation_id: NavigationId,
    pub navigable_id: u64,
    pub destination_url: String,
    pub user_involvement: ipc_messages::content::UserNavigationInvolvement,
    /// Documents still expected to report their beforeunload result for this navigation.
    pub pending_document_ids: HashSet<u64>,
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
    pub traversable_id: u64,
    pub previous_document_id: Option<u64>,
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
    pub document_id: u64,
    /// identifier corresponding to
    /// <https://html.spec.whatwg.org/multipage/#navigation-params-id>
    pub navigation_id: NavigationId,
    pub traversable_id: u64,
    pub previous_document_id: Option<u64>,
    /// browsing context selected for the new document before commit.
    pub browsing_context_id: Option<u64>,
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
            ids: UserAgentIds::default(),
            browsing_context_group_set: BrowsingContextGroupSet::default(),
            traversable_set: TraversableSet::default(),
            navigables: HashMap::new(),
            internal_navigable_ids_by_public_id: HashMap::new(),
            top_level_browsing_context_group_ids: HashMap::new(),
            event_loops: HashMap::new(),
            handles_by_event_loop_id: HashMap::new(),
            traversable_handles: HashMap::new(),
            traversable_target_names: HashMap::new(),
            active_documents_by_traversable: HashMap::new(),
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
    /// <https://html.spec.whatwg.org/multipage/#top-level-traversable>
    fn top_level_traversable_id(&self, traversable_id: u64) -> Option<u64> {
        let mut current_id = traversable_id;
        loop {
            let traversable = self.traversable_set.members.get(&current_id)?;
            if let Some(parent_id) = traversable.parent_traversable_id {
                current_id = parent_id;
                continue;
            }
            return Some(current_id);
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#bc-tlbc>
    fn top_level_browsing_context_id(&self, browsing_context_id: u64) -> Option<u64> {
        let traversable_id = self
            .documents
            .values()
            .find_map(|document| {
                (document.browsing_context_id == Some(browsing_context_id))
                    .then_some(document.traversable_id)
            })?;
        let top_level_traversable_id = self.top_level_traversable_id(traversable_id)?;
        self.traversable_set
            .members
            .get(&top_level_traversable_id)
            .and_then(|traversable| traversable.active_browsing_context_id)
    }

    /// selecting the embedder-visible active
    /// <https://html.spec.whatwg.org/multipage/#top-level-traversable>.
    fn set_active_top_level_traversable(&mut self, traversable_id: u64) {
        let top_level_id = self.top_level_traversable_id(traversable_id);
        for (candidate_id, traversable) in &mut self.traversable_set.members {
            if traversable.parent_traversable_id.is_some() {
                continue;
            }
            traversable.is_active = Some(*candidate_id) == top_level_id;
            if let Some(navigable) = self.navigables.get_mut(candidate_id)
                && let Some(navigable_traversable) = navigable.traversable.as_mut()
            {
                navigable_traversable.is_active = traversable.is_active;
            }
        }
    }

    /// caching the active document of one top-level traversable.
    fn set_traversable_active_document(&mut self, traversable_id: u64, document_id: u64) {
        self.active_documents_by_traversable
            .insert(traversable_id, document_id);
        if let Some(navigable) = self.navigables.get_mut(&traversable_id) {
            navigable.active_document_id = Some(document_id);
        }
        if let Some(traversable) = self.traversable_set.members.get_mut(&traversable_id) {
            traversable.active_document_id = Some(document_id);
        }
        if let Some(navigable) = self.navigables.get_mut(&traversable_id)
            && let Some(traversable) = navigable.traversable.as_mut()
        {
            traversable.active_document_id = Some(document_id);
        }
    }

    /// caching the active browsing context selected for one traversable-backed navigable.
    fn set_traversable_active_browsing_context(
        &mut self,
        traversable_id: u64,
        browsing_context_id: Option<u64>,
    ) {
        if let Some(traversable) = self.traversable_set.members.get_mut(&traversable_id) {
            traversable.active_browsing_context_id = browsing_context_id;
        }
        if let Some(navigable) = self.navigables.get_mut(&traversable_id)
            && let Some(traversable) = navigable.traversable.as_mut()
        {
            traversable.active_browsing_context_id = browsing_context_id;
        }
    }

    /// updating the current
    /// <https://html.spec.whatwg.org/multipage/#ongoing-navigation> marker.
    fn set_traversable_ongoing_navigation(
        &mut self,
        traversable_id: u64,
        navigation_id: Option<NavigationId>,
    ) {
        if let Some(traversable) = self.traversable_set.members.get_mut(&traversable_id) {
            traversable.ongoing_navigation_id = navigation_id;
        }
        if let Some(navigable) = self.navigables.get_mut(&traversable_id)
            && let Some(traversable) = navigable.traversable.as_mut()
        {
            traversable.ongoing_navigation_id = navigation_id;
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation>
    fn commit_session_history_entry(
        &mut self,
        traversable_id: u64,
        history_entry: SessionHistoryEntry,
        history_handling: HistoryHandlingBehavior,
    ) {
        if let Some(traversable) = self.traversable_set.members.get_mut(&traversable_id) {
            // <https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation>
            // Step 5: Let entryToReplace be navigable's active session history entry if
            // historyHandling is "replace", otherwise null.
            // Note: The branch below folds the presence or absence of `entryToReplace` into the
            // push-versus-replace match instead of storing an intermediate variable.
            // Step 9: If entryToReplace is null, clear the forward session history, set
            // historyEntry's step, and append it.
            // Note: The push branch below truncates forward history, assigns the next step, and
            // appends the replacement entry in one block.
            // Step 10: Apply the push/replace history step targetStep to traversable given
            // historyHandling and userInvolvement.
            match history_handling {
                HistoryHandlingBehavior::Push => {
                    traversable
                        .session_history_entries
                        .retain(|entry| entry.step <= traversable.current_session_history_step);
                    let next_step = traversable.current_session_history_step.saturating_add(1);
                    traversable.current_session_history_step = next_step;
                    traversable.session_history_entries.push(SessionHistoryEntry {
                        step: next_step,
                        ..history_entry
                    });
                }
                HistoryHandlingBehavior::Replace => {
                    let current_step = traversable.current_session_history_step;
                    let replacement_entry = SessionHistoryEntry {
                        step: current_step,
                        ..history_entry
                    };
                    if let Some(entry) = traversable
                        .session_history_entries
                        .iter_mut()
                        .find(|entry| entry.step == current_step)
                    {
                        *entry = replacement_entry;
                    } else {
                        traversable.session_history_entries.push(replacement_entry);
                    }
                }
            }

            if let Some(navigable) = self.navigables.get_mut(&traversable_id)
                && let Some(navigable_traversable) = navigable.traversable.as_mut()
            {
                *navigable_traversable = traversable.clone();
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
        let navigation_id = self.pending_navigation_fetch_ids_by_fetch_id.remove(&fetch_id)?;
        self.pending_navigation_fetches.remove(&navigation_id)
    }

    /// dropping all pending fetch continuations owned by one traversable.
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
        document_id: u64,
    ) -> Option<PendingNavigationFinalization> {
        let pending = self.pending_navigation_finalizations.remove(&document_id)?;
        self.pending_navigation_finalization_ids_by_navigation_id
            .remove(&pending.navigation_id);
        Some(pending)
    }

    /// dropping all pending finalization continuations owned by one traversable.
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

    /// removing one traversable and its derived indices from the user-agent state.
    fn remove_traversable(&mut self, traversable_id: u64) {
        let Some(traversable) = self.traversable_set.members.get(&traversable_id).cloned() else {
            return;
        };
        let browsing_context_id = traversable.active_browsing_context_id;
        let removed_top_level_browsing_context_id = traversable
            .parent_traversable_id
            .is_none()
            .then_some(browsing_context_id)
            .flatten();

        self.traversable_set.members.remove(&traversable_id);
        self.navigables.remove(&traversable_id);
        self.internal_navigable_ids_by_public_id
            .retain(|_, candidate_traversable_id| *candidate_traversable_id != traversable_id);
        self.traversable_handles.remove(&traversable_id);
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
    Navigate {
        request: NavigateRequest,
    },
    CompleteBeforeUnload {
        result: BeforeUnloadResult,
    },
    FinalizeCrossDocumentNavigation {
        finalized: ContentFinalizeNavigation,
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
        handler_id: DocumentFetchId,
        response: ContentFetchResponse,
    },
    DocumentFetchFailed {
        event_loop_id: usize,
        handler_id: DocumentFetchId,
    },
    NavigationFetchCompleted {
        fetch_id: NavigationFetchId,
        response: ContentFetchResponse,
    },
    NavigationFetchFailed {
        fetch_id: NavigationFetchId,
    },
    DocumentFetchTimeout {
        event_loop_id: usize,
        handler_id: DocumentFetchId,
    },
    WindowTimerTask {
        event_loop_id: usize,
        document_id: u64,
        timer_id: u32,
        timer_key: WindowTimerKey,
        nesting_level: u32,
    },
    CreateChildNavigable {
        parent_traversable_id: u64,
        content_navigable_id: ContentNavigableId,
        content_frame_id: FrameId,
    },
    IframeTraversableRemoved {
        parent_traversable_id: u64,
        content_navigable_id: ContentNavigableId,
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
    pub fn start(user_event_dispatcher: UserEventDispatcher) -> Result<Self, String> {
        let (command_sender, command_receiver) = unbounded();
        let mut worker = UserAgentWorker::new(
            command_sender.clone(),
            command_receiver,
            user_event_dispatcher,
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
        traversable_id: u64,
        source: String,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        webview::UserAgentApi::evaluate_script(self, traversable_id, source, timeout)
    }
}

impl Drop for UserAgent {
    /// best-effort shutdown of the owned user-agent thread.
    fn drop(&mut self) {
        if let Err(error) = self.shutdown_inner() {
            eprintln!("failed to shut down user-agent thread: {error}");
        }
    }
}

impl webview::UserAgentApi for UserAgent {
    /// <https://html.spec.whatwg.org/multipage/#create-a-fresh-top-level-traversable>
    fn start_top_level_traversable(&self, destination_url: String) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::CreateFreshTopLevelTraversable { destination_url })
            .map_err(|error| {
                format!("failed to start create-a-fresh-top-level-traversable: {error}")
            })
    }

    /// <https://html.spec.whatwg.org/multipage/#navigate>
    fn start_navigation(&self, request: NavigateRequest) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::Navigate { request })
            .map_err(|error| format!("failed to send navigate command: {error}"))
    }

    /// queuing DOM event dispatch on the traversable's owning
    /// <https://html.spec.whatwg.org/multipage/#event-loop>.
    fn dispatch_event_for(&self, traversable_id: u64, event: String) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::DispatchEventFor {
                traversable_id,
                event,
            })
            .map_err(|error| format!("failed to send dispatch-event request: {error}"))
    }

    /// <https://html.spec.whatwg.org/multipage/#update-the-rendering>
    fn note_rendering_opportunity(&self, traversable_id: u64) -> Result<(), String> {
        self.command_sender
            .send(UserAgentCommand::RenderingOpportunityFor { traversable_id })
            .map_err(|error| format!("failed to send rendering-opportunity request: {error}"))
    }

    /// broadcasting the embedder viewport to every owned content event loop.
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

    /// updating the viewport of one traversable's content implementation.
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

    /// the automation-only script-evaluation bridge into content.
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

/// render-state debug output on the user-agent thread.
fn log_render_state_debug(message: impl AsRef<str>) {
    if std::env::var_os("FORMAL_WEB_DEBUG_RENDER_STATE").is_some() {
        eprintln!("[render-state][user-agent] {}", message.as_ref());
    }
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
        .and_then(|parsed| {
            parsed
                .host_str()
                .map(|host| format!("{}://{}", parsed.scheme(), host))
        })
        .unwrap_or_else(|| String::from("about:blank"))
}

fn descendant_traversable_ids(state: &UserAgentState, traversable_id: u64) -> Vec<u64> {
    let mut descendants = Vec::new();
    let mut stack = vec![traversable_id];
    let mut visited = HashSet::from([traversable_id]);

    while let Some(parent_id) = stack.pop() {
        for (candidate_id, traversable) in &state.traversable_set.members {
            let Some(candidate_parent_id) = traversable.parent_traversable_id else {
                continue;
            };
            if candidate_parent_id != parent_id || !visited.insert(*candidate_id) {
                continue;
            }
            descendants.push(*candidate_id);
            stack.push(*candidate_id);
        }
    }

    descendants
}

fn descendant_navigable_ids(state: &UserAgentState, navigable_id: u64) -> Vec<u64> {
    let mut descendants = Vec::new();
    let mut stack = vec![navigable_id];
    let mut visited = HashSet::from([navigable_id]);

    while let Some(parent_id) = stack.pop() {
        for (candidate_id, navigable) in &state.navigables {
            let Some(candidate_parent_id) = navigable.parent_navigable_id else {
                continue;
            };
            if candidate_parent_id != parent_id || !visited.insert(*candidate_id) {
                continue;
            }
            descendants.push(*candidate_id);
            stack.push(*candidate_id);
        }
    }

    descendants
}

/// <https://html.spec.whatwg.org/multipage/#find-a-navigable-by-target-name>
fn find_navigable_by_target_name(state: &UserAgentState, target_name: &str) -> Option<u64> {
    state.navigables.iter().find_map(|(navigable_id, navigable)| {
        navigable
            .traversable
            .as_ref()
            .filter(|traversable| traversable.target_name == target_name)
            .map(|_| *navigable_id)
    })
}

/// Stateful owner of browser-global state plus the fetch, timer, and event-loop workers that the
/// user-agent thread coordinates.
struct UserAgentWorker {
    /// Spec-facing browser state plus the indices that make the implementation route
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
    /// Async dispatcher used to notify the embedder event loop about navigation and traversable
    /// updates without routing through a global sender.
    user_event_dispatcher: UserEventDispatcher,
    /// request ids for script-evaluation round-trips across the user-agent and
    /// content event-loop boundary.
    next_script_request_id: u64,
}

impl UserAgentWorker {
    /// starting the fetch and timer workers owned by the user-agent thread.
    fn new(
        user_agent_command_sender: Sender<UserAgentCommand>,
        command_receiver: Receiver<UserAgentCommand>,
        user_event_dispatcher: UserEventDispatcher,
    ) -> Self {
        let (fetch_command_sender, fetch_command_receiver) = unbounded();
        let fetch_user_agent_command_sender = user_agent_command_sender.clone();
        let fetch_join_handle = thread::Builder::new()
            .name(String::from("formal-web:fetch"))
            .spawn(move || run_fetch_thread(fetch_command_receiver, fetch_user_agent_command_sender))
            .unwrap_or_else(|error| panic!("failed to spawn formal-web-fetch thread: {error}"));
        let (timer_command_sender, timer_command_receiver) = unbounded();
        let timer_user_agent_command_sender = user_agent_command_sender.clone();
        let timer_join_handle = thread::Builder::new()
            .name(String::from("formal-web:timer"))
            .spawn(move || run_timer_thread(timer_command_receiver, timer_user_agent_command_sender))
            .unwrap_or_else(|error| panic!("failed to spawn formal-web-timer thread: {error}"));

        Self {
            state: UserAgentState::default(),
            command_sender: user_agent_command_sender,
            command_receiver,
            fetch_command_sender,
            fetch_join_handle: Some(fetch_join_handle),
            timer_command_sender,
            timer_join_handle: Some(timer_join_handle),
            user_event_dispatcher,
            next_script_request_id: 1,
        }
    }

    /// the top-level command loop that owns browser-global coordination.
    fn run(&mut self) {
        while let Ok(command) = self.command_receiver.recv() {
            match command {
                UserAgentCommand::CreateFreshTopLevelTraversable { destination_url } => {
                    self.create_a_fresh_top_level_traversable(destination_url);
                }
                UserAgentCommand::Navigate { request } => {
                    self.handle_navigate(request);
                }
                UserAgentCommand::CompleteBeforeUnload { result } => {
                    self.handle_complete_before_unload(result);
                }
                UserAgentCommand::FinalizeCrossDocumentNavigation { finalized } => {
                    self.handle_finalize_cross_document_navigation(finalized);
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
                UserAgentCommand::RenderingOpportunityFor {
                    traversable_id,
                } => {
                    self.handle_rendering_opportunity_for(traversable_id);
                }
                UserAgentCommand::DocumentFetchCompleted {
                    event_loop_id,
                    handler_id,
                    response,
                } => {
                    self.handle_document_fetch_completed(event_loop_id, handler_id, response);
                }
                UserAgentCommand::DocumentFetchFailed {
                    event_loop_id,
                    handler_id,
                } => {
                    self.handle_document_fetch_failed(event_loop_id, handler_id);
                }
                UserAgentCommand::NavigationFetchCompleted { fetch_id, response } => {
                    self.handle_navigation_fetch_completed(fetch_id, response);
                }
                UserAgentCommand::NavigationFetchFailed { fetch_id } => {
                    self.handle_navigation_fetch_failed(fetch_id);
                }
                UserAgentCommand::DocumentFetchTimeout {
                    event_loop_id,
                    handler_id,
                } => {
                    self.handle_document_fetch_timeout(event_loop_id, handler_id);
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
                UserAgentCommand::CreateChildNavigable {
                    parent_traversable_id,
                    content_navigable_id,
                    content_frame_id,
                } => {
                    self.handle_create_child_navigable(
                        parent_traversable_id,
                        content_navigable_id,
                        content_frame_id,
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
    }
}

impl UserAgentWorker {
    /// the request/reply path that sends one command through the owning
    /// event loop and waits for the content-side reply.
    fn send_event_loop_command(
        &self,
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

    /// resolving the event-loop command channel that owns one traversable.
    fn command_sender_for_traversable(
        &self,
        traversable_id: u64,
    ) -> Result<Sender<EventLoopCommand>, String> {
        let handle = self
            .state
            .traversable_handles
            .get(&traversable_id)
            .copied()
            .ok_or_else(|| format!("unknown traversable id: {traversable_id}"))?;
        self.state
            .event_loops
            .get(&handle)
            .map(|entry| entry.command_sender.clone())
            .ok_or_else(|| format!("missing event loop for handle {handle}"))
    }

    /// <https://html.spec.whatwg.org/multipage/#create-an-agent>
    fn create_agent(&mut self, can_block: bool, process_label: String) -> Result<Agent, String> {
        // Step 1: Let signifier be a new unique internal value.
        let agent_id = self.state.ids.allocate_agent_id();
        // Step 2: Let candidateExecution be a new candidate execution.
        // The Rust model does not surface a separate candidate-execution object because the
        // dedicated event-loop thread owns the scheduling state that HTML leaves implementation-defined.
        // Step 4: Set agent's event loop to a new event loop.
        let event_loop_id = self.state.ids.allocate_event_loop_id();
        let handle = self.state.ids.allocate_handle();
        let entry = spawn_event_loop_entry(
            event_loop_id,
            process_label,
            self.command_sender.clone(),
            self.fetch_command_sender.clone(),
            self.timer_command_sender.clone(),
            self.user_event_dispatcher.clone(),
        )?;
        self.state.handles_by_event_loop_id.insert(event_loop_id, handle);
        self.state.event_loops.insert(handle, entry);
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
    fn create_new_top_level_traversable(&mut self, target_name: String) -> Result<u64, String> {
        if parse_iframe_target_name(&target_name).is_some() {
            return Err(format!(
                "synthetic iframe target {target_name:?} must be materialized by create-a-new-child-navigable"
            ));
        }

        let traversable_id = self.state.ids.allocate_traversable_id();
        let iframe_parent_traversable_id = None;
        let frame_id = None;

        // Step 1: Let document be null.
        // Note: The null placeholder stays implicit; the Rust model materializes the initial
        // about:blank document directly in the browsing-context setup below.
        self.state.ids.observe_traversable_id(traversable_id);

        // Step 2: With a null opener, create a new top-level browsing context and document.
        let browsing_context_group_id = self.state.browsing_context_group_set.next_group_id();
        let browsing_context_id = self.state.ids.allocate_browsing_context_id();
        let agent_cluster_id = self.state.ids.allocate_agent_cluster_id();
        let agent = self.create_agent(false, String::from("about:blank"))?;
        let document_id = self.state.ids.allocate_document_id();
        let handle = self
            .state
            .handles_by_event_loop_id
            .get(&agent.event_loop_id)
            .copied()
            .ok_or_else(|| format!("missing event loop handle for agent {}", agent.id))?;
        let command_sender = self
            .state
            .event_loops
            .get(&handle)
            .map(|entry| entry.command_sender.clone())
            .ok_or_else(|| format!("missing event loop entry for handle {handle}"))?;

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

        self.state
            .event_loops
            .get_mut(&handle)
            .expect("event loop entry disappeared during top-level creation")
            .traversable_ids
            .insert(traversable_id);
        self.state.traversable_handles.insert(traversable_id, handle);
        self.state
            .traversable_target_names
            .insert(traversable_id, target_name.clone());
        self.state.set_traversable_active_document(traversable_id, document_id);
        self.state
            .top_level_browsing_context_group_ids
            .insert(browsing_context_id, browsing_context_group_id);
        self.state.browsing_context_group_set.members.insert(
            browsing_context_group_id,
            BrowsingContextGroup {
                id: browsing_context_group_id,
                browsing_context_set: HashMap::from([(
                    browsing_context_id,
                    BrowsingContext { id: browsing_context_id },
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
        let traversable = Traversable {
            id: traversable_id,
            parent_traversable_id: iframe_parent_traversable_id,
            is_active: false,
            target_name: target_name.clone(),
            active_browsing_context_id: Some(browsing_context_id),
            active_document_id: Some(document_id),
            event_loop_id: agent.event_loop_id,
            handle,
            ongoing_navigation_id: None,
            has_deferred_update_the_rendering: false,
            frame_id,
            current_session_history_step: 0,
            session_history_entries: vec![SessionHistoryEntry {
                step: 0,
                document_id,
                url: String::from("about:blank"),
            }],
        };
        self.state
            .traversable_set
            .members
            .insert(traversable_id, traversable.clone());
        self.state.navigables.insert(
            traversable_id,
            Navigable {
                id: traversable_id,
                parent_navigable_id: iframe_parent_traversable_id,
                active_document_id: Some(document_id),
                traversable: Some(traversable),
            },
        );
        self.state
            .internal_navigable_ids_by_public_id
            .insert(NavigableId::from_u128(traversable_id as u128), traversable_id);
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
        self.user_event_dispatcher
            .send(FormalWebUserEvent::NewTopLevelTraversable(
                WebviewId(traversable_id),
                target_name,
            ))?;
        // Step 13: Return traversable.
        Ok(traversable_id)
    }

    /// <https://html.spec.whatwg.org/#create-a-new-child-navigable>
    /// Note: This helper materializes the user-agent state for an iframe's initial about:blank
    /// child navigable and reuses the parent's event loop until a later cross-origin navigation
    /// causes `initialise_the_document_object` to move it.
    fn create_new_child_navigable(
        &mut self,
        parent_traversable_id: u64,
        content_navigable_id: ContentNavigableId,
        content_frame_id: FrameId,
    ) -> Result<u64, String> {
        let target_name = iframe_target_name(
            parent_traversable_id,
            content_navigable_id,
            content_frame_id,
        );
        if let Some(navigable_id) = find_navigable_by_target_name(&self.state, &target_name) {
            self.state
                .internal_navigable_ids_by_public_id
                .insert(content_navigable_id.into(), navigable_id);
            return Ok(navigable_id);
        }

        let parent_handle = self
            .state
            .traversable_handles
            .get(&parent_traversable_id)
            .copied()
            .ok_or_else(|| format!("unknown parent traversable id: {parent_traversable_id}"))?;
        let parent_traversable = self
            .state
            .traversable_set
            .members
            .get(&parent_traversable_id)
            .cloned()
            .ok_or_else(|| format!("missing parent traversable {parent_traversable_id}"))?;
        let parent_browsing_context_id = parent_traversable.active_browsing_context_id.ok_or_else(|| {
            format!("parent traversable {parent_traversable_id} has no active browsing context")
        })?;
        let top_level_browsing_context_id = self
            .state
            .top_level_browsing_context_id(parent_browsing_context_id)
            .unwrap_or(parent_browsing_context_id);
        let browsing_context_id = self.state.ids.allocate_browsing_context_id();
        let traversable_id = self.state.ids.allocate_traversable_id();
        let document_id = self.state.ids.allocate_document_id();

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
            .insert(browsing_context_id, BrowsingContext { id: browsing_context_id });

        self.state.traversable_handles.insert(traversable_id, parent_handle);
        self.state
            .traversable_target_names
            .insert(traversable_id, target_name.clone());
        self.state.set_traversable_active_document(traversable_id, document_id);
        self.state.documents.insert(
            document_id,
            DocumentState {
                traversable_id,
                browsing_context_id: Some(browsing_context_id),
                event_loop_id: parent_traversable.event_loop_id,
                url: String::from("about:blank"),
                is_initial_about_blank: true,
            },
        );

        let traversable = Traversable {
            id: traversable_id,
            parent_traversable_id: Some(parent_traversable_id),
            is_active: false,
            target_name: target_name.clone(),
            active_browsing_context_id: Some(browsing_context_id),
            active_document_id: Some(document_id),
            event_loop_id: parent_traversable.event_loop_id,
            handle: parent_handle,
            ongoing_navigation_id: None,
            has_deferred_update_the_rendering: false,
            frame_id: Some(content_frame_id),
            current_session_history_step: 0,
            session_history_entries: vec![SessionHistoryEntry {
                step: 0,
                document_id,
                url: String::from("about:blank"),
            }],
        };
        self.state
            .traversable_set
            .members
            .insert(traversable_id, traversable.clone());
        self.state.navigables.insert(
            traversable_id,
            Navigable {
                id: traversable_id,
                parent_navigable_id: Some(parent_traversable_id),
                active_document_id: Some(document_id),
                traversable: Some(traversable),
            },
        );
        self.state
            .internal_navigable_ids_by_public_id
            .insert(content_navigable_id.into(), traversable_id);
        self.state
            .event_loops
            .get_mut(&parent_handle)
            .ok_or_else(|| format!("missing parent event loop handle {parent_handle}"))?
            .traversable_ids
            .insert(traversable_id);

        self.user_event_dispatcher
            .send(FormalWebUserEvent::NewTopLevelTraversable(
                WebviewId(traversable_id),
                target_name,
            ))?;

        Ok(traversable_id)
    }

    /// <https://html.spec.whatwg.org/#create-a-new-child-navigable>
    /// Note: Content runs the local iframe/container work first and then asks the user agent to
    /// continue the suffix that allocates the stable child navigable and session-history state.
    fn handle_create_child_navigable(
        &mut self,
        parent_traversable_id: u64,
        content_navigable_id: ContentNavigableId,
        content_frame_id: FrameId,
    ) {
        let result = self.create_new_child_navigable(
            parent_traversable_id,
            content_navigable_id,
            content_frame_id,
        );
        if let Err(error) = result {
            eprintln!("failed to create child navigable: {error}");
        }
    }

    /// <https://html.spec.whatwg.org/multipage/browsers.html#obtain-browsing-context-navigation>
    /// Note: The current model uses the active document URL plus a same-site check as the
    /// observable approximation for swap-group decisions before response-driven document creation.
    fn obtain_browsing_context_to_use_for_navigation_response(
        &mut self,
        traversable_id: u64,
        destination_url: &str,
    ) -> Result<BrowsingContextNavigationSelection, String> {
        let traversable = self
            .state
            .traversable_set
            .members
            .get(&traversable_id)
            .cloned()
            .ok_or_else(|| format!("missing traversable {traversable_id}"))?;
        let browsing_context_id = traversable.active_browsing_context_id.ok_or_else(|| {
            format!("traversable {traversable_id} has no active browsing context")
        })?;

        if traversable.parent_traversable_id.is_some() {
            return Ok(BrowsingContextNavigationSelection {
                browsing_context_id,
                swapped_group: false,
            });
        }

        let source_document_url = traversable
            .active_document_id
            .and_then(|document_id| self.state.documents.get(&document_id))
            .map(|document| document.url.clone());
        let swap_group = match source_document_url {
            Some(source_document_url) => is_cross_origin_navigation(&source_document_url, destination_url)?,
            None => false,
        };
        if !swap_group {
            return Ok(BrowsingContextNavigationSelection {
                browsing_context_id,
                swapped_group: false,
            });
        }

        let new_group_id = self.state.browsing_context_group_set.next_group_id();
        let new_browsing_context_id = self.state.ids.allocate_browsing_context_id();
        self.state.browsing_context_group_set.members.insert(
            new_group_id,
            BrowsingContextGroup {
                id: new_group_id,
                browsing_context_set: HashMap::from([(
                    new_browsing_context_id,
                    BrowsingContext {
                        id: new_browsing_context_id,
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
        traversable_id: u64,
        browsing_context_id: Option<u64>,
    ) {
        let Some(browsing_context_id) = browsing_context_id else {
            return;
        };
        let is_top_level = self
            .state
            .traversable_set
            .members
            .get(&traversable_id)
            .is_some_and(|traversable| traversable.parent_traversable_id.is_none());
        let is_active = self
            .state
            .traversable_set
            .members
            .get(&traversable_id)
            .and_then(|traversable| traversable.active_browsing_context_id)
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
        traversable_id: u64,
        destination_url: String,
        user_involvement: UserNavigationInvolvement,
    ) -> Result<(), String> {
        // Step 1: Assert: this is running in parallel.
        // Note: The user-agent thread performs the navigation-fetch setup inline on the
        // user-agent thread; the actual network request runs in parallel in the fetch worker.
        let fetch_id = self.state.ids.allocate_fetch_id();
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
        self.state.insert_pending_navigation_fetch(PendingNavigationFetch {
            fetch_id,
            navigation_id,
            traversable_id,
            previous_document_id,
            request: request.clone(),
            source_snapshot_params: SourceSnapshotParams::for_user_involvement(&user_involvement),
            target_snapshot_params: TargetSnapshotParams::default(),
            navigation_timing_type: NavigationTimingType::Navigate,
            csp_navigation_type: String::from("other"),
            allow_post: false,
            user_involvement: user_involvement.clone(),
        });
        if let Err(error) = self
            .fetch_command_sender
            .send(FetchCommand::StartNavigationFetch {
                fetch_id,
                request: request.to_content_fetch_request(),
            })
        {
            let _ = self
                .state
                .take_pending_navigation_fetch_by_navigation_id(navigation_id);
            self.state
                .set_traversable_ongoing_navigation(traversable_id, None);
            return Err(format!("failed to start navigation fetch: {error}"));
        }

        Ok(())
    }

    /// <https://html.spec.whatwg.org/multipage/#navigate>
    /// Note: Steps 1–18 that require access to the source document or the navigable's active
    /// window (sandboxing, fragment navigation, historyHandling auto-resolution,
    /// targetSnapshotParams, and the Navigation API fire-navigate event) are executed in the
    /// content process before sending the `NavigateRequest` IPC. This function continues from
    /// step 19.
    fn navigate(
        &mut self,
        navigable_id: u64,
        destination_url: String,
        user_involvement: UserNavigationInvolvement,
        navigation_id: NavigationId,
    ) -> Result<(), String> {
        let traversable_id = self.traversable_id_for_navigable(navigable_id)?;
        // Note: The inclusive-descendant navigable set needed for step 23a is pre-computed here
        // before setting the ongoing navigation so that it reflects the current tree state.
        let beforeunload_navigable_ids = std::iter::once(navigable_id)
            .chain(descendant_navigable_ids(&self.state, navigable_id))
            .collect::<Vec<_>>();
        let beforeunload_navigable_ids = beforeunload_navigable_ids
            .into_iter()
            .filter(|candidate_navigable_id| {
                self.state
                    .navigables
                    .get(candidate_navigable_id)
                    .and_then(|navigable| navigable.active_document_id)
                    .and_then(|document_id| self.state.documents.get(&document_id))
                    .is_some_and(|document| !document.is_initial_about_blank)
            })
            .collect::<Vec<_>>();

        // Step 19: "Set the ongoing navigation for navigable to navigationId."
        self.state
            .set_traversable_ongoing_navigation(traversable_id, Some(navigation_id));

        // Step 23: "In parallel, run these steps:"

        if !beforeunload_navigable_ids.is_empty() {
            self.check_if_unloading_is_canceled(
                navigation_id,
                navigable_id,
                destination_url,
                user_involvement,
                beforeunload_navigable_ids,
            )
        } else {
            self.create_navigation_params_by_fetching(
                navigation_id,
                traversable_id,
                destination_url,
                user_involvement,
            )
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled>
    fn check_if_unloading_is_canceled(
        &mut self,
        navigation_id: NavigationId,
        navigable_id: u64,
        destination_url: String,
        user_involvement: UserNavigationInvolvement,
        navigables_that_need_before_unload: Vec<u64>,
    ) -> Result<(), String> {
        // Step 1: "Let documentsToFireBeforeunload be the active document of each item in
        // navigablesThatNeedBeforeUnload."
        let documents_to_fire_beforeunload = navigables_that_need_before_unload
            .iter()
            .filter_map(|candidate_navigable_id| {
                self.state
                    .navigables
                    .get(candidate_navigable_id)
                    .and_then(|navigable| navigable.active_document_id)
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
            let Ok(candidate_traversable_id) = self.traversable_id_for_navigable(*candidate_navigable_id)
            else {
                continue;
            };
            let Some(document_id) = self
                .state
                .navigables
                .get(candidate_navigable_id)
                .and_then(|navigable| navigable.active_document_id)
            else {
                continue;
            };
            beforeunload_targets
                .entry(document_id)
                .or_insert(candidate_traversable_id);
        }

        let check_id = self.state.ids.allocate_before_unload_check_id();
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
            if let Err(error) = self.send_event_loop_command(
                &command_sender,
                ContentCommand::RunBeforeUnload {
                    document_id,
                    check_id,
                },
            ) {
                self.state.pending_before_unload_navigations.remove(&check_id);
                return Err(error);
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
            .traversable_set
            .members
            .get(&traversable_id)
            .and_then(|traversable| traversable.ongoing_navigation_id)
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
    /// Note: This helper continues the algorithm after content has already had a chance to run
    /// the local `_self` / `_parent` / `_top` steps. The remaining work here is the shared
    /// target-name lookup and new-top-level branch, plus defensive handling for callers that did
    /// not provide a preselected navigable id.
    fn continue_choosing_navigable_after_content_selection(
        &mut self,
        source_navigable_id: NavigableId,
        target_name: &str,
        noopener: bool,
    ) -> Result<NavigableId, String> {
        let source_navigable_id_u64 = self.resolve_internal_navigable_id(source_navigable_id)?;
        
        // Note: `normalize_navigation_target_name` folds the `_self` special case into the empty
        // string before the direct step checks below.
        let normalized_target_name = normalize_navigation_target_name(target_name);

        // Step 4: "If name is the empty string or an ASCII case-insensitive match for \"_self\", then set chosen to currentNavigable."
        // Note: Content-driven callers are expected to resolve this branch locally. This fallback
        // keeps browser-UI or legacy callers correct when they enter `navigate` without a
        // preselected navigable id.
        if normalized_target_name.is_empty() {
            return Ok(source_navigable_id);
        }

        // Step 5: "Otherwise, if name is an ASCII case-insensitive match for \"_parent\", set chosen to currentNavigable's parent, if any, and currentNavigable otherwise."
        // Note: Content-driven callers are expected to resolve this branch locally.
        if normalized_target_name.eq_ignore_ascii_case("_parent") {
            let result = self.state
                .navigables
                .get(&source_navigable_id_u64)
                .and_then(|navigable| navigable.parent_navigable_id)
                .unwrap_or(source_navigable_id_u64);
            return Ok(NavigableId::from_u128(result as u128));
        }

        // Step 6: "Otherwise, if name is an ASCII case-insensitive match for \"_top\", set chosen to currentNavigable's traversable navigable."
        // Note: Content-driven callers are expected to resolve this branch locally.
        if normalized_target_name.eq_ignore_ascii_case("_top") {
            let source_traversable_id = self.traversable_id_for_navigable(source_navigable_id_u64)?;
            let top_level_traversable_id = self
                .state
                .top_level_traversable_id(source_traversable_id)
                .unwrap_or(source_traversable_id);
            return Ok(NavigableId::from_u128(top_level_traversable_id as u128));
        }

        // Step 8: "If chosen is null, then a new top-level traversable is being requested."
        if noopener || normalized_target_name.eq_ignore_ascii_case("_blank") {
            let new_traversable_id = self.create_new_top_level_traversable(String::new())?;
            return Ok(NavigableId::from_u128(new_traversable_id as u128));
        }

        // Step 7: "Otherwise, if name is not an ASCII case-insensitive match for \"_blank\" and noopener is false, then set chosen to the result of finding a navigable by target name given name and currentNavigable."
        if let Some(chosen_navigable_id) =
            find_navigable_by_target_name(&self.state, &normalized_target_name)
        {
            return Ok(NavigableId::from_u128(chosen_navigable_id as u128));
        }

        if parse_iframe_target_name(&normalized_target_name).is_some() {
            return Err(format!(
                "synthetic iframe target {normalized_target_name:?} was not materialized before navigate continued in the user agent"
            ));
        }

        // Step 8: "If chosen is null, then a new top-level traversable is being requested."
        let new_traversable_id = self.create_new_top_level_traversable(normalized_target_name)?;
        Ok(NavigableId::from_u128(new_traversable_id as u128))
    }

    fn traversable_id_for_navigable(&self, navigable_id: u64) -> Result<u64, String> {
        self.state
            .navigables
            .get(&navigable_id)
            .and_then(|navigable| navigable.traversable.as_ref().map(|traversable| traversable.id))
            .ok_or_else(|| format!("navigable {navigable_id} is not a traversable navigable"))
    }

    fn resolve_internal_navigable_id(&self, public_id: NavigableId) -> Result<u64, String> {
        if let Some(internal_id) = self
            .state
            .internal_navigable_ids_by_public_id
            .get(&public_id)
            .copied()
        {
            return Ok(internal_id);
        }

        let fallback = public_id.0.as_u128();
        if fallback > u64::MAX as u128 {
            return Err(format!("unknown navigable id: {public_id}"));
        }
        let internal_id = fallback as u64;
        if self.state.navigables.contains_key(&internal_id) {
            return Ok(internal_id);
        }

        Err(format!("unknown navigable id: {public_id}"))
    }

    /// removing an event-loop worker and every derived index owned by it.
    fn remove_event_loop_entry(&mut self, handle: usize) -> Option<EventLoopEntry> {
        let entry = self.state.event_loops.remove(&handle)?;
        self.state.handles_by_event_loop_id.remove(&entry.event_loop_id);
        let removed_traversable_ids = entry.traversable_ids.iter().copied().collect::<Vec<_>>();
        for traversable_id in &removed_traversable_ids {
            self.state.remove_traversable(*traversable_id);
            self.state
                .remove_pending_navigation_fetches_for_traversable(*traversable_id);
            let _ = self
                .state
                .remove_pending_navigation_finalizations_for_traversable(*traversable_id);
        }
        self.state.documents.retain(|_, document| {
            !removed_traversable_ids.contains(&document.traversable_id)
        });
        let before_unload_checks_to_remove = self
            .state
            .pending_before_unload_navigations
            .iter()
            .filter_map(|(check_id, pending)| {
                let traversable_id = self
                    .state
                    .navigables
                    .get(&pending.navigable_id)
                    .and_then(|navigable| {
                        navigable.traversable.as_ref().map(|traversable| traversable.id)
                    })?;
                removed_traversable_ids
                    .contains(&traversable_id)
                    .then_some(*check_id)
            })
            .collect::<Vec<_>>();
        for check_id in before_unload_checks_to_remove {
            self.state.pending_before_unload_navigations.remove(&check_id);
        }
        Some(entry)
    }

    /// stopping one owned event-loop worker by its Rust handle.
    fn stop_event_loop_handle(&mut self, handle: usize) -> Result<(), String> {
        match self.remove_event_loop_entry(handle) {
            Some(entry) => stop_event_loop_entry(entry),
            None => Ok(()),
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#create-a-fresh-top-level-traversable>
    /// Note: This helper creates the initial traversable/document shell immediately and then
    /// continues through the normal user-agent `navigate` / fetch / finalization path for the
    /// supplied startup URL.
    fn create_a_fresh_top_level_traversable(&mut self, destination_url: String) {
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
            eprintln!("failed to create a fresh top-level traversable: {error}");
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#navigate>
    fn handle_navigate(&mut self, request: NavigateRequest) {
        let result: Result<(), String> = (|| {
            let navigable_id = match request.chosen_navigable_id {
                Some(chosen_navigable_id) => chosen_navigable_id,
                None => self.continue_choosing_navigable_after_content_selection(
                    request.source_navigable_id,
                    &request.target,
                    request.noopener,
                )?,
            };
            let navigable_id_u64 = self.resolve_internal_navigable_id(navigable_id)?;
            let traversable_id = self.traversable_id_for_navigable(navigable_id_u64)?;
            let navigation_id = request.navigation_id.unwrap_or_else(NavigationId::new);
            self.navigate(
                navigable_id_u64,
                request.destination_url.clone(),
                request.user_involvement.clone(),
                navigation_id,
            )?;
            // Only notify the embedder for top-level navigations, not iframe children.
            let is_top_level = self
                .state
                .traversable_set
                .members
                .get(&traversable_id)
                .map(|t| t.parent_traversable_id.is_none())
                .unwrap_or(true);
            if is_top_level {
                self.user_event_dispatcher.send(FormalWebUserEvent::NavigationRequested {
                    webview_id: WebviewId(traversable_id),
                    destination_url: request.destination_url,
                })?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("failed to run navigate: {error}");
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
        traversable_id: u64,
        final_url: &str,
    ) -> Result<Option<u64>, String> {
        // Step 1: "Let browsingContext be the result of obtaining a browsing context to use for
        // a navigation response given navigationParams."
        // Note: `obtain_browsing_context_to_use_for_navigation_response` implements the
        // `#obtain-browsing-context-navigation` algorithm; its `swapped_group` field captures
        // whether a browsing-context-group switch is needed for top-level traversables.
        let browsing_context_selection =
            self.obtain_browsing_context_to_use_for_navigation_response(traversable_id, final_url)?;
        let parent_traversable_id = self
            .state
            .traversable_set
            .members
            .get(&traversable_id)
            .and_then(|t| t.parent_traversable_id);

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
        let old_handle = self
            .state
            .traversable_handles
            .get(&traversable_id)
            .copied();
        let agent = self.create_agent(false, content_process_label_from_url(final_url))?;
        let new_handle = *self
            .state
            .handles_by_event_loop_id
            .get(&agent.event_loop_id)
            .ok_or_else(|| String::from("missing handle for new agent event loop"))?;
        if let Some(old_h) = old_handle {
            if let Some(old_entry) = self.state.event_loops.get_mut(&old_h) {
                old_entry.traversable_ids.remove(&traversable_id);
            }
        }
        if let Some(new_entry) = self.state.event_loops.get_mut(&new_handle) {
            new_entry.traversable_ids.insert(traversable_id);
        }
        self.state.traversable_handles.insert(traversable_id, new_handle);
        if let Some(traversable) = self.state.traversable_set.members.get_mut(&traversable_id) {
            traversable.event_loop_id = agent.event_loop_id;
            traversable.handle = new_handle;
        }
        if let Some(navigable) = self.state.navigables.get_mut(&traversable_id)
            && let Some(nav_traversable) = navigable.traversable.as_mut()
        {
            nav_traversable.event_loop_id = agent.event_loop_id;
            nav_traversable.handle = new_handle;
        }
        Ok(Some(browsing_context_selection.browsing_context_id))
    }

    /// <https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled>
    fn handle_complete_before_unload_result(
        &mut self,
        result: BeforeUnloadResult,
    ) -> Result<(), String> {
        if let Some(pending) = self.state.pending_before_unload_navigations.get_mut(&result.check_id) {
            if !pending.pending_document_ids.remove(&result.document_id) {
                return Ok(());
            }
            pending.canceled |= result.canceled;
            if !pending.pending_document_ids.is_empty() {
                return Ok(());
            }
        }

        if let Some(pending) = self
            .state
            .pending_before_unload_navigations
            .remove(&result.check_id)
        {
            if pending.canceled || result.canceled {
                let traversable_id = self.traversable_id_for_navigable(pending.navigable_id)?;
                let navigation_is_current = self
                    .state
                    .traversable_set
                    .members
                    .get(&traversable_id)
                    .and_then(|traversable| traversable.ongoing_navigation_id)
                    == Some(pending.navigation_id);
                if navigation_is_current {
                    self.state
                        .set_traversable_ongoing_navigation(traversable_id, None);
                }
                self.user_event_dispatcher
                    .send(FormalWebUserEvent::BeforeUnloadCompleted(BeforeUnloadResult {
                        canceled: true,
                        ..result
                    }))
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
            eprintln!("failed to complete beforeunload: {error}");
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
            .traversable_set
            .members
            .get(&pending.traversable_id)
            .and_then(|traversable| traversable.ongoing_navigation_id)
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
            .traversable_set
            .members
            .get(&pending.traversable_id)
            .and_then(|traversable| traversable.active_browsing_context_id);

        // Step 4: "If all of the following are true: navigable's parent is null; historyEntry's
        // document's browsing context is not an auxiliary browsing context whose opener browsing
        // context is non-null; and historyEntry's document's origin is not navigable's active
        // document's origin, then set historyEntry's document state's navigable target name to
        // the empty string."
        // TODO: `SessionHistoryEntry` does not yet carry a per-entry navigable target name
        // field; this branch is not executed.

        self.state.set_traversable_active_browsing_context(
            pending.traversable_id,
            pending.browsing_context_id,
        );
        self.state
            .set_traversable_active_document(pending.traversable_id, finalized.document_id);

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
        self.state
            .set_traversable_ongoing_navigation(pending.traversable_id, None);
        if let Some(document) = self.state.documents.get_mut(&finalized.document_id) {
            document.url = finalized.url.clone();
            document.is_initial_about_blank = finalized.url == "about:blank";
        }
        self.handle_rendering_opportunity_for(pending.traversable_id);
        let notify_result = self.user_event_dispatcher.send(
            FormalWebUserEvent::FinalizeNavigation(FinalizeNavigation {
                webview_id: WebviewId(pending.traversable_id),
                url: finalized.url.clone(),
            }),
        );

        if let Some(previous_document_id) = pending.previous_document_id {
            if previous_document_id != finalized.document_id {
                // The old document is destroyed after the new document commits so stale
                // content-side traffic cannot revive it after the traversable has advanced.
                if let Ok(command_sender) = self.command_sender_for_traversable(pending.traversable_id)
                {
                    let _ = self.send_event_loop_command(
                        &command_sender,
                        ContentCommand::DestroyDocument {
                            document_id: previous_document_id,
                        },
                    );
                }
                self.state.documents.remove(&previous_document_id);
            }
        }

        if let Some(new_browsing_context_id) = pending.browsing_context_id {
            let is_top_level = self
                .state
                .traversable_set
                .members
                .get(&pending.traversable_id)
                .is_some_and(|traversable| traversable.parent_traversable_id.is_none());
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
            eprintln!("failed to finalize cross-document navigation: {error}");
        }
    }

    /// the automation-only script-evaluation bridge into the owning event loop.
    fn handle_evaluate_script(
        &mut self,
        traversable_id: u64,
        source: String,
        timeout: Duration,
        reply: Sender<Result<serde_json::Value, String>>,
    ) {
        let error_reply = reply.clone();
        let send_result =
            match self.state.traversable_handles.get(&traversable_id).copied() {
                Some(handle) => match self.state.event_loops.get(&handle) {
                    Some(entry) => {
                        let request_id = self.next_script_request_id;
                        self.next_script_request_id = self.next_script_request_id.wrapping_add(1);
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

    /// applying the default viewport to the active traversable and its descendants.
    fn handle_set_default_viewport(&mut self, snapshot: (u32, u32, f32, ColorScheme)) {
        let active_top_level_traversable_id = self
            .state
            .traversable_set
            .members
            .iter()
            .find_map(|(traversable_id, traversable)| {
                (traversable.parent_traversable_id.is_none() && traversable.is_active)
                    .then_some(*traversable_id)
            });
        let Some(traversable_id) = active_top_level_traversable_id else {
            return;
        };

        self.handle_set_traversable_viewport(traversable_id, snapshot, 0.0, 0.0);
        for descendant_traversable_id in descendant_traversable_ids(&self.state, traversable_id) {
            self.handle_set_traversable_viewport(descendant_traversable_id, snapshot, 0.0, 0.0);
        }
    }

    /// sending a per-traversable viewport update to the owning event loop.
    fn handle_set_traversable_viewport(
        &mut self,
        traversable_id: u64,
        snapshot: (u32, u32, f32, ColorScheme),
        offset_x: f32,
        offset_y: f32,
    ) {
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
    fn handle_dispatch_event_for(&mut self, traversable_id: u64, event: String) {
        let Some(handle) = self.state.traversable_handles.get(&traversable_id).copied() else {
            return;
        };
        let Some(document_id) = self.state.active_documents_by_traversable.get(&traversable_id)
        else {
            return;
        };
        let Some(entry) = self.state.event_loops.get(&handle) else {
            return;
        };

        let command = ContentCommand::DispatchEvent {
            events: vec![DispatchEventEntry {
                document_id: *document_id,
                event,
            }],
        };
        let _ = entry
            .command_sender
            .send(EventLoopCommand::FireAndForget { command });
    }

    /// <https://html.spec.whatwg.org/multipage/#update-the-rendering>
    fn handle_rendering_opportunity_for(&mut self, traversable_id: u64) {
        let Some(handle) = self.state.traversable_handles.get(&traversable_id).copied() else {
            return;
        };
        let Some(document_id) = self.state.active_documents_by_traversable.get(&traversable_id)
        else {
            return;
        };
        let Some(entry) = self.state.event_loops.get(&handle) else {
            return;
        };

        log_render_state_debug(format!(
            "send rendering opportunity traversable={} document={} handle={}",
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
    fn handle_document_fetch_completed(
        &mut self,
        event_loop_id: usize,
        handler_id: DocumentFetchId,
        response: ContentFetchResponse,
    ) {
        let _ = self
            .timer_command_sender
            .send(TimerCommand::Clear { timer_key: handler_id.0 });
        let Some(handle) = self.state.handles_by_event_loop_id.get(&event_loop_id).copied() else {
            return;
        };
        let Some(entry) = self.state.event_loops.get(&handle) else {
            return;
        };
        let command = ContentCommand::CompleteDocumentFetch {
            handler_id,
            response,
        };
        let _ = entry
            .command_sender
            .send(EventLoopCommand::FireAndForget { command });
    }

    /// failing an event-loop-local document fetch after the fetch worker fails.
    fn handle_document_fetch_failed(&mut self, event_loop_id: usize, handler_id: DocumentFetchId) {
        let _ = self
            .timer_command_sender
            .send(TimerCommand::Clear { timer_key: handler_id.0 });
        let Some(handle) = self.state.handles_by_event_loop_id.get(&event_loop_id).copied() else {
            return;
        };
        let Some(entry) = self.state.event_loops.get(&handle) else {
            return;
        };
        let command = ContentCommand::FailDocumentFetch { handler_id };
        let _ = entry
            .command_sender
            .send(EventLoopCommand::FireAndForget { command });
    }

    /// <https://html.spec.whatwg.org/multipage/#attempt-to-populate-the-history-entry's-document>
    fn handle_navigation_fetch_completed(
        &mut self,
        fetch_id: NavigationFetchId,
        response: ContentFetchResponse,
    ) {
        let Some(pending) = self.state.take_pending_navigation_fetch_by_fetch_id(fetch_id) else {
            return;
        };
        let navigation_is_current = self
            .state
            .traversable_set
            .members
            .get(&pending.traversable_id)
            .and_then(|traversable| traversable.ongoing_navigation_id)
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
        let browsing_context_id = match self.initialise_the_document_object(
            pending.traversable_id,
            &final_url,
        ) {
            Ok(browsing_context_id) => browsing_context_id,
            Err(error) => {
                self.state
                    .set_traversable_ongoing_navigation(pending.traversable_id, None);
                let _ = self.user_event_dispatcher.send(FormalWebUserEvent::NavigationFailed {
                    webview_id: WebviewId(pending.traversable_id),
                    message: error,
                });
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
                    .set_traversable_ongoing_navigation(pending.traversable_id, None);
                let _ = self.user_event_dispatcher.send(FormalWebUserEvent::NavigationFailed {
                    webview_id: WebviewId(pending.traversable_id),
                    message: error,
                });
                return;
            }
        };
        let document_id = self.state.ids.allocate_document_id();
        // Note: For child navigables the compositor frame_id is forwarded so the content process
        // can identify which iframe slot this document renders into.
        let frame_id = self
            .state
            .traversable_set
            .members
            .get(&pending.traversable_id)
            .and_then(|t| t.frame_id);
        let loaded_response = LoadedDocumentResponse {
            final_url: final_url.clone(),
            status: response.status,
            content_type: response.content_type.clone(),
            body: String::from_utf8_lossy(&response.body).into_owned(),
        };
        let (traversable_event_loop_id, parent_traversable_id) = self
            .state
            .traversable_set
            .members
            .get(&pending.traversable_id)
            .map(|t| (t.event_loop_id, t.parent_traversable_id))
            .unwrap_or((0, None));
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
        self.state.documents.insert(document_id, document_state.clone());
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
                self.state.insert_pending_navigation_finalization(PendingNavigationFinalization {
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
                    .set_traversable_ongoing_navigation(pending.traversable_id, None);
                let _ = self.user_event_dispatcher.send(FormalWebUserEvent::NavigationFailed {
                    webview_id: WebviewId(pending.traversable_id),
                    message: error,
                });
            }
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#attempt-to-populate-the-history-entry's-document>
    fn handle_navigation_fetch_failed(&mut self, fetch_id: NavigationFetchId) {
        let Some(pending) = self.state.take_pending_navigation_fetch_by_fetch_id(fetch_id) else {
            return;
        };
        self.state
            .set_traversable_ongoing_navigation(pending.traversable_id, None);
        let _ = self.user_event_dispatcher.send(FormalWebUserEvent::NavigationFailed {
            webview_id: WebviewId(pending.traversable_id),
            message: format!("navigation fetch failed for {}", pending.request.url),
        });
    }

    /// the document-fetch watchdog fired by the timer worker.
    fn handle_document_fetch_timeout(&mut self, event_loop_id: usize, handler_id: DocumentFetchId) {
        let Some(handle) = self.state.handles_by_event_loop_id.get(&event_loop_id).copied() else {
            return;
        };
        let Some(entry) = self.state.event_loops.get(&handle) else {
            return;
        };
        let command = ContentCommand::FailDocumentFetch { handler_id };
        let _ = entry
            .command_sender
            .send(EventLoopCommand::FireAndForget { command });
    }

    /// <https://html.spec.whatwg.org/multipage/#timers>
    fn handle_window_timer_task(
        &mut self,
        event_loop_id: usize,
        document_id: u64,
        timer_id: u32,
        timer_key: WindowTimerKey,
        nesting_level: u32,
    ) {
        let Some(handle) = self.state.handles_by_event_loop_id.get(&event_loop_id).copied() else {
            return;
        };
        let Some(entry) = self.state.event_loops.get(&handle) else {
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
        parent_traversable_id: u64,
        content_navigable_id: ContentNavigableId,
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

        let mut handles_to_maybe_stop = HashSet::new();
        let mut removed_document_ids = HashSet::new();

        for traversable_id in &traversable_ids {
            if let Some(handle) = self.state.traversable_handles.get(traversable_id).copied() {
                handles_to_maybe_stop.insert(handle);
                if let Some(entry) = self.state.event_loops.get_mut(&handle) {
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
                self.state.pending_before_unload_navigations.remove(&check_id);
            }
        }

        let mut result = Ok(());
        for handle in handles_to_maybe_stop {
            let should_stop = self
                .state
                .event_loops
                .get(&handle)
                .is_some_and(|entry| entry.traversable_ids.is_empty());
            if !should_stop {
                continue;
            }
            if let Err(error) = self.stop_event_loop_handle(handle) {
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
        self.state.handles_by_event_loop_id.clear();
        self.state.browsing_context_group_set.members.clear();
        self.state.traversable_set.members.clear();
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
    }
}