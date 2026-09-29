use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use html5ever::{local_name, ns};
use ipc_messages::content::DocumentId;
use js_engine::{ExecutionContext, gc_struct};

use crate::ContentProcess;
use crate::dom::fire_event;
use crate::html::HTMLElement;
use crate::js::Types;
use crate::js::downcast::event_target_from_js_object;
use crate::js::platform_objects::{resolve_element_object, with_global_scope};

/// <https://html.spec.whatwg.org/#htmllinkelement>
#[gc_struct]
pub struct HTMLLinkElement {
    /// <https://html.spec.whatwg.org/#htmlelement>
    pub html_element: HTMLElement,
}

impl HTMLLinkElement {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            html_element: HTMLElement::new(document, node_id, ec),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-link-href>
    pub(crate) fn href(&self, ec: &mut dyn ExecutionContext<Types>) -> String {
        // The href IDL attribute must reflect the href content attribute as a
        // URL: parse the value relative to the element's node document, and
        // return the resulting URL string, or the value itself when parsing
        // fails, or the empty string when the attribute is absent.
        let Some(href) = self.html_element.element.get_attribute("href") else {
            return String::new();
        };
        let base = with_global_scope(ec, |global_scope, _ec| Ok(global_scope.creation_url()))
            .ok()
            .flatten();
        match base.and_then(|base| base.join(&href).ok()) {
            Some(url) => url.to_string(),
            None => href,
        }
    }

    /// <https://html.spec.whatwg.org/#dom-link-href>
    pub(crate) fn set_href(&self, href: &str) {
        // On setting, the content attribute must be set to the new value.
        self.html_element
            .element
            .set_an_attribute_value("href", href, None, None);
    }

    /// <https://html.spec.whatwg.org/#dom-link-rel>
    pub(crate) fn rel(&self) -> String {
        // The rel IDL attribute must reflect the rel content attribute.
        self.html_element
            .element
            .get_attribute("rel")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-link-rel>
    pub(crate) fn set_rel(&self, rel: &str) {
        self.html_element
            .element
            .set_an_attribute_value("rel", rel, None, None);
    }

    /// <https://html.spec.whatwg.org/#reflect>
    pub(crate) fn reflected(&self, name: &str) -> String {
        // The type, as, media, hreflang, integrity, referrerPolicy and
        // charset IDL attributes reflect their content attributes.
        self.html_element
            .element
            .get_attribute(name)
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#reflect>
    pub(crate) fn set_reflected(&self, name: &str, value: &str) {
        self.html_element
            .element
            .set_an_attribute_value(name, value, None, None);
    }

    /// <https://html.spec.whatwg.org/#dom-link-crossorigin>
    pub(crate) fn cross_origin(&self) -> Option<String> {
        // The crossOrigin IDL attribute must reflect the crossorigin content
        // attribute, limited to only known values.
        self.html_element
            .element
            .get_attribute("crossorigin")
            .map(|value| {
                if value.eq_ignore_ascii_case("use-credentials") {
                    String::from("use-credentials")
                } else {
                    String::from("anonymous")
                }
            })
    }

    /// <https://html.spec.whatwg.org/#dom-link-crossorigin>
    pub(crate) fn set_cross_origin(&self, value: Option<&str>) {
        match value {
            Some(value) => {
                self.html_element
                    .element
                    .set_an_attribute_value("crossorigin", value, None, None)
            }
            None => {
                self.html_element
                    .element
                    .remove_an_attribute_by_name("crossorigin");
            }
        }
    }

    /// <https://html.spec.whatwg.org/#dom-link-disabled>
    pub(crate) fn disabled(&self) -> bool {
        // The disabled IDL attribute must reflect the disabled content
        // attribute.
        self.html_element.element.has_attribute("disabled")
    }

    /// <https://html.spec.whatwg.org/#dom-link-disabled>
    pub(crate) fn set_disabled(&self, disabled: bool) {
        if disabled {
            self.html_element
                .element
                .set_an_attribute_value("disabled", "", None, None);
        } else {
            self.html_element
                .element
                .remove_an_attribute_by_name("disabled");
        }
    }
}

/// <https://html.spec.whatwg.org/#link-type-stylesheet:process-the-linked-resource>
pub(crate) fn linked_stylesheet_fetched(
    process: &mut ContentProcess,
    document_id: DocumentId,
    url: &str,
    success: bool,
) -> Result<(), String> {
    // Step 1: If the resource's Content-Type metadata is not text/css, then set
    // success to false.
    // Step 2: If el no longer creates an external resource link that
    // contributes to the styling processing model, or if, since the resource
    // in question was fetched, it has become appropriate to fetch it again,
    // then return.
    // Step 3: If el has an associated CSS style sheet, remove the CSS style
    // sheet.
    // Step 4: If success is true, then: create a CSS style sheet ...; fire an
    // event named load at el.
    // Step 5: Otherwise, fire an event named error at el.
    // Note: The style sheet itself is created and applied by the document
    // engine (blitz) as the resource arrives; this runs the load or error
    // event for every stylesheet link whose href resolves to the fetched URL.
    let Some(content_document) = process.documents.get(&document_id) else {
        return Ok(());
    };
    let base = content_document.settings.creation_url.clone();
    let node_ids: Vec<usize> = {
        let document = content_document.document.borrow();
        let mut node_ids = Vec::new();
        document.visit(|node_id, node| {
            let Some(element) = node.element_data() else {
                return;
            };
            if element.name.ns != ns!(html) || element.name.local != local_name!("link") {
                return;
            }
            let rel_is_stylesheet = element
                .attr(local_name!("rel"))
                .is_some_and(|rel| rel.split_ascii_whitespace().any(|rel| rel == "stylesheet"));
            let href_matches = element
                .attr(local_name!("href"))
                .and_then(|href| base.join(href).ok())
                .is_some_and(|resolved| resolved.as_str() == url);
            if rel_is_stylesheet && href_matches {
                node_ids.push(node_id);
            }
        });
        node_ids
    };
    let event_type = if success { "load" } else { "error" };
    for node_id in node_ids {
        let Some(content_document) = process.documents.get_mut(&document_id) else {
            return Ok(());
        };
        let time_millis = content_document.settings.current_time_millis();
        with_global_scope(content_document.settings.ec(), |_global_scope, ec| {
            let object = resolve_element_object(node_id, ec)?;
            let Some(target) = event_target_from_js_object(ec, &object) else {
                return Ok(());
            };
            fire_event(ec, &target, event_type, time_millis, false).map(|_| ())
        })
        .map_err(|error| {
            format!(
                "failed to fire {event_type} at the link element: {}",
                error.display()
            )
        })?;
    }
    Ok(())
}
