use crate::AutomationRuntime;
use crate::{AUTOMATION_TIMEOUT, CdpEvent, HttpRequest, SCRIPT_TIMEOUT, find_header_terminator};
use base64::Engine as _;
use clap::Args;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinSet;
use tokio::time;
use tokio_tungstenite::{WebSocketStream, accept_async};
use tungstenite::{Error as WebSocketError, Message};
use uuid::Uuid;

const CDP_BROWSER_PRODUCT: &str = "formal-web/0.1";
const CDP_PROTOCOL_VERSION: &str = "1.3";
const CDP_SOCKET_IO_TIMEOUT: Duration = Duration::from_millis(100);
const CDP_PEEK_LIMIT: usize = 16 * 1024;
const CDP_EXECUTION_CONTEXT_ID: u64 = 1;
const CDP_UTILITY_EXECUTION_CONTEXT_ID: u64 = 2;
const CDP_DOCUMENT_NODE_ID: u64 = 1;

#[derive(Args, Debug, Clone)]
pub struct CdpArgs {
    #[arg(long, default_value_t = 9222)]
    pub port: u16,

    #[arg(long)]
    pub headless: bool,

    #[arg(long, value_name = "URL")]
    pub startup_url: Option<String>,
}

pub struct CdpServerHandle {
    shutdown: Option<oneshot::Sender<()>>,
    listener_thread: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct CdpState {
    port: u16,
    runtime: AutomationRuntime,
    browser_id: String,
    browser_context_id: String,
    page_target_id: String,
}

struct CdpPageSnapshot {
    url: String,
    frame_id: String,
}

#[derive(Clone, Copy)]
enum CdpRoute {
    Browser,
    Page,
}

struct PeekedRequest {
    method: String,
    target: String,
    websocket_upgrade: bool,
}

struct CdpRequest {
    id: Option<Value>,
    method: String,
    params: Value,
    session_id: Option<String>,
}

struct CdpConnectionState {
    route: CdpRoute,
    session_id: Option<String>,
    page_enabled: bool,
    runtime_enabled: bool,
    utility_world_name: Option<String>,
    utility_execution_context_id: Option<u64>,
    dom_nodes: HashMap<u64, CdpDomNode>,
    locator_node_ids: HashMap<CdpNodeLocator, u64>,
    search_results: HashMap<String, Vec<u64>>,
    next_node_id: u64,
    next_search_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CdpNodeLocator {
    Document,
    Path(Vec<usize>),
}

#[derive(Clone, Debug)]
struct CdpDomNode {
    locator: CdpNodeLocator,
}

impl CdpServerHandle {
    pub fn start(port: u16, runtime: AutomationRuntime) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|error| format!("failed to bind CDP server on port {port}: {error}"))?;
        let actual_port = listener
            .local_addr()
            .map_err(|error| format!("failed to read CDP listener address: {error}"))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("failed to configure CDP server listener: {error}"))?;

        let state = CdpState {
            port: actual_port,
            runtime,
            browser_id: new_cdp_id(),
            browser_context_id: new_cdp_id(),
            page_target_id: new_cdp_id(),
        };
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let listener_thread = thread::Builder::new()
            .name(String::from("formal-web-cdp"))
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build();
                match runtime {
                    Ok(runtime) => {
                        runtime.block_on(run_cdp_server(listener, state, shutdown_receiver))
                    }
                    Err(error) => eprintln!("formal-web cdp server init error: {error}"),
                }
            })
            .map_err(|error| format!("failed to spawn CDP server thread: {error}"))?;

        Ok(Self {
            shutdown: Some(shutdown_sender),
            listener_thread: Some(listener_thread),
        })
    }
}

impl Drop for CdpServerHandle {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(listener_thread) = self.listener_thread.take() {
            if let Err(error) = listener_thread.join() {
                eprintln!("[cdp] failed to join listener thread: {error:?}");
            }
        }
    }
}

impl CdpState {
    fn browser_ws_url(&self) -> String {
        format!(
            "ws://localhost:{}/devtools/browser/{}",
            self.port, self.browser_id
        )
    }

    fn page_ws_url(&self) -> String {
        format!(
            "ws://localhost:{}/devtools/page/{}",
            self.port, self.page_target_id
        )
    }

    fn page_snapshot(&self) -> CdpPageSnapshot {
        let snapshot = self.runtime.snapshot(Duration::from_millis(250)).ok();
        let url = snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot.current_url.clone().or_else(|| {
                    (!snapshot.displayed_url.is_empty()).then(|| snapshot.displayed_url.clone())
                })
            })
            .unwrap_or_else(|| String::from("about:blank"));
        let frame_id = self.page_target_id.clone();

        CdpPageSnapshot { url, frame_id }
    }

    fn active_page_snapshot(&self) -> Option<CdpPageSnapshot> {
        let snapshot = self.runtime.snapshot(Duration::from_millis(250)).ok()?;
        if !snapshot.has_top_level_traversable || snapshot.webview_id.is_none() {
            return None;
        }
        let url = snapshot
            .current_url
            .or_else(|| (!snapshot.displayed_url.is_empty()).then_some(snapshot.displayed_url))
            .unwrap_or_else(|| String::from("about:blank"));
        let frame_id = self.page_target_id.clone();

        Some(CdpPageSnapshot { url, frame_id })
    }

    fn target_info(&self, attached: bool) -> Value {
        let snapshot = self.page_snapshot();
        target_info_payload(self, &snapshot.url, attached)
    }
}

impl CdpRequest {
    fn parse(text: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(text).map_err(|error| format!("invalid CDP JSON: {error}"))?;
        let Some(object) = value.as_object() else {
            return Err(String::from("CDP message must be a JSON object"));
        };
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| String::from("CDP message is missing a string `method`"))?
            .to_owned();

