use ipc_messages::content::{
    CreateChildNavigableRequest,
    Event as ContentEvent, FrameId, IframeTraversableRemoval, NavigableId, NavigateRequest, NavigationId,
    UserNavigationInvolvement, iframe_target_name,
};
use std::{cell::RefCell, collections::HashSet, rc::Rc};

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
/// <https://html.spec.whatwg.org/#shared-attribute-processing-steps-for-iframe-and-frame-elements>
fn shared_attribute_processing_steps_for_iframe_and_frame_elements(
    document: &BaseDocument,
    creation_url: &Url,
    iframe_node_id: usize,
    _initial_insertion: bool,
) -> Option<Url> {
    // Shared step 1: "Let url be the URL record about:blank."
    let url = Url::parse("about:blank").ok()?;

    // Shared step 2: "If element has a src attribute specified, and its value is not the
    // empty string: let maybeURL be the result of encoding-parsing a URL given that
    // attribute's value, relative to element's node document. If maybeURL is not failure,
    // then set url to maybeURL."
    let node = document.get_node(iframe_node_id)?;
    let element = node.element_data()?;
    if let Some(src) = element.attr(local_name!("src")).map(str::trim) {
        if !src.is_empty() {
            if let Ok(url) = creation_url.join(src) {
                // TODO: Shared step 3: "If the inclusive ancestor navigables of element's
                // node navigable contains a navigable whose active document's URL equals url
                // with exclude fragments set to true, then return null."
                // This needs user-agent-owned navigable ancestry.
                return Some(url);
            }
        }
    }

    // TODO: Shared step 4: "If url matches about:blank and initialInsertion is true,
    // then perform the URL and history update steps given element's content navigable's
    // active document and url."

    // Shared step 5: "Return url." (url is still about:blank from shared step 1.)
    Some(url)
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

/// Implementation hook for cross-origin iframe placeholders used by embedder composition.
/// This is not a spec concept; it installs a token on the DOM node so UI hit-testing and
/// child webview composition can target the correct iframe box.
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

/// <https://html.spec.whatwg.org/#navigate-an-iframe-or-frame>
/// Note: Steps 1–2 (historyHandling determination) and Step 3 (resource-timing bookkeeping)
/// are delegated to the user agent process. The content process sends a NavigationRequested
/// message that triggers user-agent-side navigation of the child navigable, which handles
/// historyHandling and the full navigate algorithm (Step 4 of the spec).
fn navigate_an_iframe_or_frame(
    runtime: &ContentRuntime,
    content_navigable_id: NavigableId,
    parent_traversable_id: u64,
    content_frame_id: FrameId,
    destination_url: &Url,
) -> Result<(), String> {
    // Step 4: "Navigate element's content navigable to url using element's node document."
    // Note: Navigation is performed by sending a request to the user agent, which executes
    // the navigate algorithm including historyHandling and referrer policy resolution.
    runtime
        .event_sender
        .send(ContentEvent::NavigationRequested(NavigateRequest {
            navigation_id: Some(NavigationId::new()),
            source_navigable_id: content_navigable_id,
            chosen_navigable_id: Some(content_navigable_id),
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

    let Some(content_navigable_id) = container_state.content_navigable else {
        return Ok(());
    };

    runtime
        .event_sender
        .send(ContentEvent::IframeTraversableRemoved(IframeTraversableRemoval {
            parent_traversable_id,
            content_navigable_id,
            content_frame_id: container_state.content_frame_id,
        }))
        .map_err(|error| format!("failed to send iframe traversable removal: {error}"))
}

/// <https://html.spec.whatwg.org/#create-a-new-child-navigable>
/// Note: This function implements the content-process portion of the algorithm. Steps that
/// require a browsing context, document creation, or session history manipulation are
/// executed by the user agent after receiving the `CreateChildNavigable` IPC message.
fn create_a_new_child_navigable(
    runtime: &mut ContentRuntime,
    parent_navigable_id: NavigableId,
    parent_document_id: u64,
    iframe_node_id: usize,
) -> Result<(), String> {
    // Early return if this iframe already has a content navigable.
    if runtime
        .documents
        .get(&parent_document_id)
        .and_then(|content_document| {
            content_document
                .navigable_container_states
                .get(&iframe_node_id)
                .and_then(|state| state.content_navigable)
        })
        .is_some()
    {
        return Ok(());
    }

    // Step 1: "Let parentNavigable be element's node navigable."
    let parent_navigable = parent_navigable_id;

    // Step 2: "Let group be element's node document's browsing context's top-level browsing
    // context's group."
    // Note: Executed by the user agent upon receiving `CreateChildNavigable`.

    // Step 3: "Let browsingContext and document be the result of creating a new browsing
    // context and document given element's node document, element, and group."
    // Note: Executed by the user agent upon receiving `CreateChildNavigable`.

    // Step 4: "Let targetName be null."
    let mut target_name = None;

    // Step 5: "If element has a name content attribute, then set targetName to the value
    // of that attribute."
    if let Some(content_document) = runtime.documents.get(&parent_document_id) {
        let document = content_document.document.borrow();
        if let Some(node) = document.get_node(iframe_node_id)
            && let Some(element) = node.element_data()
            && let Some(name_value) = element.attr(local_name!("name"))
        {
            target_name = Some(name_value.to_owned());
        }
    }

    // Step 6: "Let documentState be a new document state, with document, initiator origin,
    // origin, navigable target name, and about base URL."
    // Note: Executed by the user agent upon receiving `CreateChildNavigable`.

    // Step 7: "Let navigable be a new navigable."
    // Note: Content allocates the stable navigable ID here; the user agent materializes
    // the navigable object from that ID.
    let content_navigable = runtime.allocate_navigable_id()?;

    // Step 8: "Initialize the navigable navigable given documentState and parentNavigable."
    // Note: Executed by the user agent upon receiving `CreateChildNavigable`.

    // Step 9: "Set element's content navigable to navigable."
    // Note: The content side stores only IDs for the element's content navigable.
    let content_frame_id = runtime.allocate_child_frame_id();
    let content_frame_token = runtime.allocate_placeholder_frame_token();

    if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
        content_document.navigable_container_states.insert(
            iframe_node_id,
            NavigableContainerState {
                content_navigable: Some(content_navigable),
                content_frame_id,
                content_frame_token,
                current_key: String::new(),
                cross_origin: false,
            },
        );
    }

    // Step 10: "Let historyEntry be navigable's active session history entry."
    // Note: Executed by the user agent upon receiving `CreateChildNavigable`.

    // Step 11: "Let traversable be parentNavigable's traversable navigable."
    let parent_traversable_id = u64::try_from(parent_navigable.0.as_u128())
        .map_err(|_| format!("create-a-new-child-navigable: non-u64 parent navigable id {parent_navigable}"))?;

    // Step 12: "Append the following session history traversal steps to traversable."
    // Note: Executed by the user agent upon receiving `CreateChildNavigable`.

    // Step 13: "Invoke WebDriver BiDi navigable created with traversable."
    // Note: Executed by the user agent upon receiving `CreateChildNavigable`.

    // Note: Notify the user agent to execute Steps 2–8 and 10, 12–13 of
    // `create a new child navigable`.
    runtime
        .event_sender
        .send(ContentEvent::CreateChildNavigable(CreateChildNavigableRequest {
            parent_traversable_id,
            content_navigable_id: content_navigable,
            content_frame_id,
            target_name,
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



/// Implementation hook for same-origin iframe composition used by the local DOM renderer.
/// This is an engine integration detail: it wires the already-loaded child document into
/// Blitz sub-document painting for iframe embedding.
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

    // Step 1: Assert element's content navigable is not null.
    // (Checked by caller before invoking this function.)

    // Step 2: "Let childDocument be element's content navigable's active document."
    // Note: At this point, the user agent has completed creation of the browsing context
    // and the active document exists.

    // Step 3: "If childDocument has its mute iframe load flag set, then return."
    // TODO: Implement document mute iframe load flag tracking.

    let iframe_object = crate::boa::platform_objects::resolve_element_object(
        iframe_node_id,
        &mut content_document.settings.context,
    )
    .map_err(|error| error.to_string())?;

    // Step 4: "If element's pending resource-timing start time is not null, then:"
    // TODO: Implement iframe resource timing.

    // Step 5: "Set childDocument's iframe load in progress flag."
    // TODO: Implement document iframe load in progress flag.

    // Step 6: "Fire an event named load at element."
    fire_event(&mut content_document.settings, &iframe_object, "load", false)
        .map_err(|error| error.to_string())?;

    // Step 7: "Unset childDocument's iframe load in progress flag."
    // TODO: Implement clearing of iframe load in progress flag.

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

/// <https://html.spec.whatwg.org/#the-iframe-element:html-element-post-connection-steps>
fn run_iframe_post_connection_steps(
    runtime: &mut ContentRuntime,
    parent_document_id: u64,
    iframe_node_id: usize,
) -> Result<(), String> {
    // Step 1: "If insertedNode has a sandbox attribute, then parse the sandboxing
    // directive given the attribute's value and insertedNode's iframe sandboxing flag set."
    // <https://html.spec.whatwg.org/#parse-a-sandboxing-directive>
    if let Some(content_document) = runtime.documents.get(&parent_document_id) {
        let document = content_document.document.borrow();
        if let Some(node) = document.get_node(iframe_node_id)
            && let Some(element) = node.element_data()
            && let Some(sandbox_value) = element.attr(local_name!("sandbox"))
        {
            let _iframe_sandboxing_flag_set = parse_a_sandboxing_directive(sandbox_value);
            // TODO: Persist the parsed sandboxing flag set on the iframe element/container.
        }
    }

    // Step 2: "Create a new child navigable for insertedNode."
    let parent_navigable_id = runtime
        .documents
        .get(&parent_document_id)
        .map(|content_document| NavigableId::from_u128(content_document.traversable_id as u128))
        .ok_or_else(|| format!("missing parent document {parent_document_id}"))?;
    create_a_new_child_navigable(
        runtime,
        parent_navigable_id,
        parent_document_id,
        iframe_node_id,
    )?;

    // Step 3: "Process the iframe attributes for insertedNode, with initialInsertion
    // set to true."
    // Note: This executes synchronously in the same task as Step 2.
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

/// <https://html.spec.whatwg.org/#html-element-removing-steps>
/// Note: The spec says to destroy a child navigable given `removedNode`. Content-side
/// implementation: retires the cross-origin traversable if present, removes the iframe
/// subdocument, and clears the navigable container state. The user agent completes the
/// full destroy-a-child-navigable algorithm upon receiving the IframeTraversableRemoved message.
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
    let (creation_url, parent_traversable_id, srcdoc_value, desired_url) = {
        let Some(content_document) = runtime.documents.get(&parent_document_id) else {
            return Ok(());
        };
        let creation_url = content_document.settings.creation_url.clone();
        let document = content_document.document.borrow();
        let Some(node) = document.get_node(iframe_node_id) else {
            return Ok(());
        };
        let Some(element) = node.element_data() else {
            return Ok(());
        };
        let srcdoc_value = element.attr(local_name!("srcdoc")).map(str::to_owned);

        // Step 2.1: "Let url be the result of running the shared attribute processing steps
        // for iframe and frame elements given element and initialInsertion."
        let desired_url = shared_attribute_processing_steps_for_iframe_and_frame_elements(
            &document,
            &creation_url,
            iframe_node_id,
            initial_insertion,
        );

        (
            creation_url,
            content_document.traversable_id,
            srcdoc_value,
            desired_url,
        )
    };

    let target = if let Some(srcdoc_html) = srcdoc_value.clone() {
        IframeNavigationTarget::Srcdoc { html: srcdoc_html }
    } else {
        let Some(url) = desired_url else {
            // Step 2.2: "If url is null, then return."
            return Ok(());
        };
        if matches_about_blank(&url) {
            IframeNavigationTarget::AboutBlank
        } else {
            IframeNavigationTarget::Url { url }
        }
    };

    let desired_key = match &target {
        IframeNavigationTarget::Srcdoc { html } => format!("srcdoc:{html}"),
        IframeNavigationTarget::AboutBlank => String::from("about:blank"),
        IframeNavigationTarget::Url { url } => format!("src:{url}"),
    };

    // Note: The spec checks whether the pending-navigations map already contains an entry
    // for this URL. We track the last-committed key (`current_key`) as an approximation:
    // if the desired key is unchanged since the last navigation, no new navigation is needed.
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

    // Step 2: "If element's content navigable is null, then return."
    let Some(content_navigable_id) = previous_iframe_state
        .as_ref()
        .and_then(|state| state.content_navigable)
    else {
        return Ok(());
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

    // Note: Determine whether the navigation is cross-origin for cross-origin frame setup.
    let cross_origin =
        matches!(&target, IframeNavigationTarget::Url { url } if creation_url.origin() != url.origin());

    // Note: If transitioning from cross-origin to same-origin, retire the old traversable
    // so the user agent can clean up the cross-origin child navigable.
    if let Some(previous_iframe_state) = previous_iframe_state.as_ref() {
        if previous_iframe_state.cross_origin && !cross_origin {
            retire_iframe_traversable(runtime, parent_traversable_id, previous_iframe_state)?;
        }
    }

    // Step 2.3: "If url matches about:blank and initialInsertion is true:"
    // Note: Applies to the `AboutBlank` target (no `src`/`srcdoc`) and to `src` values
    // that parse to about:blank. The `Srcdoc` target is never about:blank, so it falls
    // through to the navigation block below.
    if initial_insertion {
        match &target {
            IframeNavigationTarget::AboutBlank => {
                if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
                    content_document.navigable_container_states.insert(
                        iframe_node_id,
                        NavigableContainerState {
                            content_navigable: Some(content_navigable_id),
                            content_frame_id,
                            content_frame_token,
                            current_key: desired_key.clone(),
                            cross_origin: false,
                        },
                    );
                }
                // Step 2.3.1: "Run the iframe load event steps given element."
                run_iframe_load_event_steps(runtime, parent_document_id, iframe_node_id)?;
                // Step 2.3.2: "Return."
                return Ok(());
            }
            IframeNavigationTarget::Url { url } if matches_about_blank(url) => {
                if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
                    content_document.navigable_container_states.insert(
                        iframe_node_id,
                        NavigableContainerState {
                            content_navigable: Some(content_navigable_id),
                            content_frame_id,
                            content_frame_token,
                            current_key: desired_key.clone(),
                            cross_origin: false,
                        },
                    );
                }
                // Step 2.3.1: "Run the iframe load event steps given element."
                run_iframe_load_event_steps(runtime, parent_document_id, iframe_node_id)?;
                // Step 2.3.2: "Return."
                return Ok(());
            }
            _ => {}
        }
    }

    // Note: Record the current navigation key and cross-origin flag before navigating,
    // so re-entrant attribute changes do not trigger a redundant navigation.
    if let Some(content_document) = runtime.documents.get_mut(&parent_document_id) {
        content_document.navigable_container_states.insert(
            iframe_node_id,
            NavigableContainerState {
                content_navigable: Some(content_navigable_id),
                content_frame_id,
                content_frame_token,
                current_key: desired_key.clone(),
                cross_origin,
            },
        );
    }

    // Step 2.4: "Let referrerPolicy be the current state of element's referrerpolicy
    // content attribute."
    // TODO: Pass referrerPolicy through to navigate_an_iframe_or_frame.

    // Step 2.5: "Set element's current navigation was lazy loaded boolean to false."
    // TODO: Implement current navigation was lazy loaded tracking.

    // Step 2.6: "If the will lazy load element steps given element return true:"
    // TODO: Implement lazy loading for iframes with URL navigations.

    // Step 1: "If element's srcdoc attribute is specified:"
    if let IframeNavigationTarget::Srcdoc { html } = &target {
        // Step 1.1: "Set element's current navigation was lazy loaded boolean to false."
        // TODO: Implement current navigation was lazy loaded tracking.

        // Step 1.2: "If the will lazy load element steps given element return true:"
        // TODO: Implement lazy loading.

        // Step 1.3: "Navigate to the srcdoc resource."
        attach_iframe_subdocument_from_html(
            runtime,
            parent_document_id,
            iframe_node_id,
            creation_url.to_string(),
            html.clone(),
        )?;
        run_iframe_load_event_steps(runtime, parent_document_id, iframe_node_id)?;
        return Ok(());
    }

    // Step 2.7: "Navigate an iframe or frame given element, url[, referrerPolicy]."
    // <https://html.spec.whatwg.org/#navigate-an-iframe-or-frame>
    // Note: about:blank navigations are fulfilled locally and URL-based navigations are
    // delegated to the user agent.
    match target {
        IframeNavigationTarget::AboutBlank => {
            attach_iframe_about_blank(runtime, parent_document_id, iframe_node_id)?;
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
            navigate_an_iframe_or_frame(
                runtime,
                content_navigable_id,
                parent_traversable_id,
                content_frame_id,
                &url,
            )?;
        }
        IframeNavigationTarget::Srcdoc { .. } => {}
    }

    Ok(())
}

/// <https://html.spec.whatwg.org/#parse-a-sandboxing-directive>
fn parse_a_sandboxing_directive(value: &str) -> HashSet<String> {
    // Step 1: "Let output be a new empty set."
    let mut output = HashSet::new();

    // Step 2: "Let tokens be the result of splitting input on ASCII whitespace."
    let tokens = value.split_ascii_whitespace();

    // Step 3: "For each token of tokens:"
    for token in tokens {
        // TODO: Map each sandboxing keyword token to the corresponding sandboxing flag.
        output.insert(token.to_ascii_lowercase());
    }

    // Step 4: "Return output."
    output
}