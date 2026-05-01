mod element;

use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use html5ever::{local_name, ns};
use ipc_messages::content::{ChildNavigableCreation, Event as ContentEvent, IframeTraversableRemoval, NavigateRequest, UserNavigationInvolvement};
use url::Url;

use crate::{ContentRuntime, EMPTY_HTML_DOCUMENT, IframeState};

pub use element::HTMLIFrameElement;

enum IframeNavigationTarget {
    AboutBlank,
    Srcdoc { html: String },
    Url { url: Url },
}

fn matches_about_blank(url: &Url) -> bool {
    url.scheme() == "about" && url.path() == "blank"
}

fn iframe_target_description(target: &IframeNavigationTarget) -> String {
    match target {
        IframeNavigationTarget::AboutBlank => String::from("about:blank"),
        IframeNavigationTarget::Srcdoc { html } => format!("srcdoc(len={})", html.len()),
        IframeNavigationTarget::Url { url } => format!("url={url}"),
    }
}

impl ContentRuntime {
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

        Some(Self::iframe_navigation_target_for_attributes(
            creation_url,
            element.attr(local_name!("src")),
            element.attr(local_name!("srcdoc")),
        ))
    }

    fn attach_iframe_subdocument_from_html(
        &mut self,
        parent_document_id: u64,
        iframe_node_id: usize,
        base_url: String,
        html: String,
    ) -> Result<(), String> {
        let sub_document = Rc::new(RefCell::new(BaseDocument::new(
            self.document_config(parent_document_id, Some(base_url)),
        )));
        {
            let mut sub_document_guard = sub_document.borrow_mut();
            super::parse_html_into_document(&mut sub_document_guard, &html);
        }

        let Some(content_document) = self.documents.get_mut(&parent_document_id) else {
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
        &mut self,
        parent_document_id: u64,
        iframe_node_id: usize,
        frame_id: u64,
    ) -> Result<(), String> {
        crate::log_iframe_debug(format!(
            "attach_cross_origin_iframe parent_doc={} node={} frame={}",
            parent_document_id, iframe_node_id, frame_id
        ));
        let Some(content_document) = self.documents.get_mut(&parent_document_id) else {
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
        &self,
        parent_document_id: u64,
        source_navigable_id: u64,
        destination_url: &Url,
    ) -> Result<(), String> {
        crate::log_iframe_debug(format!(
            "request_iframe_navigation source={} destination={}",
            source_navigable_id, destination_url
        ));
        let Some(parent_traversable_id) = self
            .documents
            .get(&parent_document_id)
            .map(|d| d.traversable_id)
        else {
            return Err(format!(
                "request_iframe_navigation: parent document {parent_document_id} not found"
            ));
        };
        self.event_sender
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
        &mut self,
        parent_document_id: u64,
        iframe_node_id: usize,
    ) -> Result<(), String> {
        self.attach_iframe_subdocument_from_html(
            parent_document_id,
            iframe_node_id,
            String::from("about:blank"),
            String::from(EMPTY_HTML_DOCUMENT),
        )
    }

    fn remove_iframe_subdocument(&mut self, parent_document_id: u64, iframe_node_id: usize) {
        crate::log_iframe_debug(format!(
            "remove_iframe_subdocument parent_doc={} node={}",
            parent_document_id, iframe_node_id
        ));
        let Some(content_document) = self.documents.get_mut(&parent_document_id) else {
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
        &self,
        parent_traversable_id: u64,
        iframe_state: &IframeState,
    ) -> Result<(), String> {
        if !iframe_state.cross_origin {
            return Ok(());
        }

        crate::log_iframe_debug(format!(
            "retire_iframe_traversable parent_traversable={} source_navigable={}",
            parent_traversable_id, iframe_state.source_navigable_id
        ));
        self.event_sender
            .send(ContentEvent::IframeTraversableRemoved(
                IframeTraversableRemoval {
                    parent_traversable_id,
                    source_navigable_id: iframe_state.source_navigable_id,
                },
            ))
            .map_err(|error| format!("failed to send iframe traversable removal: {error}"))
    }

    /// <https://html.spec.whatwg.org/#create-a-new-child-navigable>
    fn create_new_child_navigable(
        &mut self,
        parent_document_id: u64,
        iframe_node_id: usize,
    ) -> Result<u64, String> {
        if let Some(iframe_state) = self
            .documents
            .get(&parent_document_id)
            .and_then(|content_document| content_document.iframe_states.get(&iframe_node_id))
        {
            return Ok(iframe_state.source_navigable_id);
        }

        let source_navigable_id = self.allocate_iframe_navigable_id();
        let Some(parent_traversable_id) = self
            .documents
            .get(&parent_document_id)
            .map(|d| d.traversable_id)
        else {
            debug_assert!(false, "create_new_child_navigable: parent document {parent_document_id} not found");
            return Ok(source_navigable_id);
        };
        if let Some(content_document) = self.documents.get_mut(&parent_document_id) {
            content_document.iframe_states.insert(
                iframe_node_id,
                IframeState {
                    source_navigable_id,
                    current_key: String::new(),
                    cross_origin: false,
                },
            );
        }

        self.event_sender
            .send(ContentEvent::ChildNavigableCreated(ChildNavigableCreation {
                parent_traversable_id,
                source_navigable_id,
            }))
            .map_err(|error| format!("failed to send child navigable created event: {error}"))?;

        self.attach_iframe_about_blank(parent_document_id, iframe_node_id)?;
        Ok(source_navigable_id)
    }

    fn iframe_parent_for_traversable(&self, traversable_id: u64) -> Option<(u64, usize)> {
        self.documents.iter().find_map(|(parent_document_id, content_document)| {
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
        &mut self,
        parent_document_id: u64,
        iframe_node_id: usize,
    ) -> Result<(), String> {
        let Some(content_document) = self.documents.get_mut(&parent_document_id) else {
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
        crate::dom::fire_event(&mut content_document.settings, &iframe_object, "load", false)
            .map_err(|error| error.to_string())?;

        // Step 7: "Unset childDocument's iframe load in progress flag."
        // TODO: Model the document iframe load in progress flag.

        Ok(())
    }

    pub(crate) fn run_iframe_load_event_steps_for_traversable(
        &mut self,
        traversable_id: u64,
    ) -> Result<(), String> {
        let Some((parent_document_id, iframe_node_id)) =
            self.iframe_parent_for_traversable(traversable_id)
        else {
            return Ok(());
        };

        self.run_iframe_load_event_steps(parent_document_id, iframe_node_id)
    }

    /// <https://html.spec.whatwg.org/#html-element-post-connection-steps>
    fn run_iframe_post_connection_steps(
        &mut self,
        parent_document_id: u64,
        iframe_node_id: usize,
    ) -> Result<(), String> {
        // Step 1: "Create a new child navigable for insertedNode."
        self.create_new_child_navigable(parent_document_id, iframe_node_id)?;

        // Step 2: "If insertedNode has a sandbox attribute, then parse the sandboxing directive given the attribute's value and insertedNode's iframe sandboxing flag set."
        // TODO: Model iframe sandboxing flag parsing.

        // Step 3: "Process the iframe attributes for insertedNode, with initialInsertion set to true."
        self.process_iframe_attributes(parent_document_id, iframe_node_id, true)
    }

    /// <https://html.spec.whatwg.org/#html-element-post-connection-steps>
    pub(crate) fn run_iframe_post_connection_steps_for_document(
        &mut self,
        parent_document_id: u64,
    ) -> Result<(), String> {
        let iframe_node_ids = {
            let Some(content_document) = self.documents.get(&parent_document_id) else {
                return Ok(());
            };
            let document = content_document.document.borrow();
            Self::connected_iframe_node_ids(&document)
        };

        for iframe_node_id in iframe_node_ids {
            self.run_iframe_post_connection_steps(parent_document_id, iframe_node_id)?;
        }

        Ok(())
    }

    /// <https://html.spec.whatwg.org/#process-the-iframe-attributes>
    fn process_iframe_attributes(
        &mut self,
        parent_document_id: u64,
        iframe_node_id: usize,
        initial_insertion: bool,
    ) -> Result<(), String> {
        let (creation_url, parent_traversable_id, desired_key, target) = {
            let Some(content_document) = self.documents.get(&parent_document_id) else {
                return Ok(());
            };
            let creation_url = content_document.settings.creation_url.clone();
            let document = content_document.document.borrow();
            let Some((desired_key, target)) =
                Self::iframe_navigation_target(&document, &creation_url, iframe_node_id)
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

        let previous_iframe_state = self
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
            .unwrap_or_else(|| self.allocate_iframe_navigable_id());
        let cross_origin = matches!(
            &target,
            IframeNavigationTarget::Url { url } if creation_url.origin() != url.origin()
        );
        if let Some(previous_iframe_state) = previous_iframe_state.as_ref() {
            if previous_iframe_state.cross_origin && !cross_origin {
                self.retire_iframe_traversable(parent_traversable_id, previous_iframe_state)?;
            }
        }
        if initial_insertion {
            match &target {
                IframeNavigationTarget::AboutBlank => {
                    if let Some(content_document) = self.documents.get_mut(&parent_document_id) {
                        content_document.iframe_states.insert(
                            iframe_node_id,
                            IframeState {
                                source_navigable_id,
                                current_key: desired_key.clone(),
                                cross_origin: false,
                            },
                        );
                    }
                    self.run_iframe_load_event_steps(parent_document_id, iframe_node_id)?;
                    return Ok(());
                }
                IframeNavigationTarget::Url { url } if matches_about_blank(url) => {
                    if let Some(content_document) = self.documents.get_mut(&parent_document_id) {
                        content_document.iframe_states.insert(
                            iframe_node_id,
                            IframeState {
                                source_navigable_id,
                                current_key: desired_key.clone(),
                                cross_origin: false,
                            },
                        );
                    }
                    self.run_iframe_load_event_steps(parent_document_id, iframe_node_id)?;
                    return Ok(());
                }
                IframeNavigationTarget::Srcdoc { .. } | IframeNavigationTarget::Url { .. } => {}
            }
        }

        crate::log_iframe_debug(format!(
            "process_iframe_attributes parent_doc={} node={} current_key={:?} desired_key={} source_navigable={} cross_origin={} target={}",
            parent_document_id,
            iframe_node_id,
            current_key,
            desired_key,
            source_navigable_id,
            cross_origin,
            iframe_target_description(&target)
        ));
        if let Some(content_document) = self.documents.get_mut(&parent_document_id) {
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
                self.attach_iframe_about_blank(parent_document_id, iframe_node_id)?;
                self.run_iframe_load_event_steps(parent_document_id, iframe_node_id)?;
            }
            IframeNavigationTarget::Srcdoc { html } => {
                self.attach_iframe_subdocument_from_html(
                    parent_document_id,
                    iframe_node_id,
                    creation_url.to_string(),
                    html,
                )?;
                self.run_iframe_load_event_steps(parent_document_id, iframe_node_id)?;
            }
            IframeNavigationTarget::Url { url } => {
                if cross_origin {
                    self.attach_cross_origin_iframe(
                        parent_document_id,
                        iframe_node_id,
                        source_navigable_id,
                    )?;
                }
                self.request_iframe_navigation(parent_document_id, source_navigable_id, &url)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        ContentFetchResponse, ContentRuntime, deferred_script_response_is_executable,
        is_javascript_mime_essence, normalized_content_type_essence,
    };
    use url::Url;

    use super::IframeNavigationTarget;

    fn response(status: u16, content_type: &str) -> ContentFetchResponse {
        ContentFetchResponse {
            final_url: String::from("https://example.test/script.js"),
            status,
            content_type: content_type.to_string(),
            body: b"console.log('ok');".to_vec(),
        }
    }

    #[test]
    fn deferred_scripts_accept_successful_javascript_mime() {
        assert!(deferred_script_response_is_executable(&response(
            200,
            "text/javascript; charset=utf-8"
        )));
    }

    #[test]
    fn deferred_scripts_reject_non_success_status() {
        assert!(!deferred_script_response_is_executable(&response(
            404,
            "text/javascript"
        )));
    }

    #[test]
    fn deferred_scripts_reject_clearly_wrong_mime() {
        assert!(!deferred_script_response_is_executable(&response(
            200,
            "text/html"
        )));
    }

    #[test]
    fn iframe_navigation_prefers_srcdoc_over_src() {
        let base = Url::parse("https://example.test/page.html").expect("valid base URL");
        let (request_key, target) = ContentRuntime::iframe_navigation_target_for_attributes(
            &base,
            Some("https://other.test/frame"),
            Some("<p>srcdoc</p>"),
        );
        assert_eq!(request_key, "srcdoc:<p>srcdoc</p>");
        match target {
            IframeNavigationTarget::Srcdoc { html } => assert_eq!(html, "<p>srcdoc</p>"),
            _ => panic!("expected srcdoc target"),
        }
    }

    #[test]
    fn iframe_navigation_resolves_relative_src() {
        let base = Url::parse("https://example.test/page.html").expect("valid base URL");
        let (request_key, target) = ContentRuntime::iframe_navigation_target_for_attributes(
            &base,
            Some("child.html"),
            None,
        );
        assert_eq!(request_key, "src:https://example.test/child.html");
        match target {
            IframeNavigationTarget::Url { url } => {
                assert_eq!(url.as_str(), "https://example.test/child.html")
            }
            _ => panic!("expected URL target"),
        }
    }

    #[test]
    fn iframe_navigation_falls_back_to_about_blank_when_src_missing() {
        let base = Url::parse("https://example.test/page.html").expect("valid base URL");
        let (request_key, target) =
            ContentRuntime::iframe_navigation_target_for_attributes(&base, None, None);
        assert_eq!(request_key, "about:blank");
        match target {
            IframeNavigationTarget::AboutBlank => {}
            _ => panic!("expected about:blank target"),
        }
    }

    #[test]
    fn deferred_scripts_allow_missing_content_type() {
        assert!(deferred_script_response_is_executable(&response(200, "")));
    }

    #[test]
    fn content_type_essence_is_case_and_parameter_insensitive() {
        let essence = normalized_content_type_essence("Application/JavaScript; Charset=UTF-8");
        assert_eq!(essence, "application/javascript");
        assert!(is_javascript_mime_essence(&essence));
    }
}