        Ok(Self {
            id: object.get("id").cloned(),
            method,
            params: object.get("params").cloned().unwrap_or_else(|| json!({})),
            session_id: object
                .get("sessionId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        })
    }
}

impl CdpConnectionState {
    fn new(route: CdpRoute) -> Self {
        Self {
            route,
            session_id: None,
            page_enabled: false,
            runtime_enabled: false,
            utility_world_name: None,
            utility_execution_context_id: None,
            dom_nodes: HashMap::new(),
            locator_node_ids: HashMap::new(),
            search_results: HashMap::new(),
            next_node_id: CDP_DOCUMENT_NODE_ID + 1,
            next_search_id: 1,
        }
    }

    fn reset_dom_state(&mut self) {
        self.dom_nodes.clear();
        self.locator_node_ids.clear();
        self.search_results.clear();
        self.next_node_id = CDP_DOCUMENT_NODE_ID + 1;
        self.next_search_id = 1;
    }

    fn node_locator(&self, node_id: u64) -> Result<CdpNodeLocator, String> {
        if node_id == CDP_DOCUMENT_NODE_ID {
            return Ok(CdpNodeLocator::Document);
        }

        self.dom_nodes
            .get(&node_id)
            .map(|node| node.locator.clone())
            .ok_or_else(|| format!("unknown DOM node id `{node_id}`"))
    }

    fn register_locator(&mut self, locator: CdpNodeLocator) -> u64 {
        if locator == CdpNodeLocator::Document {
            return CDP_DOCUMENT_NODE_ID;
        }
        if let Some(node_id) = self.locator_node_ids.get(&locator).copied() {
            return node_id;
        }

        let node_id = self.next_node_id;
        self.next_node_id += 1;
        self.dom_nodes.insert(
            node_id,
            CdpDomNode {
                locator: locator.clone(),
            },
        );
        self.locator_node_ids.insert(locator, node_id);
        node_id
    }

    fn register_locators(&mut self, locators: Vec<CdpNodeLocator>) -> Vec<u64> {
        locators
            .into_iter()
            .map(|locator| self.register_locator(locator))
            .collect()
    }

    fn next_search_id(&mut self) -> String {
        let search_id = format!("search-{}", self.next_search_id);
        self.next_search_id += 1;
        search_id
    }

    fn handle_text(&mut self, state: &CdpState, text: &str) -> Result<Vec<Value>, String> {
        let request = CdpRequest::parse(text)?;
        let mut events = Vec::new();
        let response_session_id =
            self.response_session_id(&request.method, request.session_id.clone());

        let result = match request.method.as_str() {
            "Browser.getVersion" => Ok(json!({
                "protocolVersion": CDP_PROTOCOL_VERSION,
                "product": CDP_BROWSER_PRODUCT,
                "revision": "0",
                "userAgent": CDP_BROWSER_PRODUCT,
                "jsVersion": "0.0"
            })),
            "Browser.close" => {
                state
                    .runtime
                    .request_exit()
                    .map_err(|error| format!("browser shutdown failed: {error}"))?;
                Ok(json!({}))
            }
            "Target.getTargets" => Ok(json!({
                "targetInfos": [state.target_info(self.session_id.is_some())]
            })),
            "Target.getTargetInfo" => Ok(json!({
                "targetInfo": state.target_info(self.session_id.is_some())
            })),
            "Target.attachToTarget" => {
                let requested_target = request
                    .params
                    .get("targetId")
                    .and_then(Value::as_str)
                    .unwrap_or(state.page_target_id.as_str());
                if requested_target != state.page_target_id {
                    Err(format!("unknown target id `{requested_target}`"))
                } else {
                    let session_id = self.session_id.get_or_insert_with(new_cdp_id).clone();
                    Ok(json!({ "sessionId": session_id }))
                }
            }
            "Target.setDiscoverTargets" => {
                if request
                    .params
                    .get("discover")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    events.push(target_created_event(state, self.session_id.is_some()));
                }
                Ok(json!({}))
            }
            "Target.setAutoAttach" => {
                if request
                    .params
                    .get("autoAttach")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    if self.session_id.is_none() {
                        let session_id = self.session_id.get_or_insert_with(new_cdp_id).clone();
                        events.push(attached_to_target_event(state, &session_id));
                    }
                }
                Ok(json!({}))
            }
            "Page.enable" => {
                self.page_enabled = true;
                if let Some(event) = self.current_frame_navigated_event(state) {
                    events.push(event);
                }
                Ok(json!({}))
            }
            "Page.navigate" => {
                let Some(url) = request.params.get("url").and_then(Value::as_str) else {
                    return Ok(vec![cdp_error_response(
                        request.id,
                        response_session_id.as_deref(),
                        String::from("`Page.navigate` requires a string `url` parameter"),
                    )]);
                };

                state
                    .runtime
                    .navigate(url, AUTOMATION_TIMEOUT)
                    .map_err(|error| format!("navigation failed: {error}"))?;
                self.reset_dom_state();
                let frame_id = state.page_target_id.clone();
                Ok(json!({
                    "frameId": frame_id,
                    "loaderId": frame_id
                }))
            }
            "Page.captureScreenshot" => {
                let png = state
                    .runtime
                    .screenshot(AUTOMATION_TIMEOUT)
                    .map_err(|error| format!("screenshot failed: {error}"))?;
                Ok(json!({
                    "data": base64::engine::general_purpose::STANDARD.encode(png)
                }))
            }
            "Page.getFrameTree" => {
                let snapshot = state.page_snapshot();
                Ok(json!({
                    "frameTree": {
                        "frame": frame_payload(&snapshot.url, &snapshot.frame_id)
                    }
                }))
            }
            "Page.addScriptToEvaluateOnNewDocument" => Ok(json!({ "identifier": new_cdp_id() })),
            "Page.createIsolatedWorld" => {
                let world_name = request
                    .params
                    .get("worldName")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let execution_context_id = self
                    .utility_execution_context_id
                    .get_or_insert(CDP_UTILITY_EXECUTION_CONTEXT_ID);
                let created_world = if self.utility_world_name.is_none() {
                    self.utility_world_name = Some(world_name.clone());
                    true
                } else {
                    false
                };
                if self.runtime_enabled && created_world {
                    let snapshot = state.page_snapshot();
                    events.push(event_message(
                        "Runtime.executionContextCreated",
                        json!({
                            "context": execution_context_payload(
                                *execution_context_id,
                                &snapshot.url,
                                &snapshot.frame_id,
                                &world_name,
                                false,
                            )
                        }),
                        response_session_id.as_deref(),
                    ));
                }
                Ok(json!({ "executionContextId": execution_context_id }))
            }
            "Runtime.enable" => {
                self.runtime_enabled = true;
                if let Some(event) = self.current_execution_context_created_event(state) {
                    events.push(event);
                }
                if let Some(event) = self.current_utility_execution_context_created_event(state) {
                    events.push(event);
                }
                Ok(json!({}))
            }
            "Runtime.evaluate" => {
                let Some(expression) = request.params.get("expression").and_then(Value::as_str)
                else {
                    return Ok(vec![cdp_error_response(
                        request.id,
                        response_session_id.as_deref(),
                        String::from("`Runtime.evaluate` requires a string `expression` parameter"),
                    )]);
                };
                let value = state
                    .runtime
                    .evaluate_script(expression.to_owned(), SCRIPT_TIMEOUT)
                    .map_err(|error| format!("script evaluation failed: {error}"))?;
                let result = remote_object_payload(value);
                Ok(json!({
                    "result": result
                }))
            }
            "Runtime.callFunctionOn" => Err(String::from(
                "Runtime.callFunctionOn is not supported by formal-web yet",
            )),
            "Runtime.releaseObject" => Ok(json!({})),
            "Input.dispatchMouseEvent" => {
                let event_type = request
                    .params
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if event_type == "mouseReleased" {
                    let x = request
                        .params
                        .get("x")
                        .and_then(Value::as_f64)
                        .ok_or_else(|| String::from("`Input.dispatchMouseEvent` requires `x`"))?;
                    let y = request
                        .params
                        .get("y")
                        .and_then(Value::as_f64)
                        .ok_or_else(|| String::from("`Input.dispatchMouseEvent` requires `y`"))?;
                    state
                        .runtime
                        .click(x as f32, y as f32, AUTOMATION_TIMEOUT)
                        .map_err(|error| format!("mouse dispatch failed: {error}"))?;
                }
                Ok(json!({}))
            }
            "Input.dispatchKeyEvent" => Ok(json!({})),
            "DOM.getDocument" => {
                let snapshot = state.page_snapshot();
                Ok(json!({
                    "root": document_root_payload(&snapshot.url)
                }))
            }
            "DOM.querySelector" => {
                let node_id = request
                    .params
                    .get("nodeId")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| String::from("`DOM.querySelector` requires `nodeId`"))?;
                let selector = request
                    .params
                    .get("selector")
                    .and_then(Value::as_str)
                    .ok_or_else(|| String::from("`DOM.querySelector` requires `selector`"))?;
                let parent = self.node_locator(node_id)?;
                let locators = dom_query_paths(state, &parent, selector)?;
                let node_id = if let Some(locator) = locators.into_iter().next() {
                    self.register_locator(locator)
                } else {
                    0
                };
                Ok(json!({ "nodeId": node_id }))
            }
            "DOM.querySelectorAll" => {
                let node_id = request
                    .params
                    .get("nodeId")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| String::from("`DOM.querySelectorAll` requires `nodeId`"))?;
                let selector = request
                    .params
                    .get("selector")
                    .and_then(Value::as_str)
                    .ok_or_else(|| String::from("`DOM.querySelectorAll` requires `selector`"))?;
                let parent = self.node_locator(node_id)?;
                let node_ids = self.register_locators(dom_query_paths(state, &parent, selector)?);
                Ok(json!({ "nodeIds": node_ids }))
            }
            "DOM.performSearch" => {
                let query = request
                    .params
                    .get("query")
                    .and_then(Value::as_str)
                    .ok_or_else(|| String::from("`DOM.performSearch` requires `query`"))?;
                let node_ids = self.register_locators(dom_query_paths(
                    state,
                    &CdpNodeLocator::Document,
                    query,
                )?);
                let search_id = self.next_search_id();
                self.search_results.insert(search_id.clone(), node_ids);
                Ok(json!({
                    "searchId": search_id,
                    "resultCount": self.search_results[&search_id].len(),
                }))
            }
            "DOM.getSearchResults" => {
                let search_id = request
                    .params
                    .get("searchId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| String::from("`DOM.getSearchResults` requires `searchId`"))?;
                let from_index = request
                    .params
                    .get("fromIndex")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| String::from("`DOM.getSearchResults` requires `fromIndex`"))?
                    as usize;
                let to_index = request
                    .params
                    .get("toIndex")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| String::from("`DOM.getSearchResults` requires `toIndex`"))?
                    as usize;
                let Some(results) = self.search_results.get(search_id) else {
                    return Err(format!("unknown DOM search id `{search_id}`"));
                };
                let slice_end = to_index.min(results.len());
                let node_ids = if from_index >= slice_end {
                    Vec::new()
                } else {
                    results[from_index..slice_end].to_vec()
                };
                Ok(json!({ "nodeIds": node_ids }))
            }
            "DOM.discardSearchResults" => {
                if let Some(search_id) = request.params.get("searchId").and_then(Value::as_str) {
                    self.search_results.remove(search_id);
                }
                Ok(json!({}))
            }
            "DOM.describeNode" => {
                let node_id = request
                    .params
                    .get("nodeId")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| String::from("`DOM.describeNode` requires `nodeId`"))?;
                let snapshot = state.page_snapshot();
                if node_id == CDP_DOCUMENT_NODE_ID {
                    Ok(json!({
                        "node": document_root_payload(&snapshot.url)
                    }))
                } else {
                    let locator = self.node_locator(node_id)?;
                    let description = dom_describe_node(state, &locator)?;
                    Ok(json!({
                        "node": described_node_payload(node_id, &snapshot.frame_id, description)
                    }))
                }
            }
            "DOM.getBoxModel" => {
                let node_id = request
                    .params
                    .get("nodeId")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| String::from("`DOM.getBoxModel` requires `nodeId`"))?;
                let _ = self.node_locator(node_id)?;
                Err(String::from(
                    "DOM.getBoxModel is not supported by formal-web yet",
                ))
            }
            "DOM.getContentQuads" => {
                let node_id = request
                    .params
                    .get("nodeId")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| String::from("`DOM.getContentQuads` requires `nodeId`"))?;
                let _ = self.node_locator(node_id)?;
                Err(String::from(
                    "DOM.getContentQuads is not supported by formal-web yet",
                ))
            }
            "DOM.scrollIntoViewIfNeeded" => {
                let node_id = request
                    .params
                    .get("nodeId")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        String::from("`DOM.scrollIntoViewIfNeeded` requires `nodeId`")
                    })?;
                let _ = self.node_locator(node_id)?;
                Ok(json!({}))
            }
            "Accessibility.getPartialAXTree" => {
                let node_id = request
                    .params
                    .get("nodeId")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        String::from("`Accessibility.getPartialAXTree` requires `nodeId`")
                    })?;
                let locator = self.node_locator(node_id)?;
                Ok(json!({
                    "nodes": accessibility_partial_tree(state, &locator, node_id)?
                }))
            }
            "Accessibility.getFullAXTree" => Ok(json!({
                "nodes": accessibility_full_tree(state)?
            })),
            _ => Ok(json!({})),
        };

        let mut outgoing = Vec::new();
        if let Some(id) = request.id {
            match result {
                Ok(result) => outgoing.push(cdp_success_response(
                    id,
                    response_session_id.as_deref(),
                    result,
                )),
                Err(message) => outgoing.push(cdp_error_response(
                    Some(id),
                    response_session_id.as_deref(),
                    message,
                )),
            }
        }
        outgoing.extend(events);
        Ok(outgoing)
    }

    fn translate_event(&self, state: &CdpState, event: CdpEvent) -> Vec<Value> {
        match event {
            CdpEvent::NavigationCommitted {
                url,
                frame_id: _,
                timestamp,
            } => {
                let frame_id = state.page_target_id.clone();
                let mut outgoing = vec![target_info_changed_event(
                    state,
                    &url,
                    self.session_id.is_some(),
                )];
                let session_id = self.event_session_id();
                if matches!(self.route, CdpRoute::Browser) && session_id.is_none() {
                    return outgoing;
                }
                if self.runtime_enabled {
                    outgoing.push(event_message(
                        "Runtime.executionContextsCleared",
                        json!({}),
                        session_id,
                    ));
                    outgoing.push(event_message(
                        "Runtime.executionContextCreated",
                        json!({
                            "context": execution_context_payload(
                                CDP_EXECUTION_CONTEXT_ID,
                                &url,
                                &frame_id,
                                "",
                                true,
                            )
                        }),
                        session_id,
                    ));
                    if let (Some(world_name), Some(execution_context_id)) = (
                        self.utility_world_name.as_deref(),
                        self.utility_execution_context_id,
                    ) {
                        outgoing.push(event_message(
                            "Runtime.executionContextCreated",
                            json!({
                                "context": execution_context_payload(
                                    execution_context_id,
                                    &url,
                                    &frame_id,
                                    world_name,
                                    false,
                                )
                            }),
                            session_id,
                        ));
                    }
                }
                if self.page_enabled {
                    outgoing.push(event_message(
                        "Page.frameNavigated",
                        json!({
                            "frame": frame_payload(&url, &frame_id)
                        }),
                        session_id,
                    ));
                    outgoing.push(event_message(
                        "Page.loadEventFired",
                        json!({
                            "timestamp": timestamp
                        }),
                        session_id,
                    ));
                }
                outgoing
            }
        }
    }

    fn response_session_id(
        &self,
        method: &str,
        request_session_id: Option<String>,
    ) -> Option<String> {
        request_session_id.or_else(|| {
            if matches!(self.route, CdpRoute::Browser) && session_scoped_method(method) {
                self.session_id.clone()
            } else {
                None
            }
        })
    }

    fn event_session_id(&self) -> Option<&str> {
        match self.route {
            CdpRoute::Browser => self.session_id.as_deref(),
            CdpRoute::Page => None,
        }
    }

    fn current_frame_navigated_event(&self, state: &CdpState) -> Option<Value> {
        let snapshot = state.page_snapshot();
        let session_id = self.event_session_id();
        if matches!(self.route, CdpRoute::Browser) && session_id.is_none() {
            return None;
        }
        Some(event_message(
            "Page.frameNavigated",
            json!({
                "frame": frame_payload(&snapshot.url, &snapshot.frame_id)
            }),
            session_id,
        ))
    }

    fn current_execution_context_created_event(&self, state: &CdpState) -> Option<Value> {
        let snapshot = state.page_snapshot();
        let session_id = self.event_session_id();
        if matches!(self.route, CdpRoute::Browser) && session_id.is_none() {
            return None;
        }
        Some(event_message(
            "Runtime.executionContextCreated",
            json!({
                "context": execution_context_payload(
                    CDP_EXECUTION_CONTEXT_ID,
                    &snapshot.url,
                    &snapshot.frame_id,
                    "",
                    true,
                )
            }),
            session_id,
        ))
    }

    fn current_utility_execution_context_created_event(&self, state: &CdpState) -> Option<Value> {
        let snapshot = state.page_snapshot();
        let session_id = self.event_session_id();
        let (Some(world_name), Some(execution_context_id)) = (
            self.utility_world_name.as_deref(),
            self.utility_execution_context_id,
        ) else {
            return None;
        };
        if matches!(self.route, CdpRoute::Browser) && session_id.is_none() {
            return None;
        }
        Some(event_message(
            "Runtime.executionContextCreated",
            json!({
                "context": execution_context_payload(
                    execution_context_id,
                    &snapshot.url,
                    &snapshot.frame_id,
                    world_name,
                    false,
                )
            }),
            session_id,
        ))
    }
}

