use ipc_messages::content::NavigationFetchId;

/// Model-local allocator state for the browser-global identifiers owned by the user-agent.
///
/// The Rust runtime still uses primitive integer ids on the wire, but grouping the allocators in
/// one struct keeps the state surface aligned with the HTML and Fetch concepts it tracks.
#[derive(Clone, Debug)]
pub struct UserAgentIds {
    /// Model-local identifier for the content worker handle owned by the user agent.
    pub next_handle: usize,
    /// <https://html.spec.whatwg.org/multipage/#event-loop>
    pub next_event_loop_id: usize,
    /// Model-local identifier for the Rust-owned top-level traversable/webview surface.
    pub next_traversable_id: u64,
    /// Model-local identifier for <https://html.spec.whatwg.org/multipage/#browsing-context>
    pub next_browsing_context_id: u64,
    /// <https://dom.spec.whatwg.org/#concept-document>
    pub next_document_id: u64,
    /// <https://html.spec.whatwg.org/multipage/#agent-cluster>
    pub next_agent_cluster_id: u64,
    /// <https://tc39.es/ecma262/#sec-agents>
    pub next_agent_id: u64,
    /// <https://html.spec.whatwg.org/multipage/#ongoing-navigation>
    pub next_navigation_id: u64,
    /// Model-local identifier for pending
    /// <https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled>
    /// continuations.
    pub next_before_unload_check_id: u64,
}

impl Default for UserAgentIds {
    /// seeding the user-agent's browser-global identifier allocators.
    fn default() -> Self {
        Self {
            next_handle: 1,
            next_event_loop_id: 1,
            next_traversable_id: 1,
            next_browsing_context_id: 1,
            next_document_id: 1,
            next_agent_cluster_id: 0,
            next_agent_id: 0,
            next_navigation_id: 1,
            next_before_unload_check_id: 1,
        }
    }
}

impl UserAgentIds {
    /// allocating Rust handles that own content event loops.
    pub fn allocate_handle(&mut self) -> usize {
        let handle = self.next_handle;
        self.next_handle += 1;
        handle
    }

    /// <https://html.spec.whatwg.org/multipage/#event-loop>.
    pub fn allocate_event_loop_id(&mut self) -> usize {
        let event_loop_id = self.next_event_loop_id;
        self.next_event_loop_id += 1;
        event_loop_id
    }

    /// observing an externally supplied event-loop id before allocating the next one.
    pub fn observe_event_loop_id(&mut self, event_loop_id: usize) {
        self.next_event_loop_id = self.next_event_loop_id.max(event_loop_id + 1);
    }

    /// <https://html.spec.whatwg.org/multipage/#top-level-traversable>.
    pub fn allocate_traversable_id(&mut self) -> u64 {
        let traversable_id = self.next_traversable_id;
        self.next_traversable_id += 1;
        traversable_id
    }

    /// observing a traversable id created by another runtime component.
    pub fn observe_traversable_id(&mut self, traversable_id: u64) {
        self.next_traversable_id = self.next_traversable_id.max(traversable_id + 1);
    }

    /// <https://html.spec.whatwg.org/multipage/#browsing-context>.
    pub fn allocate_browsing_context_id(&mut self) -> u64 {
        let browsing_context_id = self.next_browsing_context_id;
        self.next_browsing_context_id += 1;
        browsing_context_id
    }

    /// <https://dom.spec.whatwg.org/#concept-document>.
    pub fn allocate_document_id(&mut self) -> u64 {
        let document_id = self.next_document_id;
        self.next_document_id += 1;
        document_id
    }

    /// observing a document id created by content before allocating the next one.
    pub fn observe_document_id(&mut self, document_id: u64) {
        self.next_document_id = self.next_document_id.max(document_id + 1);
    }

    /// <https://html.spec.whatwg.org/multipage/#agent-cluster>.
    pub fn allocate_agent_cluster_id(&mut self) -> u64 {
        let agent_cluster_id = self.next_agent_cluster_id;
        self.next_agent_cluster_id += 1;
        agent_cluster_id
    }

    /// <https://tc39.es/ecma262/#sec-agents>.
    pub fn allocate_agent_id(&mut self) -> u64 {
        let agent_id = self.next_agent_id;
        self.next_agent_id += 1;
        agent_id
    }

    /// <https://html.spec.whatwg.org/multipage/#ongoing-navigation>.
    pub fn allocate_navigation_id(&mut self) -> u64 {
        let navigation_id = self.next_navigation_id;
        self.next_navigation_id += 1;
        navigation_id
    }

    /// queued `beforeunload` continuations under
    /// <https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled>.
    pub fn allocate_before_unload_check_id(&mut self) -> u64 {
        let check_id = self.next_before_unload_check_id;
        self.next_before_unload_check_id += 1;
        check_id
    }

    /// <https://fetch.spec.whatwg.org/#fetch-controller> ids owned by the user agent.
    pub fn allocate_fetch_id(&mut self) -> NavigationFetchId {
        NavigationFetchId::new()
    }
}