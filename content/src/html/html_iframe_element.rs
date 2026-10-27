use ipc_messages::content::{
    DocumentId, Event as ContentEvent, FrameId, IframeTraversableRemoval, NavigableId,
    NewChildNavigableInfo, UserNavigationInvolvement, iframe_target_name,
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use blitz_dom::BaseDocument;
use html5ever::{local_name, ns};
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::{ExecutionContext, gc_struct};
use url::Url;

use crate::{
    ContentDocument, ContentProcess, NavigableContainerState, dom::event::EventTargetAccess,
    dom::fire_event, html::HTMLElement, html::create_a_new_browsing_context_and_document,
    html::environment_settings_object::RealmWiring, html::navigate, js::Types,
    js::platform_objects::with_global_scope, webidl::Callback,
};

/// <https://html.spec.whatwg.org/#htmliframeelement>
#[gc_struct]
pub struct HTMLIFrameElement {
    /// <https://html.spec.whatwg.org/#htmlelement>
    pub html_element: HTMLElement,

    /// <https://html.spec.whatwg.org/#handler-onload>
    onload: GcCell<Option<Callback>>,

    /// <https://html.spec.whatwg.org/#handler-onerror>
    onerror: GcCell<Option<Callback>>,
}

impl EventTargetAccess for HTMLIFrameElement {
    fn get_event_target(&self, ec: &mut dyn ExecutionContext<Types>) -> crate::dom::EventTarget {
        self.html_element.element.node.get_event_target(ec)
    }
}

impl HTMLIFrameElement {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            html_element: HTMLElement::new(document, node_id, ec),
            onload: gc_cell_new(None, ec),
            onerror: gc_cell_new(None, ec),
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
        // Step 1: "Let attribute be the result of getting an attribute by namespace and local name
        // given null, srcdoc's local name, and this."
        // Step 2: "If attribute is null, then return the empty string."
        // Step 3: "Return attribute's value."
        self.html_element
            .element
            .get_attribute("srcdoc")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-iframe-srcdoc>
    pub(crate) fn set_srcdoc(&self, srcdoc: &str) {
        // Step 1: "Let compliantString be the result of invoking the get trusted type compliant
        // string algorithm with TrustedHTML, this's relevant global object, the given value,
        // "HTMLIFrameElement srcdoc", and "script"."
        // Note: Trusted Types are not implemented; the given string is used directly.
        // Step 2: "Set an attribute value given this, srcdoc's local name, and compliantString."
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

    /// <https://html.spec.whatwg.org/#handler-onload>
    pub(crate) fn onload_value(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<Callback> {
        self.onload.get(ec)
    }

    /// <https://html.spec.whatwg.org/#handler-onload>
    pub(crate) fn replace_onload(
        &self,
        callback: Option<Callback>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<Callback> {
        self.onload.replace(callback, ec)
    }

    /// <https://html.spec.whatwg.org/#handler-onerror>
    pub(crate) fn onerror_value(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<Callback> {
        self.onerror.get(ec)
    }

    /// <https://html.spec.whatwg.org/#handler-onerror>
    pub(crate) fn replace_onerror(
        &self,
        callback: Option<Callback>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<Callback> {
        self.onerror.replace(callback, ec)
    }
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

/// <https://html.spec.whatwg.org/#navigate-an-iframe-or-frame>
/// Note: Steps 1–2 (historyHandling determination) and Step 3 (resource-timing bookkeeping)
/// are delegated to the user agent process. The content process sends a NavigationRequested
/// message that triggers user-agent-side navigation of the child navigable, which handles
/// historyHandling and the full navigate algorithm (Step 4 of the spec).
fn navigate_an_iframe_or_frame(
    process: &ContentProcess,
    content_navigable_id: NavigableId,
    parent_traversable_id: NavigableId,
    content_frame_id: FrameId,
    destination_url: &Url,
    srcdoc_html: Option<String>,
) -> Result<(), String> {
    // Step 4: "Navigate element's content navigable to url using element's node document."
    // Note: Navigation is performed by sending a request to the user agent, which executes
    // the navigate algorithm including historyHandling and referrer policy resolution.
    navigate(
        &process.event_sender,
        content_navigable_id,
        Some(content_navigable_id),
        destination_url.to_string(),
        iframe_target_name(
            parent_traversable_id,
            content_navigable_id,
            content_frame_id,
        ),
        UserNavigationInvolvement::None,
        false,
        None,
        None,
        None,
        None,
        srcdoc_html,
    )
}

pub(crate) fn retire_iframe_traversable(
    process: &ContentProcess,
    parent_traversable_id: NavigableId,
    container_state: &NavigableContainerState,
) -> Result<(), String> {
    let Some(content_navigable_id) = container_state.content_navigable else {
        return Ok(());
    };

    process
        .event_sender
        .send(ContentEvent::IframeTraversableRemoved(
            IframeTraversableRemoval {
                parent_traversable_id,
                content_navigable_id,
                content_frame_id: container_state.content_frame_id,
            },
        ))
        .map_err(|error| format!("failed to send iframe traversable removal: {error}"))
}

/// <https://html.spec.whatwg.org/#create-a-new-child-navigable>
fn create_a_new_child_navigable(
    process: &mut ContentProcess,
    parent_navigable_id: NavigableId,
    parent_document_id: DocumentId,
    iframe_node_id: usize,
) -> Result<(), String> {
    // Early return if this iframe already has a content navigable.
    if process
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

    // Step 2: "Let group be element's node document's browsing context's top-level
    // browsing context's group."
    // Note: The browsing context group is user-agent state; the content process
    // has no notion of it beyond being the same process. The UA resolves the
    // parent's group when it catches up navigable state
    // (`UserAgent::create_a_new_child_navigable`).
    // Step 3: "Let browsingContext and document be the result of creating a new browsing
    // context and document given element's node document, element, and group."
    // Note: Content-process portion of this step: the document, realm, Window
    // and environment settings object are created below via
    // `create_a_new_browsing_context_and_document` (steps 10, 13, 15, 22 of
    // "creating a new browsing context and document"); the browsing-context
    // allocation, group membership, agent and session-history steps run in the
    // user agent when it handles the CreateChildNavigable IPC
    // (`UserAgent::create_a_new_child_navigable`), before navigation.
    let content_navigable = process.allocate_navigable_id()?;
    let new_document_id = DocumentId::new();
    let top_level_traversable_id = process
        .documents
        .get(&parent_document_id)
        .map(|doc| doc.top_level_traversable_id)
        .unwrap_or(parent_navigable);

    let parent_origin = process
        .documents
        .get(&parent_document_id)
        .map(|doc| doc.settings.origin.clone());
    let content_frame_id = process.allocate_child_frame_id();
    let event_sender = process.event_sender.clone();
    let task_sources = process.event_loop_task_sources();
    let parent_engine = &mut process
        .documents
        .get_mut(&parent_document_id)
        .ok_or_else(|| format!("missing parent document {parent_document_id}"))?
        .settings
        .realm_execution_context;
    let (global_object, window, settings, new_document) =
        create_a_new_browsing_context_and_document(
            Some(parent_engine),
            parent_origin,
            RealmWiring {
                source_navigable_id: content_navigable,
                document_id: new_document_id,
                event_sender: event_sender.clone(),
                task_sources,
            },
        )?;

    // Register the document in ContentProcess immediately. A fresh dirty
    // flag: this child navigable's render is driven by content command
    // handlers (event/script dispatch); resource-load re-notes via the
    // shell provider are not connected for dynamically-created documents.
    let needs_paint = Rc::new(Cell::new(false));
    process.documents.insert(
        new_document_id,
        ContentDocument {
            traversable_id: content_navigable,
            parent_traversable_id: Some(parent_navigable),
            top_level_traversable_id,
            frame_id: content_frame_id,
            document: new_document,
            settings,
            providers: None,
            pending_document_load: None,
            navigable_container_states: HashMap::new(),
            viewport_offset_x: 0.0,
            viewport_offset_y: 0.0,
            needs_paint,
            last_scene: None,
            last_composition: None,
        },
    );
    process
        .active_documents_by_traversable
        .insert(content_navigable, new_document_id);

    // Step 4: "Let targetName be null."
    // Step 5: "If element has a name content attribute, then set targetName to the value
    // of that attribute."
    let target_name = if let Some(content_document) = process.documents.get(&parent_document_id) {
        let document = content_document.document.borrow();
        if let Some(node) = document.get_node(iframe_node_id)
            && let Some(element) = node.element_data()
            && let Some(name_value) = element.attr(local_name!("name"))
        {
            Some(name_value.to_owned())
        } else {
            None
        }
    } else {
        None
    };

    // Step 6: "Let documentState be a new document state, with document, initiator origin,
    // origin, navigable target name, and about base URL."
    // Note: The document state fields that live in content (document reference, origin) are
    // already satisfied by the document created above. The UA-side document state (navigable
    // tree registration, session history) is set up in the CreateChildNavigable handler.
    // Step 7: "Let navigable be a new navigable."
    // Note: The navigable id was allocated above as `content_navigable`.

    // Step 8: "Initialize the navigable navigable given documentState and parentNavigable."
    // Note: The UA initializes its navigable state upon receiving CreateChildNavigable.
    // The content side already has the document created and registered.
    // Step 9: "Set element's content navigable to navigable."
    if let Some(content_document) = process.documents.get_mut(&parent_document_id) {
        content_document.navigable_container_states.insert(
            iframe_node_id,
            NavigableContainerState {
                content_navigable: Some(content_navigable),
                content_frame_id,
                current_key: String::new(),
                child_document_loaded: false,
            },
        );
        // Register the iframe node -> content navigable mapping on the
        // parent realm's GlobalScope so the contentWindow binding can hand
        // out the child navigable's WindowProxy, backed by the child
        // document's Window created in this process.
        with_global_scope(
            &mut content_document.settings.realm_execution_context,
            |global_scope, ec| {
                global_scope.register_iframe_content_navigable(
                    iframe_node_id,
                    content_navigable,
                    Some((window.clone(), global_object.clone())),
                    ec,
                )
            },
        )
        .map_err(|error| {
            format!(
                "failed to register iframe content navigable: {}",
                error.display()
            )
        })?;
    }

    // Step 10: "Let historyEntry be navigable's active session history entry."
    // Step 11: "Let traversable be parentNavigable's traversable navigable."
    // Step 12: "Append the following session history traversal steps to traversable."
    // Step 13: "Invoke WebDriver BiDi navigable created with traversable."
    //
    // Note: Steps 10-13 are delegated to the UA via the `new_child_navigable`
    // field on the NavigateRequest: `UserAgent::create_a_new_child_navigable`
    // catches up the navigable-owning state (browsing context, BCG membership,
    // navigable state, session history entries, event loop and graphics
    // registration) before the navigation proceeds.  The navigation itself is
    // the initial `about:blank` navigation; it creates the child's active
    // document in the same content process, reusing the realm created above
    // (initialise-the-document-object step 6).
    let parent_traversable_id = parent_navigable;
    let child_navigable_info = NewChildNavigableInfo {
        parent_traversable_id,
        content_navigable_id: content_navigable,
        content_frame_id,
        document_id: new_document_id,
        target_name,
    };
    navigate(
        &process.event_sender,
        content_navigable,
        Some(content_navigable),
        String::from("about:blank"),
        iframe_target_name(parent_traversable_id, content_navigable, content_frame_id),
        UserNavigationInvolvement::None,
        false,
        None,
        None,
        None,
        Some(child_navigable_info),
        None,
    )
}

fn navigable_container_for_child_navigable(
    process: &ContentProcess,
    content_frame_id: FrameId,
) -> Option<(DocumentId, usize)> {
    process
        .documents
        .iter()
        .find_map(|(parent_document_id, content_document)| {
            content_document.navigable_container_states.iter().find_map(
                |(iframe_node_id, container_state)| {
                    (container_state.content_frame_id == content_frame_id)
                        .then_some((*parent_document_id, *iframe_node_id))
                },
            )
        })
}

/// <https://html.spec.whatwg.org/#iframe-load-event-steps>
fn run_iframe_load_event_steps(
    process: &mut ContentProcess,
    parent_document_id: DocumentId,
    iframe_node_id: usize,
) -> Result<(), String> {
    let Some(content_document) = process.documents.get_mut(&parent_document_id) else {
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
    let iframe_object = crate::js::platform_objects::resolve_element_object(
        iframe_node_id,
        &mut content_document.settings.realm_execution_context,
    )
    .map_err(|error| error.display().to_string())?;

    // Step 4: "If element's pending resource-timing start time is not null, then:"
    // TODO: Implement iframe resource timing.
    // Step 5: "Set childDocument's iframe load in progress flag."
    // TODO: Implement document iframe load in progress flag.

    // Step 6: "Fire an event named load at element."
    let time_millis = content_document.settings.current_time_millis();
    let ec = &mut content_document.settings.realm_execution_context;

    let event_target = ec
        .with_object_any(&iframe_object)
        .and_then(|data| {
            data.downcast_ref::<crate::html::HTMLIFrameElement>()
                .cloned()
        })
        .map(|iframe| iframe.get_event_target(ec))
        .ok_or_else(|| {
            let msg = "run_iframe_load_event_steps: iframe_object is not an HTMLIFrameElement"
                .to_string();
            log::error!("{msg}");
            msg
        })?;

    fire_event(ec, &event_target, "load", time_millis, false)
        .map_err(|error| format!("fire_event failed: {error:?}"))?;

    // Step 7: "Unset childDocument's iframe load in progress flag."
    // TODO: Implement clearing of iframe load in progress flag.
    Ok(())
}

/// <https://html.spec.whatwg.org/#completely-finish-loading>
pub(crate) fn run_iframe_load_event_steps_for_traversable(
    process: &mut ContentProcess,
    traversable_id: NavigableId,
) -> Result<(), String> {
    // The document finished loading, so per "completely finish loading" step 4
    // ("If container is an iframe element, then queue an element task on the
    // DOM manipulation task source given container to run the iframe load event
    // steps given container") the iframe load event steps run on this
    // document's container.  The queued element task runs only after the
    // container document's parser-blocking scripts have executed ("the end"
    // step 5 executes the scripts that will execute when the document has
    // finished parsing).  Here the container's parser-discovered scripts are
    // deferred to its document load completion instead, so while the
    // container's load has not completed yet (pending_document_load is still
    // set) the iframe load event steps are held back and fired by the
    // container's load completion (fire_deferred_iframe_load_events) once its
    // scripts have run.
    let Some(document_id) = process
        .active_documents_by_traversable
        .get(&traversable_id)
        .copied()
    else {
        return Ok(());
    };
    let Some(content_frame_id) = process
        .documents
        .get(&document_id)
        .map(|document| document.frame_id)
    else {
        return Ok(());
    };
    let Some((parent_document_id, iframe_node_id)) =
        navigable_container_for_child_navigable(process, content_frame_id)
    else {
        return Ok(());
    };

    let parent_not_loaded = process
        .documents
        .get(&parent_document_id)
        .map(|document| document.pending_document_load.is_some())
        .unwrap_or(false);
    if parent_not_loaded {
        if let Some(content_document) = process.documents.get_mut(&parent_document_id)
            && let Some(state) = content_document
                .navigable_container_states
                .get_mut(&iframe_node_id)
        {
            state.child_document_loaded = true;
        }
        return Ok(());
    }

    run_iframe_load_event_steps(process, parent_document_id, iframe_node_id)
}

pub(crate) fn fire_deferred_iframe_load_events(
    process: &mut ContentProcess,
    parent_document_id: DocumentId,
) -> Result<(), String> {
    // Preserves the ordering that "completely finish loading" step 4 gives via
    // its queued element task: the iframe load event steps run only after the
    // container document's parser-discovered scripts, which this implementation
    // defers to the parent's document load completion ("the end", steps 5-9).
    // Called by the parent's load completion.
    let pending: Vec<usize> = {
        let Some(content_document) = process.documents.get(&parent_document_id) else {
            return Ok(());
        };
        content_document
            .navigable_container_states
            .iter()
            .filter(|(_, state)| state.child_document_loaded)
            .map(|(node_id, _)| *node_id)
            .collect()
    };
    for iframe_node_id in pending {
        if let Some(content_document) = process.documents.get_mut(&parent_document_id)
            && let Some(state) = content_document
                .navigable_container_states
                .get_mut(&iframe_node_id)
        {
            state.child_document_loaded = false;
        }
        run_iframe_load_event_steps(process, parent_document_id, iframe_node_id)?;
    }
    Ok(())
}

/// <https://html.spec.whatwg.org/#the-iframe-element:html-element-post-connection-steps>
pub(crate) fn run_iframe_post_connection_steps(
    process: &mut ContentProcess,
    parent_document_id: DocumentId,
    iframe_node_id: usize,
) -> Result<(), String> {
    // Step 1: "If insertedNode's node document's browsing context is null, then return."
    // Note: A document created by the user agent has a browsing context; a
    // document missing from this process's table no longer has one.
    if !process.documents.contains_key(&parent_document_id) {
        return Ok(());
    }
    // Step 2: "If insertedNode has a sandbox attribute, then parse the sandboxing directive
    // given the attribute's value and insertedNode's iframe sandboxing flag set."
    // <https://html.spec.whatwg.org/#parse-a-sandboxing-directive>
    if let Some(content_document) = process.documents.get(&parent_document_id) {
        let document = content_document.document.borrow();
        if let Some(node) = document.get_node(iframe_node_id)
            && let Some(element) = node.element_data()
            && let Some(sandbox_value) = element.attr(local_name!("sandbox"))
        {
            let _iframe_sandboxing_flag_set = parse_a_sandboxing_directive(sandbox_value);
            // TODO: Persist the parsed sandboxing flag set on the iframe element/container.
        }
    }
    // Step 3: "Create a new child navigable for insertedNode."
    let parent_navigable_id = process
        .documents
        .get(&parent_document_id)
        .map(|content_document| content_document.traversable_id)
        .ok_or_else(|| format!("missing parent document {parent_document_id}"))?;
    create_a_new_child_navigable(
        process,
        parent_navigable_id,
        parent_document_id,
        iframe_node_id,
    )?;
    // Step 4: "Process the iframe attributes for insertedNode, with initialInsertion set to true."
    process_iframe_attributes(process, parent_document_id, iframe_node_id, true)
}

pub(crate) fn run_iframe_post_connection_steps_for_document(
    process: &mut ContentProcess,
    parent_document_id: DocumentId,
) -> Result<(), String> {
    let iframe_node_ids = {
        let Some(content_document) = process.documents.get(&parent_document_id) else {
            return Ok(());
        };
        let document = content_document.document.borrow();
        connected_iframe_node_ids(&document)
    };

    for iframe_node_id in iframe_node_ids {
        run_iframe_post_connection_steps(process, parent_document_id, iframe_node_id)?;
    }

    Ok(())
}

/// <https://html.spec.whatwg.org/#html-element-removing-steps>
pub(crate) fn run_iframe_removing_steps(
    process: &mut ContentProcess,
    parent_document_id: DocumentId,
    iframe_node_id: usize,
) -> Result<(), String> {
    // "The iframe HTML element removing steps, given removedNode, are to destroy a child
    // navigable given removedNode."
    // Note: The content process retires the traversable and clears the navigable
    // container state; the user agent runs the full destroy-a-child-navigable
    // algorithm when it receives the IframeTraversableRemoved message.  The DOM
    // hook records this step (it cannot reach the navigable state) and the content
    // process runs it at the end of the current operation rather than inside the
    // removal.
    let Some((parent_traversable_id, iframe_state)) = process
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

    retire_iframe_traversable(process, parent_traversable_id, &iframe_state)?;
    if let Some(content_document) = process.documents.get_mut(&parent_document_id) {
        // Unregister the iframe node -> content navigable mapping.
        with_global_scope(
            &mut content_document.settings.realm_execution_context,
            |global_scope, ec| {
                global_scope.unregister_iframe_content_navigable(iframe_node_id, ec);
                Ok(())
            },
        )
        .map_err(|error| {
            format!(
                "failed to unregister iframe content navigable: {}",
                error.display()
            )
        })?;
        content_document
            .navigable_container_states
            .remove(&iframe_node_id);
    }
    Ok(())
}

pub(crate) fn run_iframe_removing_steps_for_document(
    process: &mut ContentProcess,
    parent_document_id: DocumentId,
) -> Result<(), String> {
    let iframe_node_ids = {
        let Some(content_document) = process.documents.get(&parent_document_id) else {
            return Ok(());
        };
        let document = content_document.document.borrow();
        connected_iframe_node_ids(&document)
    };

    for iframe_node_id in iframe_node_ids {
        run_iframe_removing_steps(process, parent_document_id, iframe_node_id)?;
    }

    Ok(())
}

pub(crate) fn process_iframe_attributes(
    process: &mut ContentProcess,
    parent_document_id: DocumentId,
    iframe_node_id: usize,
    initial_insertion: bool,
) -> Result<(), String> {
    let Some(parent_traversable_id) = process
        .documents
        .get(&parent_document_id)
        .map(|content_document| content_document.traversable_id)
    else {
        return Ok(());
    };

    // Step 1: "If element's srcdoc attribute is specified:"
    let srcdoc_html = {
        let Some(content_document) = process.documents.get(&parent_document_id) else {
            return Ok(());
        };
        let document = content_document.document.borrow();
        document
            .get_node(iframe_node_id)
            .and_then(|node| node.element_data())
            .and_then(|element| element.attr(local_name!("srcdoc")).map(str::to_owned))
    };
    if let Some(srcdoc_html) = srcdoc_html {
        // Step 1.1: "Set element's current navigation was lazy loaded boolean to false."
        // TODO: Implement current navigation was lazy loaded tracking.
        // Step 1.2: "If the will lazy load element steps given element return true:"
        // TODO: Implement lazy loading for srcdoc iframes.
        // Step 1.3: "Navigate to the srcdoc resource: Navigate an iframe or frame given element, about:srcdoc, the empty string, and the value of element's srcdoc attribute."
        // Note: The content process sends the srcdoc body to the user agent,
        // which answers the `about:srcdoc` navigation with it.  The document is
        // created like any other navigation's document, in this process because
        // `about:srcdoc` inherits the parent's origin.
        let desired_key = format!("srcdoc:{srcdoc_html}");
        let Some((content_navigable_id, content_frame_id)) = record_container_navigation_key(
            process,
            parent_document_id,
            iframe_node_id,
            desired_key,
        ) else {
            return Ok(());
        };
        let srcdoc_url = Url::parse("about:srcdoc").map_err(|error| error.to_string())?;
        return navigate_an_iframe_or_frame(
            process,
            content_navigable_id,
            parent_traversable_id,
            content_frame_id,
            &srcdoc_url,
            Some(srcdoc_html),
        );
    }

    // Step 2: "Otherwise:"
    // Step 2.1: "Let url be the result of running the shared attribute processing steps for iframe and frame elements given element and initialInsertion."
    let desired_url = {
        let Some(content_document) = process.documents.get(&parent_document_id) else {
            return Ok(());
        };
        let creation_url = content_document.settings.creation_url.clone();
        let document = content_document.document.borrow();
        shared_attribute_processing_steps_for_iframe_and_frame_elements(
            &document,
            &creation_url,
            iframe_node_id,
            initial_insertion,
        )
    };
    // Step 2.2: "If url is null, then return."
    let Some(url) = desired_url else {
        return Ok(());
    };
    // Step 2.3: "If url matches about:blank and initialInsertion is true:"
    if matches_about_blank(&url) && initial_insertion {
        // Step 2.3.1: "Run the iframe load event steps given element."
        // Note: The child navigable's initial `about:blank` navigation (sent by
        // `create_a_new_child_navigable`) creates the document; the iframe load
        // event steps run when that document finishes loading, through
        // `run_iframe_load_event_steps_for_traversable`.
        record_container_navigation_key(
            process,
            parent_document_id,
            iframe_node_id,
            String::from("about:blank"),
        );
        // Step 2.3.2: "Return."
        return Ok(());
    }
    // Step 2.4: "Let referrerPolicy be the current state of element's referrerpolicy content attribute."
    // TODO: Pass referrerPolicy through to navigate_an_iframe_or_frame.
    // Step 2.5: "Set element's current navigation was lazy loaded boolean to false."
    // TODO: Implement current navigation was lazy loaded tracking.
    // Step 2.6: "If the will lazy load element steps given element return true:"
    // TODO: Implement lazy loading for iframes with URL navigations.
    // Step 2.7: "Navigate: Navigate an iframe or frame given element, url, referrerPolicy, null, and initialInsertion."
    // <https://html.spec.whatwg.org/#navigate-an-iframe-or-frame>
    let desired_key = if matches_about_blank(&url) {
        String::from("about:blank")
    } else {
        format!("src:{url}")
    };
    let Some((content_navigable_id, content_frame_id)) =
        record_container_navigation_key(process, parent_document_id, iframe_node_id, desired_key)
    else {
        return Ok(());
    };
    navigate_an_iframe_or_frame(
        process,
        content_navigable_id,
        parent_traversable_id,
        content_frame_id,
        &url,
        None,
    )
}

/// Records a container's current navigation key and returns the child navigable
/// and frame the navigation renders into.  Returns `None` when the element has
/// no content navigable yet, or when the key is unchanged, so the caller does
/// not navigate again.  Serves steps 1.3 and 2.7 of
/// <https://html.spec.whatwg.org/#process-the-iframe-attributes>.
fn record_container_navigation_key(
    process: &mut ContentProcess,
    parent_document_id: DocumentId,
    iframe_node_id: usize,
    desired_key: String,
) -> Option<(NavigableId, FrameId)> {
    // Note: The spec checks whether the pending-navigations map already
    // contains an entry for this URL.  The last-committed key (`current_key`)
    // is tracked instead: if the desired key is unchanged since the last
    // navigation, no new navigation is needed.
    let previous_iframe_state =
        process
            .documents
            .get(&parent_document_id)
            .and_then(|content_document| {
                content_document
                    .navigable_container_states
                    .get(&iframe_node_id)
                    .cloned()
            });
    if previous_iframe_state
        .as_ref()
        .is_some_and(|state| state.current_key == desired_key)
    {
        return None;
    }
    let content_navigable_id = previous_iframe_state
        .as_ref()
        .and_then(|state| state.content_navigable)?;
    let content_frame_id = previous_iframe_state?.content_frame_id;
    let content_document = process.documents.get_mut(&parent_document_id)?;
    content_document.navigable_container_states.insert(
        iframe_node_id,
        NavigableContainerState {
            content_navigable: Some(content_navigable_id),
            content_frame_id,
            current_key: desired_key,
            child_document_loaded: false,
        },
    );
    Some((content_navigable_id, content_frame_id))
}

/// <https://html.spec.whatwg.org/#parse-a-sandboxing-directive>
fn parse_a_sandboxing_directive(input: &str) -> HashSet<String> {
    // Step 1: "Split input on ASCII whitespace, to obtain tokens."
    let tokens = input.split_ascii_whitespace();
    // Step 2: "Let output be empty."
    let mut output = HashSet::new();
    // Step 3: "Add the following flags to output: ..."
    // Note: The sandboxing flag set is not modeled; the output collects the
    // tokens instead, and the parsed set is discarded by the caller.
    for token in tokens {
        output.insert(token.to_ascii_lowercase());
    }
    output
}