async fn run_cdp_server(
    listener: TcpListener,
    state: CdpState,
    mut shutdown: oneshot::Receiver<()>,
) {
    let listener = match tokio::net::TcpListener::from_std(listener) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("formal-web cdp listener init error: {error}");
            return;
        }
    };

    let (session_shutdown_tx, session_shutdown_rx) = watch::channel(false);
    let mut sessions = JoinSet::new();

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                if let Err(error) = session_shutdown_tx.send(true) {
                    eprintln!("[cdp] failed to signal session shutdown: {error}");
                }
                break;
            }
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _address)) => {
                        let task_state = state.clone();
                        let task_shutdown = session_shutdown_rx.clone();
                        sessions.spawn(async move {
                            if let Err(error) = handle_cdp_stream(stream, task_state, task_shutdown).await {
                                eprintln!("formal-web cdp connection error: {error}");
                            }
                        });
                    }
                    Err(error) => {
                        eprintln!("formal-web cdp accept error: {error}");
                        break;
                    }
                }
            }
        }
    }

    while let Some(joined) = sessions.join_next().await {
        if let Err(error) = joined {
            eprintln!("formal-web cdp session task error: {error}");
        }
    }
}

async fn handle_cdp_stream(
    mut stream: TcpStream,
    state: CdpState,
    shutdown: watch::Receiver<bool>,
) -> Result<(), String> {
    let Some(peeked_request) = peek_request(&stream).await? else {
        return Ok(());
    };

    if peeked_request.websocket_upgrade {
        match parse_route(&peeked_request.target, &state) {
            Some(route) => handle_websocket_connection(stream, state, shutdown, route).await,
            None => {
                write_json_response(
                    &mut stream,
                    &peeked_request.method,
                    404,
                    "Not Found",
                    json!({ "error": "unknown CDP websocket route" }),
                )
                .await
            }
        }
    } else {
        handle_http_connection(stream, &state).await
    }
}

async fn peek_request(stream: &TcpStream) -> Result<Option<PeekedRequest>, String> {
    let mut buffer = vec![0_u8; CDP_PEEK_LIMIT];
    loop {
        let bytes_peeked = match stream.peek(&mut buffer).await {
            Ok(bytes_peeked) => bytes_peeked,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                continue;
            }
            Err(error) => return Err(format!("failed to peek CDP request: {error}")),
        };
        if bytes_peeked == 0 {
            return Ok(None);
        }

        let header_bytes = &buffer[..bytes_peeked];
        if let Some(header_end) = find_header_terminator(header_bytes) {
            let header_text = String::from_utf8_lossy(&header_bytes[..header_end]);
            let mut lines = header_text.lines();
            let request_line = lines.next().unwrap_or_default();
            let mut request_parts = request_line.split_whitespace();
            let method = request_parts.next().unwrap_or("GET").to_owned();
            let target = request_parts.next().unwrap_or("/").to_owned();

            let mut upgrade_websocket = false;
            let mut connection_upgrade = false;
            for line in lines {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Some((name, value)) = trimmed.split_once(':') {
                    let name = name.trim();
                    let value = value.trim();
                    if name.eq_ignore_ascii_case("upgrade")
                        && value.eq_ignore_ascii_case("websocket")
                    {
                        upgrade_websocket = true;
                    }
                    if name.eq_ignore_ascii_case("connection")
                        && value.to_ascii_lowercase().contains("upgrade")
                    {
                        connection_upgrade = true;
                    }
                }
            }

            return Ok(Some(PeekedRequest {
                method,
                target,
                websocket_upgrade: upgrade_websocket && connection_upgrade,
            }));
        }

        if bytes_peeked == buffer.len() {
            return Err(String::from(
                "incoming CDP request headers exceeded the size limit",
            ));
        }
    }
}

fn parse_route(target: &str, state: &CdpState) -> Option<CdpRoute> {
    let path = target.split('?').next().unwrap_or(target);
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    match segments.as_slice() {
        ["devtools", "browser", browser_id] if *browser_id == state.browser_id => {
            Some(CdpRoute::Browser)
        }
        ["devtools", "page", page_target_id] if *page_target_id == state.page_target_id => {
            Some(CdpRoute::Page)
        }
        _ => None,
    }
}

async fn handle_http_connection(mut stream: TcpStream, state: &CdpState) -> Result<(), String> {
    let Some(request) = read_http_request_head(&mut stream).await? else {
        return Ok(());
    };

    let (status, status_text, body) = dispatch_http_request(&request, state);
    write_http_response_async(&mut stream, &request.method, status, status_text, &body).await
}

