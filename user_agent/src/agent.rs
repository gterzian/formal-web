//! User-agent-side records of the HTML agents and agent clusters — the
//! "integration with the JavaScript agent formalism" and "integration with
//! the JavaScript agent cluster formalism" sections of the HTML spec: the
//! similar-origin window agents the user agent obtains inside the agent
//! clusters it spawns, and the dedicated worker agents its content
//! processes report as obtained.  An agent cluster is one content process;
//! it contains the cluster's single similar-origin window agent and the
//! dedicated worker agents of the workers the cluster's realms create
//! (nested arbitrarily: a worker agent's realm can create further
//! workers, whose agents join the same cluster).  Each agent owns an event
//! loop; the user-agent-side records of those event loops live in
//! [`crate::event_loops`].

use std::collections::HashSet;

use ipc_messages::content::{
    AgentClusterId, AgentId, EventLoopId, NavigableId, WorkerId, WorkerOwner,
};

use crate::event_loops::{WindowEventLoop, WorkerEventLoop};

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
/// Origins are stored as serialized strings here until the dedicated origin
/// model is shared across all browser components.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum AgentClusterKey {
    Site(String),
    Origin(String),
}

/// <https://html.spec.whatwg.org/multipage/webappapis.html#similar-origin-window-agent>
#[derive(Debug)]
pub struct SimilarOriginWindowAgent {
    /// The signifier created by <https://html.spec.whatwg.org/multipage/#create-an-agent>.
    pub id: AgentId,
    /// <https://tc39.es/ecma262/#sec-agents>
    pub can_block: bool,
    /// <https://html.spec.whatwg.org/multipage/#concept-agent-event-loop>
    /// The agent's window event loop, which runs on the main thread of the
    /// agent cluster's content process.
    pub event_loop_id: EventLoopId,
    /// The user-agent-side record of that window event loop: the event loop
    /// id, the content process's command channel, its event channel, and
    /// the content process itself.
    pub event_loop: WindowEventLoop,
    /// The traversables whose active documents run on this agent's event loop.
    pub traversable_ids: HashSet<NavigableId>,
}

/// <https://html.spec.whatwg.org/multipage/webappapis.html#dedicated-worker-agent>
#[derive(Debug)]
pub struct DedicatedWorkerAgent {
    /// The identifier of the worker whose global scope this agent runs.
    pub worker_id: WorkerId,
    /// <https://html.spec.whatwg.org/multipage/#concept-WorkerGlobalScope-owner-set>
    /// The realm that created the worker.
    pub owner: WorkerOwner,
    /// The similar-origin window agent of the agent cluster (the content
    /// process) hosting this agent's thread; the record is dropped when
    /// that process exits, since the process exit takes the worker threads
    /// down.
    pub host_agent_id: AgentId,
    /// <https://html.spec.whatwg.org/multipage/#concept-agent-event-loop>
    /// The agent's own worker event loop, distinct from the window event
    /// loop of the similar-origin window agent hosting its thread.
    pub event_loop: WorkerEventLoop,
}

/// <https://html.spec.whatwg.org/multipage/#agent-cluster>
#[derive(Clone, Debug)]
pub struct AgentCluster {
    /// The identifier of the agent cluster.
    pub id: AgentClusterId,
    /// <https://html.spec.whatwg.org/multipage/#agent-cluster-cross-origin-isolation>
    pub cross_origin_isolation_mode: CrossOriginIsolationMode,
    /// <https://html.spec.whatwg.org/multipage/#is-origin-keyed>
    pub is_origin_keyed: bool,
    /// The single similar-origin window agent contained in the agent
    /// cluster, referenced by signifier into `UserAgentState::agents`.
    /// <https://html.spec.whatwg.org/multipage/#similar-origin-window-agent>
    pub similar_origin_window_agent: AgentId,
}
