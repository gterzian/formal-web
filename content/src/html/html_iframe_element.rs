use ipc_messages::content::{
    ContentNavigableId, CreateChildNavigableRequest,
    Event as ContentEvent, FrameId, IframeTraversableRemoval, NavigableId, NavigateRequest, NavigationId,
    UserNavigationInvolvement, iframe_target_name,
};
use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};
use html5ever::{local_name, ns};
use url::Url;

use crate::{
    ContentRuntime, EMPTY_HTML_DOCUMENT, NavigableContainerState, dom::fire_event,
    html::HTMLElement,
};

/// <https://html.spec.whatwg.org/#htmliframeelement>
#[derive(Trace, Finalize, JsData)]
pub struct HTMLIFrameElement {
    /// <https://html.spec.whatwg.org/#htmlelement>
    pub html_element: HTMLElement,
}

impl HTMLIFrameElement {
    pub fn new(document: Rc<RefCell<BaseDocument>>, node_id: usize) -> Self {
        Self {
            html_element: HTMLElement::new(document, node_id),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-iframe-src>
    pub(crate) fn src(&self) -> String {
        // Step 1: "Return the value of the src content attribute."
        self.html_element
            .element
            .get_attribute("src")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-iframe-src>
    pub(crate) fn set_src(&self, src: &str) {
        // Step 1: "Set this's src content attribute to the given value."
        self.html_element.element.set_attribute("src", src);
    }

    /// <https://html.spec.whatwg.org/#dom-iframe-srcdoc>
    pub(crate) fn srcdoc(&self) -> String {
        // Step 1: "Return the value of the srcdoc content attribute."
        self.html_element
            .element
            .get_attribute("srcdoc")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-iframe-srcdoc>
    pub(crate) fn set_srcdoc(&self, srcdoc: &str) {
        // Step 1: "Set this's srcdoc content attribute to the given value."
        self.html_element.element.set_attribute("srcdoc", srcdoc);
    }

    /// <https://html.spec.whatwg.org/#dom-iframe-name>
    pub(crate) fn name(&self) -> String {
        // Step 1: "Return the value of the name content attribute."
        self.html_element
            .element
            .get_attribute("name")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-iframe-name>
    pub(crate) fn set_name(&self, name: &str) {
        // Step 1: "Set this's name content attribute to the given value."
        self.html_element.element.set_attribute("name", name);
    }

    /// <https://html.spec.whatwg.org/#dom-dim-width>
    pub(crate) fn width(&self) -> String {
        // Step 1: "Return the value of the width content attribute."
        self.html_element
            .element
            .get_attribute("width")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-dim-width>
    pub(crate) fn set_width(&self, width: &str) {
        // Step 1: "Set this's width content attribute to the given value."
        self.html_element.element.set_attribute("width", width);
    }

    /// <https://html.spec.whatwg.org/#dom-dim-height>
    pub(crate) fn height(&self) -> String {
        // Step 1: "Return the value of the height content attribute."
        self.html_element
            .element
            .get_attribute("height")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-dim-height>
    pub(crate) fn set_height(&self, height: &str) {
        // Step 1: "Set this's height content attribute to the given value."
        self.html_element.element.set_attribute("height", height);
    }
}

enum IframeNavigationTarget {
    AboutBlank,
    Srcdoc { html: String },
    Url { url: Url },
}

fn matches_about_blank(url: &Url) -> bool {
    url.scheme() == "about" && url.path() == "blank"
}

/// <https://html.spec.whatwg.org/#process-the-iframe-attributes>
fn iframe_navigation_target_for_attributes(
    creation_url: &Url,
    src: Option<&str>,
    srcdoc: Option<&str>,
) -> (String, IframeNavigationTarget) {
    // Step 1: "If element has a srcdoc attribute specified, then:"
    if let Some(srcdoc) = srcdoc {
        // Step 1.1: "Set url to about:srcdoc."
        // Step 1.2: "Set srcdoc to the value of element's srcdoc attribute."
        return (
            format!("srcdoc:{srcdoc}"),
            IframeNavigationTarget::Srcdoc {
                html: srcdoc.to_owned(),
            },
        );
    }

    // Step 2: "Otherwise, let url be the result of running the shared attribute processing steps for iframe and frame elements."
    if let Some(src) = src.map(str::trim) {
        if !src.is_empty() {
            if let Ok(url) = creation_url.join(src) {
                return (format!("src:{}", url), IframeNavigationTarget::Url { url });
            }
        }
    }

    // Step 3: "If url is failure, set url to about:blank."
    (String::from("about:blank"), IframeNavigationTarget::AboutBlank)
}

fn connected_iframe_node_ids(document: &BaseDocument) -> Vec<usize> {
    let mut iframe_node_ids = Vec::new();
    document.visit(|node_id, node| {
        let Some(element) = node.element_data() else {
            return;
        };
        if element.name.ns != ns!(html)
            || element.name.local != local_name!("iframe")
            || !node.flags.is_in_document()
        {
            return;
        }

        iframe_node_ids.push(node_id);
    });
    iframe_node_ids
}

/// <https://html.spec.whatwg.org/#process-the-iframe-attributes>
fn iframe_navigation_target(
    document: &BaseDocument,
    creation_url: &Url,
    iframe_node_id: usize,
) -> Option<(String, IframeNavigationTarget)> {
    let node = document.get_node(iframe_node_id)?;
    let element = node.element_data()?;

    Some(iframe_navigation_target_for_attributes(
        creation_url,
        element.attr(local_name!("src")),
        element.attr(local_name!("srcdoc")),
    ))
}

fn attach_iframe_subdocument_from_html(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
    iframe_node_id: usize,
    base_url: String,
    html: String,
) -> Result<(), String> {
    let Some(traversable_id) = runtime
        .documents
        .get(&parent_document_id)
        .map(|content_document| content_document.traversable_id)
    else {
        return Ok(());
    };
    let sub_document = Rc::new(RefCell::new(BaseDocument::new(runtime.document_config(
        traversable_id,
        parent_document_id,
        Some(base_url),
    ))));
    {
        let mut sub_document_guard = sub_document.borrow_mut();
        super::parse_html_into_document(&mut sub_document_guard, &html);
    }

    let Some(content_document) = runtime.documents.get_mut(&parent_document_id) else {
        return Ok(());
    };
    let mut document = content_document.document.borrow_mut();
    if document.get_node(iframe_node_id).is_none() {
        return Ok(());
    }
    let mut mutator = document.mutate();
    mutator.remove_cross_origin_iframe(iframe_node_id);
    mutator.set_sub_document(iframe_node_id, Box::new(sub_document));
    Ok(())
}

fn attach_cross_origin_iframe(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
    iframe_node_id: usize,
    frame_token: u64,
) -> Result<(), String> {
    let Some(content_document) = runtime.documents.get_mut(&parent_document_id) else {
        return Ok(());
    };
    let mut document = content_document.document.borrow_mut();
    if document.get_node(iframe_node_id).is_none() {
        return Ok(());
    }
    let mut mutator = document.mutate();
    mutator.remove_sub_document(iframe_node_id);
    mutator.set_cross_origin_iframe(iframe_node_id, frame_token);
    Ok(())
}

/// <https://html.spec.whatwg.org/multipage/#navigate-an-iframe-or-frame>
/// Note: The iframe's content navigable is created during `create a new child navigable`; this
/// helper sends the iframe navigation request to the user agent with the content navigable ID.
fn navigate_an_iframe_or_frame(
    runtime: &ContentRuntime,
    content_navigable_id: ContentNavigableId,
    parent_traversable_id: u64,
    content_frame_id: FrameId,
    destination_url: &Url,
) -> Result<(), String> {
    // Step 1: "Let elementDocument be element's node document."
    // Note: The caller resolves parent-document state before invoking this helper.

    // Step 2: "Let targetNavigable be element's content navigable."
    // Note: `content_navigable_id` is the unique navigable ID created for this iframe.

    // Step 3: "Navigate targetNavigable to url."
    // Note: content_navigable_id is the stable navigable identifier used consistently
    // across content and user agent.
    let navigable_id: NavigableId = content_navigable_id.into();
    runtime
        .event_sender
        .send(ContentEvent::NavigationRequested(NavigateRequest {
            navigation_id: Some(NavigationId::new()),
            source_navigable_id: navigable_id,
            chosen_navigable_id: Some(navigable_id),
            destination_url: destination_url.to_string(),
            target: iframe_target_name(
                parent_traversable_id,
                content_navigable_id,
                content_frame_id,
            ),
            user_involvement: UserNavigationInvolvement::None,
            noopener: false,
        }))
        .map_err(|error| format!("failed to send iframe navigation request: {error}"))
}

fn attach_iframe_about_blank(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
    iframe_node_id: usize,
) -> Result<(), String> {
    attach_iframe_subdocument_from_html(
        runtime,
        parent_document_id,
        iframe_node_id,
        String::from("about:blank"),
        String::from(EMPTY_HTML_DOCUMENT),
    )
}

fn remove_iframe_subdocument(runtime: &mut ContentRuntime, parent_document_id: u64, iframe_node_id: usize) {
    let Some(content_document) = runtime.documents.get_mut(&parent_document_id) else {
        return;
    };
    {
        let mut document = content_document.document.borrow_mut();
        if document.get_node(iframe_node_id).is_none() {
            return;
        }
        let mut mutator = document.mutate();
        mutator.remove_sub_document(iframe_node_id);
        mutator.remove_cross_origin_iframe(iframe_node_id);
    }
}

pub(crate) fn retire_iframe_traversable(
    runtime: &ContentRuntime,
    parent_traversable_id: u64,
    container_state: &NavigableContainerState,
) -> Result<(), String> {
    if !container_state.cross_origin {
        return Ok(());
    }

    runtime
        .event_sender
        .send(ContentEvent::IframeTraversableRemoved(IframeTraversableRemoval {
            parent_traversable_id,
            content_navigable_id: container_state.content_navigable_id,
            content_frame_id: container_state.content_frame_id,
        }))
        .map_err(|error| format!("failed to send iframe traversable removal: {error}"))
}

/// <https://html.spec.whatwg.org/#create-a-new-child-navigable>
/// Step 1-8: Content allocates IDs, installs container state, and sends
/// CreateChildNavigable to the user agent without waiting.
/// Step 9+: The user agent runs its own suffix steps after receiving that message.
/// Note: Content keeps running synchronously and immediately continues with
/// `process the iframe attributes` in the same post-connection task.
fn allocate_child_navigable_state(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
    iframe_node_id: usize,
) -> Result<(), String> {
    // If a child navigable already exists for this iframe, nothing to do.
    if runtime
        .documents
        .get(&parent_document_id)
        .and_then(|content_document| {
            content_document
                .navigable_container_states
                .get(&iframe_node_id)
        })
        .is_some()
    {
        return Ok(());
    }

    // Step 1: "Let document be element's node document."
    // Note: `parent_document_id` identifies that already-materialized document.

    // Step 2: "Let parentTraversable be document's node navigable's traversable navigable."
    // Note: `parent_traversable_id` is read from the parent content document state.

    // Step 3: "Let id be a new navigable id."
    // Step 4: Allocate cross-process frame identifiers used by paint/compositor plumbing.
    let content_navigable_id = runtime.allocate_child_navigable_id()?;
    let content_frame_id = runtime.allocate_child_frame_id();
    let content_frame_token = runtime.allocate_placeholder_frame_token();

    let parent_traversable_id = runtime
        .documents
        .get(&parent_document_id)
        .map(|content_document| content_document.traversable_id)
        .ok_or_else(|| format!("allocate_child_navigable_state: parent document {parent_document_id} not found"))?;

    // Step 5: "Let navigable be the result of creating a new browsing context and document..."
    // Note: The content-side half is to install the iframe container state immediately.

    // Step 6: "Set element's content navigable to navigable."
    // Note: `navigable_container_states` is the content-side storage for that relationship.

    // Step 7: "Set element's iframe load in progress flag to false."
    // Note: The explicit flag is not modeled yet; container-state insertion is the current
    // observable effect before user-agent continuation.
    if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
        content_document.navigable_container_states.insert(
            iframe_node_id,
            NavigableContainerState {
                content_navigable_id,
                content_frame_id,
                content_frame_token,
                current_key: String::new(),
                cross_origin: false,
            },
        );
    }

    // Step 8: "Append the session-history traversal steps to traversable."
    // Note: The user-agent owns those traversal steps. Content notifies the user-agent and
    // continues synchronously without waiting for a continuation message.
    runtime
        .event_sender
        .send(ContentEvent::CreateChildNavigable(CreateChildNavigableRequest {
            parent_traversable_id,
            content_navigable_id,
            content_frame_id,
        }))
        .map_err(|error| format!("failed to send create-child-navigable request: {error}"))
}

fn navigable_container_for_child_navigable(
    runtime: &ContentRuntime,
    content_frame_id: FrameId,
) -> Option<(u64, usize)> {
    runtime
        .documents
        .iter()
        .find_map(|(parent_document_id, content_document)| {
            content_document
                .navigable_container_states
                .iter()
                .find_map(|(iframe_node_id, container_state)| {
                    (container_state.content_frame_id == content_frame_id)
                        .then_some((*parent_document_id, *iframe_node_id))
                })
        })
}



pub(crate) fn attach_same_origin_child_document_for_traversable(
    runtime: &mut ContentRuntime,
    traversable_id: u64,
) -> Result<(), String> {
    let Some(document_id) = runtime
        .active_documents_by_traversable
        .get(&traversable_id)
        .copied()
    else {
        return Ok(());
    };
    let Some(child_document) = runtime.documents.get(&document_id) else {
        return Ok(());
    };
    let child_frame_id = child_document.frame_id;
    let child_subdocument = Rc::clone(&child_document.document);
    let Some((parent_document_id, iframe_node_id)) =
        navigable_container_for_child_navigable(runtime, child_frame_id)
    else {
        return Ok(());
    };
    let Some(container_state) = runtime
        .documents
        .get(&parent_document_id)
        .and_then(|content_document| content_document.navigable_container_states.get(&iframe_node_id))
    else {
        return Ok(());
    };
    if container_state.cross_origin {
        return Ok(());
    }
    let Some(parent_document) = runtime.documents.get_mut(&parent_document_id) else {
        return Ok(());
    };
    let mut document = parent_document.document.borrow_mut();
    if document.get_node(iframe_node_id).is_none() {
        return Ok(());
    }
    let mut mutator = document.mutate();
    mutator.remove_cross_origin_iframe(iframe_node_id);
    mutator.set_sub_document(iframe_node_id, Box::new(child_subdocument));
    Ok(())
}

/// <https://html.spec.whatwg.org/#iframe-load-event-steps>
fn run_iframe_load_event_steps(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
    iframe_node_id: usize,
) -> Result<(), String> {
    let Some(content_document) = runtime.documents.get_mut(&parent_document_id) else {
        return Ok(());
    };

    let iframe_exists = {
        let document = content_document.document.borrow();
        document.get_node(iframe_node_id).is_some()
    };
    if !iframe_exists {
        return Ok(());
    }

    // Step 1: "Assert: element's content navigable is not null."
    // Note: The runtime stores iframe state only after creating a child navigable for the iframe node.

    // Step 2: "Let childDocument be element's content navigable's active document."
    // Note: The current content runtime does not expose a separate child-document carrier here; the iframe node state identifies the active child navigable.

    // Step 3: "If childDocument has its mute iframe load flag set, then return."
    // TODO: Model the document mute iframe load flag.

    let iframe_object = crate::boa::platform_objects::resolve_element_object(
        iframe_node_id,
        &mut content_document.settings.context,
    )
    .map_err(|error| error.to_string())?;

    // Step 4: "If element's pending resource-timing start time is not null, then:"
    // TODO: Model iframe resource timing bookkeeping.

    // Step 5: "Set childDocument's iframe load in progress flag."
    // TODO: Model the document iframe load in progress flag.

    // Step 6: "Fire an event named load at element."
    fire_event(&mut content_document.settings, &iframe_object, "load", false)
        .map_err(|error| error.to_string())?;

    // Step 7: "Unset childDocument's iframe load in progress flag."
    // TODO: Model the document iframe load in progress flag.

    Ok(())
}

pub(crate) fn run_iframe_load_event_steps_for_traversable(
    runtime: &mut ContentRuntime,
    traversable_id: u64,
) -> Result<(), String> {
    let Some(document_id) = runtime
        .active_documents_by_traversable
        .get(&traversable_id)
        .copied()
    else {
        return Ok(());
    };
    let Some(content_frame_id) = runtime.documents.get(&document_id).map(|document| document.frame_id)
    else {
        return Ok(());
    };
    let Some((parent_document_id, iframe_node_id)) =
        navigable_container_for_child_navigable(runtime, content_frame_id)
    else {
        return Ok(());
    };

    run_iframe_load_event_steps(runtime, parent_document_id, iframe_node_id)
}

/// <https://html.spec.whatwg.org/#html-element-post-connection-steps>
fn run_iframe_post_connection_steps(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
    iframe_node_id: usize,
) -> Result<(), String> {
    // Step 1: "Create a new child navigable for insertedNode."
    // Content allocates IDs and sends CreateChildNavigable message (fire and forget).
    // Content does NOT wait for async response; it continues synchronously instead.
    allocate_child_navigable_state(runtime, parent_document_id, iframe_node_id)?;

    // Step 2: "If insertedNode has a sandbox attribute, then parse the sandboxing directive given the attribute's value and insertedNode's iframe sandboxing flag set."
    // TODO: Model iframe sandboxing flag parsing.

    // Step 3: "Process the iframe attributes for insertedNode, with initialInsertion set to true."
    // This runs as part of the same synchronous execution, not as an async continuation.
    process_iframe_attributes(runtime, parent_document_id, iframe_node_id, true)
}

pub(crate) fn run_iframe_post_connection_steps_for_document(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
) -> Result<(), String> {
    let iframe_node_ids = {
        let Some(content_document) = runtime.documents.get(&parent_document_id) else {
            return Ok(());
        };
        let document = content_document.document.borrow();
        connected_iframe_node_ids(&document)
    };

    for iframe_node_id in iframe_node_ids {
        run_iframe_post_connection_steps(runtime, parent_document_id, iframe_node_id)?;
    }

    Ok(())
}

/// <https://dom.spec.whatwg.org/#concept-node-remove>
fn run_iframe_removing_steps(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
    iframe_node_id: usize,
) -> Result<(), String> {
    let Some((parent_traversable_id, iframe_state)) = runtime
        .documents
        .get(&parent_document_id)
        .and_then(|content_document| {
            content_document
                .navigable_container_states
                .get(&iframe_node_id)
                .cloned()
                .map(|state| (content_document.traversable_id, state))
        })
    else {
        return Ok(());
    };

    retire_iframe_traversable(runtime, parent_traversable_id, &iframe_state)?;
    remove_iframe_subdocument(runtime, parent_document_id, iframe_node_id);
    if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
        content_document
            .navigable_container_states
            .remove(&iframe_node_id);
    }
    Ok(())
}

pub(crate) fn run_iframe_removing_steps_for_document(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
) -> Result<(), String> {
    let iframe_node_ids = {
        let Some(content_document) = runtime.documents.get(&parent_document_id) else {
            return Ok(());
        };
        let document = content_document.document.borrow();
        connected_iframe_node_ids(&document)
    };

    for iframe_node_id in iframe_node_ids {
        run_iframe_removing_steps(runtime, parent_document_id, iframe_node_id)?;
    }

    Ok(())
}

/// <https://html.spec.whatwg.org/#process-the-iframe-attributes>
fn process_iframe_attributes(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
    iframe_node_id: usize,
    initial_insertion: bool,
) -> Result<(), String> {
    // Step 1: "Let creation url be insertedNode's node document's relevant settings object's creation URL."
    let (creation_url, parent_traversable_id, desired_key, target) = {
        let Some(content_document) = runtime.documents.get(&parent_document_id) else {
            return Ok(());
        };
        let creation_url = content_document.settings.creation_url.clone();
        let document = content_document.document.borrow();
        // Step 2: "Let url be the result of running the shared attribute processing steps for iframe and frame elements."
        let Some((desired_key, target)) =
            iframe_navigation_target(&document, &creation_url, iframe_node_id)
        else {
            return Ok(());
        };
        (
            creation_url,
            content_document.traversable_id,
            desired_key,
            target,
        )
    };

    // Step 3: "If element's current pending navigation key equals the new key, then return."
    // Note: `current_key` stores the last processed key for this container.
    let previous_iframe_state = runtime
        .documents
        .get(&parent_document_id)
        .and_then(|content_document| {
            content_document
                .navigable_container_states
                .get(&iframe_node_id)
        })
        .cloned();
    
    let current_key = previous_iframe_state
        .as_ref()
        .map(|state| state.current_key.clone());
    if current_key.as_deref() == Some(desired_key.as_str()) {
        return Ok(());
    }

    // Step 4: "Let navigable be element's content navigable if it exists, or null otherwise."
    // Note: Post-connection step 1 allocates the child navigable before this function runs.
    let content_navigable_id = match previous_iframe_state
        .as_ref()
        .map(|state| state.content_navigable_id)
    {
        Some(content_navigable_id) => content_navigable_id,
        None => {
            // The post-connection algorithm always runs `create a new child navigable` first.
            // If that relationship is absent, there is no valid continuation.
            return Ok(());
        }
    };
    let content_frame_id = match previous_iframe_state.as_ref().map(|state| state.content_frame_id) {
        Some(content_frame_id) => content_frame_id,
        None => return Ok(()),
    };
    let content_frame_token = match previous_iframe_state
        .as_ref()
        .map(|state| state.content_frame_token)
    {
        Some(content_frame_token) => content_frame_token,
        None => return Ok(()),
    };

    // Step 5: "If navigable is null, then return."
    // Note: The early return above already enforces this branch.

    // Step 7: Determine origin for cross-origin detection.
    let cross_origin =
        matches!(&target, IframeNavigationTarget::Url { url } if creation_url.origin() != url.origin());

    // Step 8: Retire old traversable if switching from cross-origin to same-origin.
    if let Some(previous_iframe_state) = previous_iframe_state.as_ref() {
        if previous_iframe_state.cross_origin && !cross_origin {
            retire_iframe_traversable(runtime, parent_traversable_id, previous_iframe_state)?;
        }
    }

    // Step 9: "If url matches about:blank and initialInsertion is true, then return."
    if initial_insertion {
        match &target {
            // Step 9a: about:blank with initialInsertion
            IframeNavigationTarget::AboutBlank => {
                if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
                    content_document.navigable_container_states.insert(
                        iframe_node_id,
                        NavigableContainerState {
                            content_navigable_id,
                            content_frame_id,
                            content_frame_token,
                            current_key: desired_key.clone(),
                            cross_origin: false,
                        },
                    );
                }
                run_iframe_load_event_steps(runtime, parent_document_id, iframe_node_id)?;
                return Ok(());
            }
            // Step 9b: URL matches about:blank
            IframeNavigationTarget::Url { url } if matches_about_blank(url) => {
                if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
                    content_document.navigable_container_states.insert(
                        iframe_node_id,
                        NavigableContainerState {
                            content_navigable_id,
                            content_frame_id,
                            content_frame_token,
                            current_key: desired_key.clone(),
                            cross_origin: false,
                        },
                    );
                }
                run_iframe_load_event_steps(runtime, parent_document_id, iframe_node_id)?;
                return Ok(());
            }
            _ => {}
        }
    }

    // Step 10: Update iframe container state with new attributes.
    if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
        content_document.navigable_container_states.insert(
            iframe_node_id,
            NavigableContainerState {
                content_navigable_id,
                content_frame_id,
                content_frame_token,
                current_key: desired_key.clone(),
                cross_origin,
            },
        );
    }

    // Step 11: "Navigate navigable to url."
    // Note: <https://html.spec.whatwg.org/multipage/#navigate-an-iframe-or-frame>
    match target {
        // Step 11a: about:blank - this branch is reachable for non-initial insertion runs,
        // e.g. when later attribute changes resolve to about:blank.
        IframeNavigationTarget::AboutBlank => {
            attach_iframe_about_blank(runtime, parent_document_id, iframe_node_id)?;
            run_iframe_load_event_steps(runtime, parent_document_id, iframe_node_id)?;
        }
        // Step 11b: srcdoc - attach HTML content locally
        IframeNavigationTarget::Srcdoc { html } => {
            attach_iframe_subdocument_from_html(
                runtime,
                parent_document_id,
                iframe_node_id,
                creation_url.to_string(),
                html,
            )?;
            run_iframe_load_event_steps(runtime, parent_document_id, iframe_node_id)?;
        }
        // Step 11c: URL - navigate via user agent
        IframeNavigationTarget::Url { url } => {
            if cross_origin {
                attach_cross_origin_iframe(
                    runtime,
                    parent_document_id,
                    iframe_node_id,
                    content_frame_token,
                )?;
            }
            
            // content_navigable_id is the navigable identifier for this iframe
            navigate_an_iframe_or_frame(
                runtime,
                content_navigable_id,
                parent_traversable_id,
                content_frame_id,
                &url,
            )?;
        }
    }

    Ok(())
}