async fn read_http_request_head(stream: &mut TcpStream) -> Result<Option<HttpRequest>, String> {
    let mut buffer = vec![0_u8; CDP_PEEK_LIMIT];
    let mut total = 0usize;
    loop {
        let read = stream
            .read(&mut buffer[total..])
            .await
            .map_err(|error| format!("failed to read CDP HTTP request: {error}"))?;
        if read == 0 {
            if total == 0 {
                return Ok(None);
            }
            return Err(String::from(
                "unexpected EOF while reading CDP HTTP request headers",
            ));
        }
        total += read;
        if let Some(header_end) = find_header_terminator(&buffer[..total]) {
            let header_text = String::from_utf8_lossy(&buffer[..header_end]);
            let mut lines = header_text.lines();
            let request_line = lines.next().unwrap_or_default();
            let mut request_parts = request_line.split_whitespace();
            let method = request_parts.next().unwrap_or("GET").to_owned();
            let target = request_parts.next().unwrap_or("/").to_owned();
            return Ok(Some(HttpRequest {
                method,
                target,
                body: Vec::new(),
            }));
        }
        if total == buffer.len() {
            return Err(String::from(
                "incoming CDP HTTP headers exceeded the size limit",
            ));
        }
    }
}

fn dispatch_http_request(request: &HttpRequest, state: &CdpState) -> (u16, &'static str, Vec<u8>) {
    let path = request
        .target
        .split('?')
        .next()
        .unwrap_or(request.target.as_str());
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    match (request.method.as_str(), segments.as_slice()) {
        ("GET", ["json", "version"]) => (
            200,
            "OK",
            json_bytes(json!({
                "Browser": CDP_BROWSER_PRODUCT,
                "Protocol-Version": CDP_PROTOCOL_VERSION,
                "User-Agent": CDP_BROWSER_PRODUCT,
                "webSocketDebuggerUrl": state.browser_ws_url()
            })),
        ),
        ("GET", ["json"]) | ("GET", ["json", "list"]) => {
            let body = if let Some(snapshot) = state.active_page_snapshot() {
                json!([{
                    "description": "",
                    "devtoolsFrontendUrl": "",
                    "id": state.page_target_id,
                    "title": "formal-web",
                    "type": "page",
                    "url": snapshot.url,
                    "webSocketDebuggerUrl": state.page_ws_url()
                }])
            } else {
                json!([])
            };
            (200, "OK", json_bytes(body))
        }
        _ => (
            404,
            "Not Found",
            json_bytes(json!({
                "error": format!("unsupported CDP route {} {}", request.method, request.target)
            })),
        ),
    }
}

async fn handle_websocket_connection(
    stream: TcpStream,
    state: CdpState,
    mut shutdown: watch::Receiver<bool>,
    route: CdpRoute,
) -> Result<(), String> {
    let mut websocket = accept_async(stream)
        .await
        .map_err(|error| format!("failed to accept CDP websocket: {error}"))?;

    let (event_sender, event_receiver) = mpsc::channel();
    let event_receiver = std::sync::Mutex::new(event_receiver);
    state
        .runtime
        .set_cdp_event_sink(Some(event_sender), AUTOMATION_TIMEOUT)?;

    let mut connection = CdpConnectionState::new(route);
    let result = async {
        loop {
            for message in collect_cdp_event_messages(&connection, &event_receiver, &state) {
                send_cdp_message(&mut websocket, &message).await?;
            }
            if *shutdown.borrow() {
                return Ok(());
            }

            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        return Ok(());
                    }
                }
                _ = time::sleep(CDP_SOCKET_IO_TIMEOUT) => {
                    continue;
                }
                incoming = websocket.next() => {
                    match incoming {
                        Some(Ok(message)) => match message {
                            Message::Text(text) => {
                                let outgoing = match connection.handle_text(&state, text.as_ref()) {
                                    Ok(outgoing) => outgoing,
                                    Err(error) => {
                                        let fallback = cdp_error_response(
                                            raw_request_id(text.as_ref()),
                                            raw_response_session_id(&connection, text.as_ref()).as_deref(),
                                            error,
                                        );
                                        vec![fallback]
                                    }
                                };
                                for message in outgoing {
                                    send_cdp_message(&mut websocket, &message).await?;
                                }
                                for message in collect_cdp_event_messages(&connection, &event_receiver, &state) {
                                    send_cdp_message(&mut websocket, &message).await?;
                                }
                            }
                            Message::Ping(payload) => {
                                send_ws_message(&mut websocket, Message::Pong(payload)).await?;
                            }
                            Message::Close(_frame) => return Ok(()),
                            _ => {}
                        },
                        Some(Err(WebSocketError::Io(error))) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                            continue;
                        }
                        Some(Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed)) | None => {
                            return Ok(());
                        }
                        Some(Err(error)) => return Err(format!("failed to read CDP websocket message: {error}")),
                    }
                }
            }
        }
    }
    .await;

    if let Err(error) = state
        .runtime
        .set_cdp_event_sink(None, Duration::from_millis(250))
    {
        eprintln!("[cdp] failed to clear CDP event sink: {error}");
    }
    result
}

fn collect_cdp_event_messages(
    connection: &CdpConnectionState,
    event_receiver: &std::sync::Mutex<mpsc::Receiver<CdpEvent>>,
    state: &CdpState,
) -> Vec<Value> {
    let mut outgoing = Vec::new();
    let Ok(receiver) = event_receiver.lock() else {
        return outgoing;
    };
    while let Ok(event) = receiver.try_recv() {
        outgoing.extend(connection.translate_event(state, event));
    }
    outgoing
}

async fn send_ws_message(
    websocket: &mut WebSocketStream<TcpStream>,
    message: Message,
) -> Result<(), String> {
    websocket
        .send(message)
        .await
        .map_err(|error| format!("failed to send CDP websocket message: {error}"))
}

async fn send_cdp_message(
    websocket: &mut WebSocketStream<TcpStream>,
    message: &Value,
) -> Result<(), String> {
    let payload = serde_json::to_string(message)
        .map_err(|error| format!("failed to serialize CDP message: {error}"))?;
    send_ws_message(websocket, Message::Text(payload.into())).await
}

async fn write_json_response(
    stream: &mut TcpStream,
    method: &str,
    status: u16,
    status_text: &str,
    body: Value,
) -> Result<(), String> {
    let body = json_bytes(body);
    write_http_response_async(stream, method, status, status_text, &body).await
}

async fn write_http_response_async(
    stream: &mut TcpStream,
    method: &str,
    status: u16,
    status_text: &str,
    body: &[u8],
) -> Result<(), String> {
    let mut header = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if method.eq_ignore_ascii_case("OPTIONS") {
        header.push_str("Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\n");
    }
    header.push_str("\r\n");
    stream
        .write_all(header.as_bytes())
        .await
        .map_err(|error| format!("failed to write CDP HTTP response headers: {error}"))?;
    if !method.eq_ignore_ascii_case("HEAD") {
        stream
            .write_all(body)
            .await
            .map_err(|error| format!("failed to write CDP HTTP response body: {error}"))?;
    }
    stream
        .flush()
        .await
        .map_err(|error| format!("failed to flush CDP HTTP response: {error}"))
}

fn json_bytes(value: Value) -> Vec<u8> {
    serde_json::to_vec(&value).expect("cdp json response serialization should succeed")
}

fn new_cdp_id() -> String {
    Uuid::new_v4().to_string()
}

fn cdp_success_response(id: Value, session_id: Option<&str>, result: Value) -> Value {
    let mut response = json!({
        "id": id,
        "result": result,
    });
    if let Some(session_id) = session_id {
        response["sessionId"] = json!(session_id);
    }
    response
}

fn cdp_error_response(id: Option<Value>, session_id: Option<&str>, message: String) -> Value {
    let mut response = json!({
        "error": {
            "code": -32000,
            "message": message,
        }
    });
    if let Some(id) = id {
        response["id"] = id;
    }
    if let Some(session_id) = session_id {
        response["sessionId"] = json!(session_id);
    }
    response
}

fn event_message(method: &str, params: Value, session_id: Option<&str>) -> Value {
    let mut event = json!({
        "method": method,
        "params": params,
    });
    if let Some(session_id) = session_id {
        event["sessionId"] = json!(session_id);
    }
    event
}

fn target_created_event(state: &CdpState, attached: bool) -> Value {
    json!({
        "method": "Target.targetCreated",
        "params": {
            "targetInfo": state.target_info(attached)
        }
    })
}

fn attached_to_target_event(state: &CdpState, session_id: &str) -> Value {
    json!({
        "method": "Target.attachedToTarget",
        "params": {
            "sessionId": session_id,
            "targetInfo": state.target_info(true),
            "waitingForDebugger": false
        }
    })
}

fn target_info_changed_event(state: &CdpState, url: &str, attached: bool) -> Value {
    json!({
        "method": "Target.targetInfoChanged",
        "params": {
            "targetInfo": target_info_payload(state, url, attached)
        }
    })
}

fn target_info_payload(state: &CdpState, url: &str, attached: bool) -> Value {
    json!({
        "targetId": state.page_target_id,
        "type": "page",
        "title": "formal-web",
        "url": url,
        "attached": attached,
        "browserContextId": state.browser_context_id,
        "canAccessOpener": false,
    })
}

fn frame_payload(url: &str, frame_id: &str) -> Value {
    json!({
        "id": frame_id,
        "loaderId": frame_id,
        "url": url,
        "securityOrigin": url_origin(url),
        "mimeType": "text/html",
    })
}

fn execution_context_payload(
    execution_context_id: u64,
    url: &str,
    frame_id: &str,
    name: &str,
    is_default: bool,
) -> Value {
    json!({
        "id": execution_context_id,
        "origin": url_origin(url),
        "name": name,
        "auxData": {
            "isDefault": is_default,
            "type": "default",
            "frameId": frame_id,
        }
    })
}

fn document_root_payload(url: &str) -> Value {
    json!({
        "nodeId": CDP_DOCUMENT_NODE_ID,
        "backendNodeId": CDP_DOCUMENT_NODE_ID,
        "nodeType": 9,
        "nodeName": "#document",
        "localName": "",
        "nodeValue": "",
        "childNodeCount": 1,
        "documentURL": url,
        "baseURL": url,
        "xmlVersion": "",
    })
}

