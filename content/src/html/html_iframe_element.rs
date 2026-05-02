use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};
use html5ever::{local_name, ns};
use ipc_messages::content::{
    ChildNavigableCreation, Event as ContentEvent, IframeTraversableRemoval, NavigateRequest,
    UserNavigationInvolvement,
};
use url::Url;

use crate::{
    ContentRuntime, EMPTY_HTML_DOCUMENT, IframeState, dom::fire_event, html::HTMLElement,
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
    let sub_document = Rc::new(RefCell::new(BaseDocument::new(runtime.document_config(
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
    frame_id: u64,
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
    mutator.set_cross_origin_iframe(iframe_node_id, frame_id);
    Ok(())
}

fn request_iframe_navigation(
    runtime: &ContentRuntime,
    parent_document_id: u64,
    source_navigable_id: u64,
    destination_url: &Url,
) -> Result<(), String> {
    let Some(parent_traversable_id) = runtime
        .documents
        .get(&parent_document_id)
        .map(|d| d.traversable_id)
    else {
        return Err(format!(
            "request_iframe_navigation: parent document {parent_document_id} not found"
        ));
    };
    runtime
        .event_sender
        .send(ContentEvent::NavigationRequested(NavigateRequest {
            source_navigable_id,
            destination_url: destination_url.to_string(),
            target: format!("_iframe|{}|{}", parent_traversable_id, source_navigable_id),
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
    iframe_state: &IframeState,
) -> Result<(), String> {
    if !iframe_state.cross_origin {
        return Ok(());
    }

    runtime
        .event_sender
        .send(ContentEvent::IframeTraversableRemoved(IframeTraversableRemoval {
            parent_traversable_id,
            source_navigable_id: iframe_state.source_navigable_id,
        }))
        .map_err(|error| format!("failed to send iframe traversable removal: {error}"))
}

/// <https://html.spec.whatwg.org/#create-a-new-child-navigable>
fn create_new_child_navigable(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
    iframe_node_id: usize,
) -> Result<u64, String> {
    if let Some(iframe_state) = runtime
        .documents
        .get(&parent_document_id)
        .and_then(|content_document| content_document.iframe_states.get(&iframe_node_id))
    {
        return Ok(iframe_state.source_navigable_id);
    }

    let source_navigable_id = runtime.allocate_iframe_navigable_id();
    let Some(parent_traversable_id) = runtime
        .documents
        .get(&parent_document_id)
        .map(|d| d.traversable_id)
    else {
        debug_assert!(
            false,
            "create_new_child_navigable: parent document {parent_document_id} not found"
        );
        return Ok(source_navigable_id);
    };

    if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
        content_document.iframe_states.insert(
            iframe_node_id,
            IframeState {
                source_navigable_id,
                current_key: String::new(),
                cross_origin: false,
            },
        );
    }

    runtime
        .event_sender
        .send(ContentEvent::ChildNavigableCreated(ChildNavigableCreation {
            parent_traversable_id,
            source_navigable_id,
        }))
        .map_err(|error| format!("failed to send child navigable created event: {error}"))?;

    attach_iframe_about_blank(runtime, parent_document_id, iframe_node_id)?;
    Ok(source_navigable_id)
}

fn iframe_parent_for_traversable(
    runtime: &ContentRuntime,
    traversable_id: u64,
) -> Option<(u64, usize)> {
    runtime
        .documents
        .iter()
        .find_map(|(parent_document_id, content_document)| {
            content_document
                .iframe_states
                .iter()
                .find_map(|(iframe_node_id, iframe_state)| {
                    (iframe_state.source_navigable_id == traversable_id)
                        .then_some((*parent_document_id, *iframe_node_id))
                })
        })
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
    let Some((parent_document_id, iframe_node_id)) =
        iframe_parent_for_traversable(runtime, traversable_id)
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
                .iframe_states
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
        content_document.iframe_states.remove(&iframe_node_id);
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
        .and_then(|content_document| content_document.iframe_states.get(&iframe_node_id))
        .cloned();
    let current_key = previous_iframe_state
        .as_ref()
        .map(|state| state.current_key.clone());
    if current_key.as_deref() == Some(desired_key.as_str()) {
        return Ok(());
    }

    let source_navigable_id = previous_iframe_state
        .as_ref()
        .map(|state| state.source_navigable_id)
        .unwrap_or_else(|| runtime.allocate_iframe_navigable_id());
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
                    content_document.iframe_states.insert(
                        iframe_node_id,
                        IframeState {
                            source_navigable_id,
                            current_key: desired_key.clone(),
                            cross_origin: false,
                        },
                    );
                }
                run_iframe_load_event_steps(runtime, parent_document_id, iframe_node_id)?;
                return Ok(());
            }
            IframeNavigationTarget::Url { url } if matches_about_blank(url) => {
                if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
                    content_document.iframe_states.insert(
                        iframe_node_id,
                        IframeState {
                            source_navigable_id,
                            current_key: desired_key.clone(),
                            cross_origin: false,
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
        content_document.iframe_states.insert(
            iframe_node_id,
            IframeState {
                source_navigable_id,
                current_key: desired_key.clone(),
                cross_origin,
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
                attach_cross_origin_iframe(runtime, parent_document_id, iframe_node_id, source_navigable_id)?;
            }
            request_iframe_navigation(runtime, parent_document_id, source_navigable_id, &url)?;
        }
    }

    Ok(())
}