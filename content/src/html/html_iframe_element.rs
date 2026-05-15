use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};
use html5ever::{local_name, ns};
use ipc_messages::content::{
    ChildNavigableCreated, ContentNavigableId, CreateChildNavigableRequest,
    Event as ContentEvent, FrameId, IframeTraversableRemoval, NavigateRequest, NavigationId,
    UserNavigationInvolvement, iframe_target_name,
};
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
/// helper reuses that stable navigable id for later navigations.
fn navigate_an_iframe_or_frame(
    runtime: &ContentRuntime,
    navigable_id: u64,
    parent_traversable_id: u64,
    content_navigable_id: ContentNavigableId,
    content_frame_id: FrameId,
    destination_url: &Url,
) -> Result<(), String> {
    // Step 1: "Let elementDocument be element's node document."
    // Note: The caller resolves parent-document state before invoking this helper.

    // Step 2: "Let targetNavigable be element's content navigable."
    // Note: `navigable_id` is the stable child navigable created for the iframe's initial
    // about:blank document by `create a new child navigable`.

    // Step 3: "Navigate targetNavigable to url."
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
/// Note: Content runs the local DOM/container steps first, then asks the user agent to continue
/// the child-navigable creation suffix asynchronously.
fn request_child_navigable_creation(
    runtime: &ContentRuntime,
    parent_traversable_id: u64,
    content_navigable_id: ContentNavigableId,
    content_frame_id: FrameId,
) -> Result<(), String> {
    runtime
        .event_sender
        .send(ContentEvent::CreateChildNavigable(CreateChildNavigableRequest {
            parent_traversable_id,
            content_navigable_id,
            content_frame_id,
        }))
        .map_err(|error| format!("failed to send create-child-navigable request: {error}"))
}

/// <https://html.spec.whatwg.org/#create-a-new-child-navigable>
fn create_new_child_navigable(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
    iframe_node_id: usize,
) -> Result<ContentNavigableId, String> {
    if let Some(container_state) = runtime
        .documents
        .get(&parent_document_id)
        .and_then(|content_document| {
            content_document
                .navigable_container_states
                .get(&iframe_node_id)
        })
    {
        return Ok(container_state.content_navigable_id);
    }

    let content_navigable_id = runtime.allocate_child_navigable_id()?;
    let content_frame_id = runtime.allocate_child_frame_id();
    let content_frame_token = runtime.allocate_placeholder_frame_token();
    let parent_traversable_id = runtime
        .documents
        .get(&parent_document_id)
        .map(|content_document| content_document.traversable_id)
        .ok_or_else(|| format!("create_new_child_navigable: parent document {parent_document_id} not found"))?;
    request_child_navigable_creation(
        runtime,
        parent_traversable_id,
        content_navigable_id,
        content_frame_id,
    )?;
    if runtime.documents.get(&parent_document_id).is_none() {
        debug_assert!(
            false,
            "create_new_child_navigable: parent document {parent_document_id} not found"
        );
        return Ok(content_navigable_id);
    }

    if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
        content_document.navigable_container_states.insert(
            iframe_node_id,
            NavigableContainerState {
                navigable_id: None,
                content_navigable_id,
                content_frame_id,
                content_frame_token,
                current_key: String::new(),
                cross_origin: false,
                child_creation_requested: true,
            },
        );
    }

    attach_iframe_about_blank(runtime, parent_document_id, iframe_node_id)?;
    Ok(content_navigable_id)
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

/// <https://html.spec.whatwg.org/#create-a-new-child-navigable>
/// Note: This resumes the content-side continuation after the user agent finishes the suffix that
/// allocates the stable child navigable and its initial session-history state.
pub(crate) fn complete_child_navigable_creation(
    runtime: &mut ContentRuntime,
    created: ChildNavigableCreated,
) -> Result<(), String> {
    let Some((parent_document_id, iframe_node_id)) = navigable_container_for_child_navigable(
        runtime,
        created.content_frame_id,
    ) else {
        return Ok(());
    };

    let Some(parent_traversable_id) = runtime
        .documents
        .get(&parent_document_id)
        .map(|content_document| content_document.traversable_id)
    else {
        return Ok(());
    };
    if parent_traversable_id != created.parent_traversable_id {
        return Ok(());
    }

    let Some(content_document) = runtime.documents.get_mut(&parent_document_id) else {
        return Ok(());
    };
    let Some(container_state) = content_document
        .navigable_container_states
        .get_mut(&iframe_node_id)
    else {
        return Ok(());
    };
    if container_state.content_navigable_id != created.content_navigable_id {
        return Ok(());
    }

    container_state.navigable_id = Some(created.navigable_id);
    container_state.child_creation_requested = false;
    if container_state.current_key.starts_with("src:") {
        container_state.current_key.clear();
    }
    process_iframe_attributes(runtime, parent_document_id, iframe_node_id, false)
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
    create_new_child_navigable(runtime, parent_document_id, iframe_node_id)?;

    // Step 2: "If insertedNode has a sandbox attribute, then parse the sandboxing directive given the attribute's value and insertedNode's iframe sandboxing flag set."
    // TODO: Model iframe sandboxing flag parsing.

    // Step 3: "Process the iframe attributes for insertedNode, with initialInsertion set to true."
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
    let (creation_url, parent_traversable_id, desired_key, target) = {
        let Some(content_document) = runtime.documents.get(&parent_document_id) else {
            return Ok(());
        };
        let creation_url = content_document.settings.creation_url.clone();
        let document = content_document.document.borrow();
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

    let content_navigable_id = match previous_iframe_state
        .as_ref()
        .map(|state| state.content_navigable_id)
    {
        Some(content_navigable_id) => content_navigable_id,
        None => runtime.allocate_child_navigable_id()?,
    };
    let content_frame_id = match previous_iframe_state.as_ref().map(|state| state.content_frame_id)
    {
        Some(content_frame_id) => content_frame_id,
        None => runtime.allocate_child_frame_id(),
    };
    let content_frame_token = match previous_iframe_state
        .as_ref()
        .map(|state| state.content_frame_token)
    {
        Some(content_frame_token) => content_frame_token,
        None => runtime.allocate_placeholder_frame_token(),
    };
    let navigable_id = previous_iframe_state.as_ref().and_then(|state| state.navigable_id);
    let child_creation_requested = previous_iframe_state
        .as_ref()
        .is_some_and(|state| state.child_creation_requested);
    if navigable_id.is_none() && !child_creation_requested {
        request_child_navigable_creation(
            runtime,
            parent_traversable_id,
            content_navigable_id,
            content_frame_id,
        )?;
    }
    let cross_origin =
        matches!(&target, IframeNavigationTarget::Url { url } if creation_url.origin() != url.origin());
    if let Some(previous_iframe_state) = previous_iframe_state.as_ref() {
        if previous_iframe_state.cross_origin && !cross_origin {
            retire_iframe_traversable(runtime, parent_traversable_id, previous_iframe_state)?;
        }
    }
    if initial_insertion {
        match &target {
            IframeNavigationTarget::AboutBlank => {
                if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
                    content_document.navigable_container_states.insert(
                        iframe_node_id,
                        NavigableContainerState {
                            navigable_id,
                            content_navigable_id,
                            content_frame_id,
                            content_frame_token,
                            current_key: desired_key.clone(),
                            cross_origin: false,
                            child_creation_requested: navigable_id.is_none(),
                        },
                    );
                }
                run_iframe_load_event_steps(runtime, parent_document_id, iframe_node_id)?;
                return Ok(());
            }
            IframeNavigationTarget::Url { url } if matches_about_blank(url) => {
                if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
                    content_document.navigable_container_states.insert(
                        iframe_node_id,
                        NavigableContainerState {
                            navigable_id,
                            content_navigable_id,
                            content_frame_id,
                            content_frame_token,
                            current_key: desired_key.clone(),
                            cross_origin: false,
                            child_creation_requested: navigable_id.is_none(),
                        },
                    );
                }
                run_iframe_load_event_steps(runtime, parent_document_id, iframe_node_id)?;
                return Ok(());
            }
            IframeNavigationTarget::Srcdoc { .. } | IframeNavigationTarget::Url { .. } => {}
        }
    }

    if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
        content_document.navigable_container_states.insert(
            iframe_node_id,
            NavigableContainerState {
                navigable_id,
                content_navigable_id,
                content_frame_id,
                content_frame_token,
                current_key: desired_key.clone(),
                cross_origin,
                child_creation_requested: navigable_id.is_none(),
            },
        );
    }

    // Step 4: "Navigate element's content navigable to url."
    match target {
        IframeNavigationTarget::AboutBlank => {
            attach_iframe_about_blank(runtime, parent_document_id, iframe_node_id)?;
            run_iframe_load_event_steps(runtime, parent_document_id, iframe_node_id)?;
        }
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
        IframeNavigationTarget::Url { url } => {
            if cross_origin {
                attach_cross_origin_iframe(
                    runtime,
                    parent_document_id,
                    iframe_node_id,
                    content_frame_token,
                )?;
            }
            let Some(navigable_id) = navigable_id else {
                return Ok(());
            };
            navigate_an_iframe_or_frame(
                runtime,
                navigable_id,
                parent_traversable_id,
                content_navigable_id,
                content_frame_id,
                &url,
            )?;
        }
    }

    Ok(())
}