fn described_node_payload(node_id: u64, frame_id: &str, description: Value) -> Value {
    let mut object = description.as_object().cloned().unwrap_or_default();
    object.insert(String::from("nodeId"), json!(node_id));
    object.insert(String::from("backendNodeId"), json!(node_id));
    object.insert(String::from("frameId"), json!(frame_id));
    object
        .entry(String::from("attributes"))
        .or_insert_with(|| json!([]));
    Value::Object(object)
}

fn dom_query_paths(
    state: &CdpState,
    parent: &CdpNodeLocator,
    selector: &str,
) -> Result<Vec<CdpNodeLocator>, String> {
    let value = evaluate_cdp_script(state, dom_query_paths_script(parent, selector))?;
    let Some(paths) = value.as_array() else {
        return Err(String::from(
            "DOM selector query did not evaluate to an array",
        ));
    };

    paths
        .iter()
        .map(|path| {
            let Some(segments) = path.as_array() else {
                return Err(String::from("DOM selector path was not an array"));
            };
            let mut indices = Vec::with_capacity(segments.len());
            for segment in segments {
                let Some(index) = segment.as_u64() else {
                    return Err(String::from("DOM selector path segment was not numeric"));
                };
                indices.push(index as usize);
            }
            Ok(CdpNodeLocator::Path(indices))
        })
        .collect()
}

fn dom_describe_node(state: &CdpState, locator: &CdpNodeLocator) -> Result<Value, String> {
    let value = evaluate_cdp_script(state, dom_describe_node_script(locator))?;
    if value.is_null() {
        return Err(String::from("DOM node is no longer attached"));
    }
    if value.as_object().is_some() {
        Ok(value)
    } else {
        Err(String::from(
            "DOM node description did not evaluate to an object",
        ))
    }
}

fn accessibility_partial_tree(
    state: &CdpState,
    locator: &CdpNodeLocator,
    node_id: u64,
) -> Result<Value, String> {
    let value = evaluate_cdp_script(state, accessibility_partial_tree_script(locator, node_id))?;
    if value.is_null() {
        return Err(String::from("Accessibility partial tree returned null"));
    }
    if value.as_array().is_some() {
        Ok(value)
    } else {
        Err(String::from(
            "Accessibility partial tree did not evaluate to an array",
        ))
    }
}

fn accessibility_full_tree(state: &CdpState) -> Result<Value, String> {
    let value = evaluate_cdp_script(state, accessibility_full_tree_script())?;
    if value.as_array().is_some() {
        Ok(value)
    } else {
        Err(String::from(
            "Accessibility full tree did not evaluate to an array",
        ))
    }
}

fn evaluate_cdp_script(state: &CdpState, source: String) -> Result<Value, String> {
    state
        .runtime
        .evaluate_script(source, SCRIPT_TIMEOUT)
        .map_err(|error| format!("CDP script evaluation failed: {error}"))
}

fn dom_query_paths_script(parent: &CdpNodeLocator, selector: &str) -> String {
    let parent = locator_resolution_expression(parent);
    let selector = serde_json::to_string(selector).expect("selector serialization should succeed");
    format!(
        "(() => {{ const __formalWebCdpQueryPaths = true; const __formalWebCdpParent = {parent}; if (!__formalWebCdpParent || typeof __formalWebCdpParent.querySelectorAll !== 'function') return []; function __formalWebCdpPathFor(node) {{ const __formalWebCdpPath = []; let __formalWebCdpCurrent = node; while (__formalWebCdpCurrent && __formalWebCdpCurrent !== document) {{ const __formalWebCdpOwner = __formalWebCdpCurrent.parentNode; if (!__formalWebCdpOwner || !__formalWebCdpOwner.childNodes) return null; let __formalWebCdpIndex = -1; for (let __formalWebCdpOffset = 0; __formalWebCdpOffset < __formalWebCdpOwner.childNodes.length; __formalWebCdpOffset += 1) {{ if (__formalWebCdpOwner.childNodes[__formalWebCdpOffset] === __formalWebCdpCurrent) {{ __formalWebCdpIndex = __formalWebCdpOffset; break; }} }} if (__formalWebCdpIndex < 0) return null; __formalWebCdpPath.unshift(__formalWebCdpIndex); __formalWebCdpCurrent = __formalWebCdpOwner; }} return __formalWebCdpPath; }} const __formalWebCdpMatches = __formalWebCdpParent.querySelectorAll({selector}); const __formalWebCdpPaths = []; for (let __formalWebCdpIndex = 0; __formalWebCdpIndex < __formalWebCdpMatches.length; __formalWebCdpIndex += 1) {{ const __formalWebCdpPath = __formalWebCdpPathFor(__formalWebCdpMatches[__formalWebCdpIndex]); if (__formalWebCdpPath) __formalWebCdpPaths.push(__formalWebCdpPath); }} return __formalWebCdpPaths; }})()"
    )
}

fn dom_describe_node_script(locator: &CdpNodeLocator) -> String {
    let locator = locator_resolution_expression(locator);
    format!(
        "(() => {{ const __formalWebCdpDescribeNode = true; const __formalWebCdpNode = {locator}; if (!__formalWebCdpNode) return null; const __formalWebCdpAttributes = []; if (__formalWebCdpNode.nodeType === Node.ELEMENT_NODE && __formalWebCdpNode.attributes) {{ for (let __formalWebCdpIndex = 0; __formalWebCdpIndex < __formalWebCdpNode.attributes.length; __formalWebCdpIndex += 1) {{ const __formalWebCdpAttribute = __formalWebCdpNode.attributes[__formalWebCdpIndex]; if (__formalWebCdpAttribute) {{ __formalWebCdpAttributes.push(__formalWebCdpAttribute.name); __formalWebCdpAttributes.push(__formalWebCdpAttribute.value); }} }} }} return {{ nodeType: __formalWebCdpNode.nodeType, nodeName: __formalWebCdpNode.nodeName || '#document', localName: __formalWebCdpNode.localName || '', nodeValue: __formalWebCdpNode.nodeValue || '', childNodeCount: __formalWebCdpNode.childNodes ? __formalWebCdpNode.childNodes.length : 0, attributes: __formalWebCdpAttributes }}; }})()"
    )
}

fn accessibility_partial_tree_script(locator: &CdpNodeLocator, node_id: u64) -> String {
    let locator = locator_resolution_expression(locator);
    format!(
        "(() => {{ const __formalWebCdpGetPartialAxTree = true; function __formalWebCdpRoleFor(node) {{ if (!node) return 'generic'; if (node.nodeType === Node.DOCUMENT_NODE) return 'document'; if (node.nodeType !== Node.ELEMENT_NODE) return 'text'; const explicit = node.getAttribute('role'); if (explicit) return explicit; if (/^h[1-6]$/.test(node.localName)) return 'heading'; switch (node.localName) {{ case 'a': return node.hasAttribute('href') ? 'link' : 'generic'; case 'button': return 'button'; case 'iframe': return 'iframe'; case 'img': return 'image'; case 'input': return node.type === 'checkbox' ? 'checkbox' : 'textbox'; case 'main': return 'main'; default: return 'generic'; }} }} function __formalWebCdpNameFor(node) {{ if (!node || node.nodeType !== Node.ELEMENT_NODE) return ''; const ariaLabel = node.getAttribute('aria-label'); if (ariaLabel && ariaLabel.trim()) return ariaLabel.trim(); const alt = node.getAttribute('alt'); if (alt && alt.trim()) return alt.trim(); const title = node.getAttribute('title'); if (title && title.trim()) return title.trim(); const text = (node.innerText || node.textContent || '').replace(/\\s+/g, ' ').trim(); if (text) return text; if (typeof node.value === 'string' && node.value.trim()) return node.value.trim(); return ''; }} function __formalWebCdpDescribe(node, axNodeId, backendDOMNodeId) {{ return {{ nodeId: axNodeId, ignored: false, role: {{ type: 'internalRole', value: __formalWebCdpRoleFor(node) }}, name: {{ type: 'computedString', value: __formalWebCdpNameFor(node) }}, backendDOMNodeId: backendDOMNodeId, childIds: [] }}; }} const __formalWebCdpNode = {locator}; if (!__formalWebCdpNode) return []; const __formalWebCdpNodes = []; let __formalWebCdpCurrent = __formalWebCdpNode; let __formalWebCdpDepth = 0; while (__formalWebCdpCurrent && __formalWebCdpDepth < 8) {{ __formalWebCdpNodes.push(__formalWebCdpDescribe(__formalWebCdpCurrent, 'ax-{node_id}-' + __formalWebCdpDepth, __formalWebCdpDepth === 0 ? {node_id} : 0)); __formalWebCdpCurrent = __formalWebCdpCurrent.parentElement; __formalWebCdpDepth += 1; }} return __formalWebCdpNodes; }})()"
    )
}

fn accessibility_full_tree_script() -> String {
    String::from(
        "(() => { const __formalWebCdpGetFullAxTree = true; const __formalWebCdpRoot = document.body || document.documentElement; if (!__formalWebCdpRoot) return []; function __formalWebCdpRoleFor(node) { if (!node) return 'generic'; if (node.nodeType !== Node.ELEMENT_NODE) return 'text'; const explicit = node.getAttribute('role'); if (explicit) return explicit; if (/^h[1-6]$/.test(node.localName)) return 'heading'; switch (node.localName) { case 'a': return node.hasAttribute('href') ? 'link' : 'generic'; case 'button': return 'button'; case 'iframe': return 'iframe'; case 'img': return 'image'; case 'input': return node.type === 'checkbox' ? 'checkbox' : 'textbox'; case 'main': return 'main'; default: return 'generic'; } } function __formalWebCdpNameFor(node) { if (!node || node.nodeType !== Node.ELEMENT_NODE) return ''; const ariaLabel = node.getAttribute('aria-label'); if (ariaLabel && ariaLabel.trim()) return ariaLabel.trim(); const alt = node.getAttribute('alt'); if (alt && alt.trim()) return alt.trim(); const title = node.getAttribute('title'); if (title && title.trim()) return title.trim(); const text = (node.innerText || node.textContent || '').replace(/\\s+/g, ' ').trim(); if (text) return text; if (typeof node.value === 'string' && node.value.trim()) return node.value.trim(); return ''; } function __formalWebCdpDescribe(node, index) { return { nodeId: 'ax-full-' + index, ignored: false, role: { type: 'internalRole', value: __formalWebCdpRoleFor(node) }, name: { type: 'computedString', value: __formalWebCdpNameFor(node) }, backendDOMNodeId: 0, childIds: [] }; } const __formalWebCdpNodes = []; const __formalWebCdpCandidates = __formalWebCdpRoot.querySelectorAll('*'); let __formalWebCdpIndex = 0; if (__formalWebCdpRoleFor(__formalWebCdpRoot) !== 'generic' || __formalWebCdpNameFor(__formalWebCdpRoot)) { __formalWebCdpNodes.push(__formalWebCdpDescribe(__formalWebCdpRoot, __formalWebCdpIndex)); __formalWebCdpIndex += 1; } for (let __formalWebCdpOffset = 0; __formalWebCdpOffset < __formalWebCdpCandidates.length; __formalWebCdpOffset += 1) { const __formalWebCdpNode = __formalWebCdpCandidates[__formalWebCdpOffset]; const __formalWebCdpRole = __formalWebCdpRoleFor(__formalWebCdpNode); const __formalWebCdpName = __formalWebCdpNameFor(__formalWebCdpNode); if (__formalWebCdpRole === 'generic' && !__formalWebCdpName) continue; __formalWebCdpNodes.push(__formalWebCdpDescribe(__formalWebCdpNode, __formalWebCdpIndex)); __formalWebCdpIndex += 1; if (__formalWebCdpNodes.length >= 128) break; } return __formalWebCdpNodes; })()",
    )
}

fn locator_resolution_expression(locator: &CdpNodeLocator) -> String {
    match locator {
        CdpNodeLocator::Document => String::from("document"),
        CdpNodeLocator::Path(path) => {
            let path = serde_json::to_string(path).expect("path serialization should succeed");
            format!(
                "(() => {{ let __formalWebCdpNode = document; for (const __formalWebCdpIndex of {path}) {{ if (!__formalWebCdpNode || !__formalWebCdpNode.childNodes) return null; __formalWebCdpNode = __formalWebCdpNode.childNodes[__formalWebCdpIndex]; }} return __formalWebCdpNode; }})()"
            )
        }
    }
}

fn url_origin(url: &str) -> String {
    if url == "about:blank" {
        return String::from("null");
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return String::from("null");
    };
    let authority = rest.split('/').next().unwrap_or_default();
    format!("{scheme}://{authority}")
}

fn remote_object_payload(value: Value) -> Value {
    match value {
        Value::Null => json!({
            "type": "object",
            "subtype": "null",
            "value": Value::Null,
        }),
        Value::Bool(value) => json!({
            "type": "boolean",
            "value": value,
        }),
        Value::Number(value) => json!({
            "type": "number",
            "value": value,
        }),
        Value::String(value) => json!({
            "type": "string",
            "value": value,
        }),
        Value::Array(value) => json!({
            "type": "object",
            "subtype": "array",
            "value": value,
            "description": "Array",
        }),
        Value::Object(value) => json!({
            "type": "object",
            "value": value,
            "description": "Object",
        }),
    }
}

fn raw_request_id(text: &str) -> Option<Value> {
    let parsed = serde_json::from_str::<Value>(text).ok()?;
    parsed.get("id").cloned()
}

fn raw_response_session_id(connection: &CdpConnectionState, text: &str) -> Option<String> {
    let parsed = serde_json::from_str::<Value>(text).ok();
    let request_session_id = parsed
        .as_ref()
        .and_then(|value| value.get("sessionId"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let method = parsed
        .as_ref()
        .and_then(|value| value.get("method"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    connection.response_session_id(method, request_session_id)
}

fn session_scoped_method(method: &str) -> bool {
    method.starts_with("Page.")
        || method.starts_with("Runtime.")
        || method.starts_with("Input.")
        || method.starts_with("DOM.")
        || method.starts_with("Accessibility.")
}

#[cfg(test)]
mod tests {
    use super::{
        CDP_BROWSER_PRODUCT, CDP_PROTOCOL_VERSION, CdpEvent, CdpServerHandle,
        find_header_terminator,
    };
    use crate::{
        AutomationCommand, AutomationRuntime, AutomationSnapshot, AutomationVisibleFrameViewport,
    };
    use base64::Engine as _;
    use ipc_messages::content::{NavigableId, WebviewId};
    use serde_json::{Value, json};
    use std::io::{ErrorKind, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;
    use std::time::Duration;
    use tungstenite::{Error as WebSocketError, Message, WebSocket, client::client};

    const TEST_TIMEOUT: Duration = Duration::from_secs(2);
    const RETRY_DELAY: Duration = Duration::from_millis(10);

    struct MockRuntimeState {
        snapshot: AutomationSnapshot,
        screenshot: Vec<u8>,
        evaluation_result: Value,
        last_script: Option<String>,
        event_sink: Option<mpsc::Sender<CdpEvent>>,
    }

    #[test]
    fn discovery_endpoints_expose_browser_and_page_targets() {
        let (_server, _state, port) = start_test_server();

        let version = http_get_json(port, "/json/version");
        assert_eq!(version["Browser"], CDP_BROWSER_PRODUCT);
        assert_eq!(version["Protocol-Version"], CDP_PROTOCOL_VERSION);
        let browser_ws_url = version["webSocketDebuggerUrl"]
            .as_str()
            .expect("browser websocket url should be a string");
        assert!(browser_ws_url.starts_with(&format!("ws://localhost:{port}/devtools/browser/")));

        let targets = http_get_json(port, "/json")
            .as_array()
            .cloned()
            .expect("CDP target list should be an array");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0]["type"], "page");
        assert_eq!(targets[0]["title"], "formal-web");
        assert_eq!(targets[0]["url"], "about:blank");
        let page_ws_url = targets[0]["webSocketDebuggerUrl"]
            .as_str()
            .expect("page websocket url should be a string");
        assert!(page_ws_url.starts_with(&format!("ws://localhost:{port}/devtools/page/")));
    }

    #[test]
    fn browser_websocket_supports_minimum_session_flow() {
        let (_server, state, port) = start_test_server();
        let version = http_get_json(port, "/json/version");
        let targets = http_get_json(port, "/json")
            .as_array()
            .cloned()
            .expect("CDP target list should be an array");
        let browser_ws_url = version["webSocketDebuggerUrl"]
            .as_str()
            .expect("browser websocket url should be a string")
            .to_owned();
        let page_target_id = targets[0]["id"]
            .as_str()
            .expect("page target id should be a string")
            .to_owned();
        let mut socket = connect_websocket(&browser_ws_url, port);
        let mut next_id = 1_u64;

        let (browser_version, browser_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Browser.getVersion",
            json!({}),
            None,
        );
        assert!(browser_events.is_empty());
        assert_eq!(browser_version["result"]["product"], CDP_BROWSER_PRODUCT);
        assert_eq!(
            browser_version["result"]["protocolVersion"],
            CDP_PROTOCOL_VERSION
        );

        let (targets_response, target_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Target.getTargets",
            json!({}),
            None,
        );
        assert!(target_events.is_empty());
        assert_eq!(
            targets_response["result"]["targetInfos"][0]["targetId"],
            page_target_id
        );
        assert_eq!(
            targets_response["result"]["targetInfos"][0]["attached"],
            false
        );

        let (attach_response, attach_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Target.attachToTarget",
            json!({ "targetId": page_target_id, "flatten": true }),
            None,
        );
        assert!(attach_events.is_empty());
        let session_id = attach_response["result"]["sessionId"]
            .as_str()
            .expect("session id should be a string")
            .to_owned();

        let (page_enable, page_enable_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Page.enable",
            json!({}),
            Some(&session_id),
        );
        assert_eq!(page_enable["sessionId"], session_id);
        assert_event_methods(&page_enable_events, &["Page.frameNavigated"]);
        assert_eq!(page_enable_events[0]["sessionId"], session_id);
        assert_eq!(
            page_enable_events[0]["params"]["frame"]["id"],
            page_target_id
        );

        let (runtime_enable, runtime_enable_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Runtime.enable",
            json!({}),
            Some(&session_id),
        );
        assert_eq!(runtime_enable["sessionId"], session_id);
        assert_event_methods(&runtime_enable_events, &["Runtime.executionContextCreated"]);
        assert_eq!(runtime_enable_events[0]["sessionId"], session_id);

        let navigation_url = "https://example.com/formal-web";
        let (navigate_response, navigate_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Page.navigate",
            json!({ "url": navigation_url }),
            Some(&session_id),
        );
        assert_eq!(navigate_response["sessionId"], session_id);
        assert_eq!(navigate_response["result"]["frameId"], page_target_id);
        assert_event_methods(
            &navigate_events,
            &[
                "Target.targetInfoChanged",
                "Runtime.executionContextsCleared",
                "Runtime.executionContextCreated",
                "Page.frameNavigated",
                "Page.loadEventFired",
            ],
        );
        assert!(navigate_events.iter().any(|event| {
            event["method"] == "Page.frameNavigated"
                && event["params"]["frame"]["url"] == navigation_url
                && event["params"]["frame"]["id"] == page_target_id
                && event["sessionId"] == session_id
        }));
        assert!(navigate_events.iter().any(|event| {
            event["method"] == "Page.loadEventFired"
                && event["params"]["timestamp"] == json!(1234.5)
                && event["sessionId"] == session_id
        }));

        let (frame_tree, frame_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Page.getFrameTree",
            json!({}),
            Some(&session_id),
        );
        assert!(frame_events.is_empty());
        assert_eq!(
            frame_tree["result"]["frameTree"]["frame"]["url"],
            navigation_url
        );

        let (document_response, document_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "DOM.getDocument",
            json!({}),
            Some(&session_id),
        );
        assert!(document_events.is_empty());
        assert_eq!(
            document_response["result"]["root"]["documentURL"],
            navigation_url
        );
        let root_node_id = document_response["result"]["root"]["nodeId"]
            .as_u64()
            .expect("document root node id should be numeric");

        let (query_response, query_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "DOM.querySelector",
            json!({
                "nodeId": root_node_id,
                "selector": "#click-counter-button"
            }),
            Some(&session_id),
        );
        assert!(query_events.is_empty());
        let button_node_id = query_response["result"]["nodeId"]
            .as_u64()
            .expect("queried button node id should be numeric");
        assert!(button_node_id > root_node_id);

        let (query_all_response, query_all_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "DOM.querySelectorAll",
            json!({
                "nodeId": root_node_id,
                "selector": "button"
            }),
            Some(&session_id),
        );
        assert!(query_all_events.is_empty());
        let query_all_nodes = query_all_response["result"]["nodeIds"]
            .as_array()
            .expect("querySelectorAll should return an array of node ids");
        assert_eq!(query_all_nodes.len(), 2);
        assert_eq!(query_all_nodes[0], button_node_id);

        let (search_response, search_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "DOM.performSearch",
            json!({ "query": "button" }),
            Some(&session_id),
        );
        assert!(search_events.is_empty());
        assert_eq!(search_response["result"]["resultCount"], 2);
        let search_id = search_response["result"]["searchId"]
            .as_str()
            .expect("search id should be a string")
            .to_owned();

        let (search_results_response, search_results_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "DOM.getSearchResults",
            json!({
                "searchId": search_id,
                "fromIndex": 0,
                "toIndex": 10
            }),
            Some(&session_id),
        );
        assert!(search_results_events.is_empty());
        assert_eq!(
            search_results_response["result"]["nodeIds"]
                .as_array()
                .expect("search results should be an array")
                .len(),
            2
        );

        let (describe_response, describe_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "DOM.describeNode",
            json!({ "nodeId": button_node_id }),
            Some(&session_id),
        );
        assert!(describe_events.is_empty());
        assert_eq!(describe_response["result"]["node"]["nodeName"], "BUTTON");
        assert_eq!(describe_response["result"]["node"]["localName"], "button");
        assert!(
            describe_response["result"]["node"]["attributes"]
                .as_array()
                .is_some_and(|attributes| {
                    attributes
                        .iter()
                        .any(|value| value == "click-counter-button")
                })
        );

        let (box_model_response, box_model_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "DOM.getBoxModel",
            json!({ "nodeId": button_node_id }),
            Some(&session_id),
        );
        assert!(box_model_events.is_empty());
        assert_eq!(
            box_model_response["error"]["message"],
            "DOM.getBoxModel is not supported by formal-web yet"
        );

        let (content_quads_response, content_quads_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "DOM.getContentQuads",
            json!({ "nodeId": button_node_id }),
            Some(&session_id),
        );
        assert!(content_quads_events.is_empty());
        assert_eq!(
            content_quads_response["error"]["message"],
            "DOM.getContentQuads is not supported by formal-web yet"
        );

        let (scroll_response, scroll_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "DOM.scrollIntoViewIfNeeded",
            json!({ "nodeId": button_node_id }),
            Some(&session_id),
        );
        assert!(scroll_events.is_empty());
        assert_eq!(scroll_response["result"], json!({}));

        let (partial_ax_response, partial_ax_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Accessibility.getPartialAXTree",
            json!({ "nodeId": button_node_id }),
            Some(&session_id),
        );
        assert!(partial_ax_events.is_empty());
        assert_eq!(
            partial_ax_response["result"]["nodes"][0]["role"]["value"],
            "button"
        );
        assert_eq!(
            partial_ax_response["result"]["nodes"][0]["name"]["value"],
            "Count interactions"
        );

        let (full_ax_response, full_ax_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Accessibility.getFullAXTree",
            json!({}),
            Some(&session_id),
        );
        assert!(full_ax_events.is_empty());
        assert!(
            full_ax_response["result"]["nodes"]
                .as_array()
                .is_some_and(|nodes| nodes.iter().any(|node| {
                    node["role"]["value"] == "button"
                        && node["name"]["value"] == "Count interactions"
                }))
        );

        let (evaluate_response, evaluate_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Runtime.evaluate",
            json!({ "expression": "1 + 1" }),
            Some(&session_id),
        );
        assert!(evaluate_events.is_empty());
        assert_eq!(evaluate_response["result"]["result"]["value"], 2);
        assert_eq!(
            state
                .lock()
                .expect("mock runtime mutex poisoned")
                .last_script
                .as_deref(),
            Some("1 + 1")
        );

        let (screenshot_response, screenshot_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Page.captureScreenshot",
            json!({}),
            Some(&session_id),
        );
        assert!(screenshot_events.is_empty());
        assert_eq!(
            screenshot_response["result"]["data"],
            base64::engine::general_purpose::STANDARD.encode([0_u8, 1, 2, 3])
        );

        let (unknown_response, unknown_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "FormalWeb.unknownMethod",
            json!({}),
            Some(&session_id),
        );
        assert!(unknown_events.is_empty());
        assert_eq!(unknown_response["result"], json!({}));

        if let Err(error) = socket.close(None) {
            eprintln!("[cdp-test] failed to close test socket: {error}");
        }
    }

    fn start_test_server() -> (CdpServerHandle, Arc<Mutex<MockRuntimeState>>, u16) {
        let port = free_local_port();
        let state = Arc::new(Mutex::new(MockRuntimeState {
            snapshot: automation_snapshot("about:blank", NavigableId::from_u128(1)),
            screenshot: vec![0_u8, 1, 2, 3],
            evaluation_result: json!(2),
            last_script: None,
            event_sink: None,
        }));
        let runtime = mock_runtime(Arc::clone(&state));
        let server = CdpServerHandle::start(port, runtime).expect("test CDP server should start");
        (server, state, port)
    }

    fn mock_runtime(state: Arc<Mutex<MockRuntimeState>>) -> AutomationRuntime {
        let send_state = Arc::clone(&state);
        AutomationRuntime::new(
            move |command| {
                let mut state = send_state.lock().expect("mock runtime mutex poisoned");
                match command {
                    AutomationCommand::Snapshot { reply } => {
                        let _ = reply.send(Ok(state.snapshot.clone()));
                    }
                    AutomationCommand::VisibleFrameViewports { reply } => {
                        let _ = reply.send(Ok(Vec::<AutomationVisibleFrameViewport>::new()));
                    }
                    AutomationCommand::Screenshot { reply } => {
                        let _ = reply.send(Ok(state.screenshot.clone()));
                    }
                    AutomationCommand::Navigate { url, reply } => {
                        let frame_id = state
                            .snapshot
                            .navigable_id
                            .expect("mock snapshot should have a frame id");
                        state.snapshot.current_url = Some(url.clone());
                        state.snapshot.displayed_url = url.clone();
                        if let Some(sink) = state.event_sink.as_ref() {
                            let _ = sink.send(CdpEvent::NavigationCommitted {
                                url,
                                frame_id: frame_id.to_string(),
                                timestamp: 1234.5,
                            });
                        }
                        let _ = reply.send(Ok(state.snapshot.clone()));
                    }
                    AutomationCommand::Click { reply, .. }
                    | AutomationCommand::ClickElement { reply, .. }
                    | AutomationCommand::Scroll { reply, .. } => {
                        let _ = reply.send(Ok(()));
                    }
                    AutomationCommand::EvaluateScript { source, reply, .. } => {
                        state.last_script = Some(source.clone());
                        let _ = reply.send(Ok(mock_evaluate_script_result(
                            &source,
                            &state.evaluation_result,
                        )));
                    }
                    AutomationCommand::SetCdpEventSink { sink, reply } => {
                        state.event_sink = sink;
                        let _ = reply.send(Ok(()));
                    }
                }
                Ok(())
            },
            || Ok(()),
            || true,
        )
    }

    fn mock_evaluate_script_result(source: &str, fallback: &Value) -> Value {
        if source.contains("__formalWebCdpQueryPaths") {
            if source.contains("\"#click-counter-button\"") {
                return json!([[1, 3, 9]]);
            }
            if source.contains("\"button\"") {
                return json!([[1, 3, 9], [1, 3, 11]]);
            }
            return json!([]);
        }
        if source.contains("__formalWebCdpDescribeNode") {
            return json!({
                "nodeType": 1,
                "nodeName": "BUTTON",
                "localName": "button",
                "nodeValue": "",
                "childNodeCount": 1,
                "attributes": ["id", "click-counter-button", "class", "counter-button"],
            });
        }
        if source.contains("__formalWebCdpGetPartialAxTree") {
            return json!([
                {
                    "nodeId": "ax-2-0",
                    "ignored": false,
                    "role": { "type": "internalRole", "value": "button" },
                    "name": { "type": "computedString", "value": "Count interactions" },
                    "backendDOMNodeId": 2,
                    "childIds": [],
                },
                {
                    "nodeId": "ax-2-1",
                    "ignored": false,
                    "role": { "type": "internalRole", "value": "main" },
                    "name": { "type": "computedString", "value": "" },
                    "backendDOMNodeId": 0,
                    "childIds": [],
                }
            ]);
        }
        if source.contains("__formalWebCdpGetFullAxTree") {
            return json!([
                {
                    "nodeId": "ax-full-0",
                    "ignored": false,
                    "role": { "type": "internalRole", "value": "heading" },
                    "name": { "type": "computedString", "value": "FormalWeb Fetch Bootstrap" },
                    "backendDOMNodeId": 0,
                    "childIds": [],
                },
                {
                    "nodeId": "ax-full-1",
                    "ignored": false,
                    "role": { "type": "internalRole", "value": "button" },
                    "name": { "type": "computedString", "value": "Count interactions" },
                    "backendDOMNodeId": 0,
                    "childIds": [],
                }
            ]);
        }
        fallback.clone()
    }

    #[test]
    fn navigation_recreates_utility_world_execution_context() {
        let (_server, _state, port) = start_test_server();
        let version = http_get_json(port, "/json/version");
        let targets = http_get_json(port, "/json")
            .as_array()
            .cloned()
            .expect("CDP target list should be an array");
        let browser_ws_url = version["webSocketDebuggerUrl"]
            .as_str()
            .expect("browser websocket url should be a string")
            .to_owned();
        let page_target_id = targets[0]["id"]
            .as_str()
            .expect("page target id should be a string")
            .to_owned();
        let mut socket = connect_websocket(&browser_ws_url, port);
        let mut next_id = 1_u64;

        let (attach_response, attach_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Target.attachToTarget",
            json!({ "targetId": page_target_id, "flatten": true }),
            None,
        );
        assert!(attach_events.is_empty());
        let session_id = attach_response["result"]["sessionId"]
            .as_str()
            .expect("session id should be a string")
            .to_owned();

        let (_page_enable, page_enable_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Page.enable",
            json!({}),
            Some(&session_id),
        );
        assert_event_methods(&page_enable_events, &["Page.frameNavigated"]);
        assert_eq!(
            page_enable_events[0]["params"]["frame"]["id"],
            page_target_id
        );

        let (_runtime_enable, runtime_enable_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Runtime.enable",
            json!({}),
            Some(&session_id),
        );
        assert_event_methods(&runtime_enable_events, &["Runtime.executionContextCreated"]);

        let (_world_response, world_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Page.createIsolatedWorld",
            json!({
                "frameId": "ignored-by-test",
                "worldName": "__formal_web_utility_world"
            }),
            Some(&session_id),
        );
        assert_event_methods(&world_events, &["Runtime.executionContextCreated"]);
        assert_eq!(
            world_events[0]["params"]["context"]["name"],
            "__formal_web_utility_world"
        );
        assert_eq!(
            world_events[0]["params"]["context"]["auxData"]["frameId"],
            page_target_id
        );

        let navigation_url = "https://example.com/utility-world";
        let (navigate_response, navigate_events) = send_cdp_request(
            &mut socket,
            &mut next_id,
            "Page.navigate",
            json!({ "url": navigation_url }),
            Some(&session_id),
        );
        assert_eq!(navigate_response["result"]["frameId"], page_target_id);
        assert_event_methods(
            &navigate_events,
            &[
                "Target.targetInfoChanged",
                "Runtime.executionContextsCleared",
                "Runtime.executionContextCreated",
                "Runtime.executionContextCreated",
                "Page.frameNavigated",
                "Page.loadEventFired",
            ],
        );
        assert_eq!(
            navigate_events[3]["params"]["context"]["name"],
            "__formal_web_utility_world"
        );
        assert_eq!(
            navigate_events[2]["params"]["context"]["auxData"]["frameId"],
            page_target_id
        );
        assert_eq!(
            navigate_events[3]["params"]["context"]["auxData"]["frameId"],
            page_target_id
        );
        assert_eq!(
            navigate_events[3]["params"]["context"]["auxData"]["isDefault"],
            false
        );
        assert_eq!(navigate_events[4]["params"]["frame"]["id"], page_target_id);

        if let Err(error) = socket.close(None) {
            eprintln!("[cdp-test] failed to close test socket: {error}");
        }
    }

    fn automation_snapshot(url: &str, navigable_id: NavigableId) -> AutomationSnapshot {
        AutomationSnapshot {
            webview_id: Some(WebviewId(navigable_id)),
            current_url: Some(url.to_owned()),
            displayed_url: url.to_owned(),
            navigable_id: Some(navigable_id),
            has_top_level_traversable: true,
        }
    }

    fn free_local_port() -> u16 {
        TcpListener::bind(("127.0.0.1", 0))
            .expect("ephemeral port bind should succeed")
            .local_addr()
            .expect("ephemeral port address should be available")
            .port()
    }

    fn http_get_json(port: u16, path: &str) -> Value {
        let body = http_get(port, path);
        serde_json::from_slice(&body).expect("HTTP response body should be valid JSON")
    }

    fn http_get(port: u16, path: &str) -> Vec<u8> {
        let mut last_error = None;
        for _attempt in 0..50 {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(mut stream) => {
                    stream
                        .set_read_timeout(Some(TEST_TIMEOUT))
                        .expect("HTTP read timeout should be configurable");
                    let request = format!(
                        "GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n"
                    );
                    stream
                        .write_all(request.as_bytes())
                        .expect("HTTP request should write successfully");
                    let mut response = Vec::new();
                    stream
                        .read_to_end(&mut response)
                        .expect("HTTP response should be readable");
                    let header_end = find_header_terminator(&response)
                        .expect("HTTP response should contain a header terminator");
                    let header_text = String::from_utf8_lossy(&response[..header_end]);
                    assert!(header_text.starts_with("HTTP/1.1 200 OK"));
                    return response[header_end..].to_vec();
                }
                Err(error) if error.kind() == ErrorKind::ConnectionRefused => {
                    last_error = Some(error);
                    thread::sleep(RETRY_DELAY);
                }
                Err(error) => panic!("unexpected HTTP connect failure: {error}"),
            }
        }

        panic!(
            "timed out connecting to test CDP server: {}",
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| String::from("no connection attempts were made"))
        );
    }

    fn connect_websocket(url: &str, port: u16) -> WebSocket<TcpStream> {
        let mut last_error = None;
        for _attempt in 0..50 {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => {
                    stream
                        .set_read_timeout(Some(TEST_TIMEOUT))
                        .expect("websocket read timeout should be configurable");
                    stream
                        .set_write_timeout(Some(TEST_TIMEOUT))
                        .expect("websocket write timeout should be configurable");
                    return client(url, stream)
                        .expect("websocket handshake should succeed")
                        .0;
                }
                Err(error) if error.kind() == ErrorKind::ConnectionRefused => {
                    last_error = Some(error);
                    thread::sleep(RETRY_DELAY);
                }
                Err(error) => panic!("unexpected websocket connect failure: {error}"),
            }
        }

        panic!(
            "timed out connecting websocket to test CDP server: {}",
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| String::from("no connection attempts were made"))
        );
    }

    fn send_cdp_request(
        socket: &mut WebSocket<TcpStream>,
        next_id: &mut u64,
        method: &str,
        params: Value,
        session_id: Option<&str>,
    ) -> (Value, Vec<Value>) {
        let id = *next_id;
        *next_id += 1;
        let mut request = json!({
            "id": id,
            "method": method,
            "params": params,
        });
        if let Some(session_id) = session_id {
            request["sessionId"] = json!(session_id);
        }
        socket
            .send(Message::Text(request.to_string().into()))
            .expect("CDP request should send successfully");

        let mut events = Vec::new();
        let response = loop {
            match socket.read() {
                Ok(Message::Text(text)) => {
                    let message: Value = serde_json::from_str(text.as_ref())
                        .expect("CDP websocket payload should be valid JSON");
                    if message["id"].as_u64() == Some(id) {
                        break message;
                    }
                    events.push(message);
                }
                Ok(Message::Ping(payload)) => {
                    socket
                        .send(Message::Pong(payload))
                        .expect("CDP pong should send successfully");
                }
                Ok(Message::Close(_)) => panic!("CDP websocket closed while waiting for {method}"),
                Ok(_) => {}
                Err(error) => panic!("failed to read CDP response for {method}: {error}"),
            }
        };
        events.extend(drain_websocket_messages(socket));
        (response, events)
    }

    fn drain_websocket_messages(socket: &mut WebSocket<TcpStream>) -> Vec<Value> {
        let mut messages = Vec::new();
        loop {
            match socket.read() {
                Ok(Message::Text(text)) => messages.push(
                    serde_json::from_str(text.as_ref())
                        .expect("CDP websocket payload should be valid JSON"),
                ),
                Ok(Message::Ping(payload)) => {
                    socket
                        .send(Message::Pong(payload))
                        .expect("CDP pong should send successfully");
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(WebSocketError::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    break;
                }
                Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed) => break,
                Err(error) => panic!("failed to drain CDP websocket messages: {error}"),
            }
        }
        messages
    }

    fn assert_event_methods(events: &[Value], expected_methods: &[&str]) {
        let methods = events
            .iter()
            .filter_map(|event| event["method"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(methods, expected_methods);
    }
}
