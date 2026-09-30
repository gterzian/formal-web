use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use js_engine::{Completion, ExecutionContext, gc_struct};
use url::Url;

use blitz_traits::net::Request;
use html5ever::{local_name, ns};
use ipc_messages::content::DocumentId;

use crate::dom::{Document, fire_event};
use crate::html::{EnvironmentSettingsObject, HTMLElement};
use crate::js::Types;
use crate::js::downcast::event_target_from_js_object;
use crate::js::platform_objects::{resolve_element_object, with_global_scope};
use crate::{ContentProcess, PendingNetworkHandler};

/// <https://html.spec.whatwg.org/#htmlscriptelement>
#[gc_struct]
pub struct HTMLScriptElement {
    /// <https://html.spec.whatwg.org/#htmlelement>
    pub html_element: HTMLElement,
}

impl HTMLScriptElement {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            html_element: HTMLElement::new(document, node_id, ec),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-script-src>
    pub(crate) fn src(&self, document_url: &Url) -> String {
        // The src IDL attribute must reflect the src content attribute (a
        // USVString attribute whose content attribute contains a URL):
        // Step 1: Let element be the result of running this's get the
        // element.
        // Step 2: Let contentAttributeValue be the result of running this's
        // get the content attribute.
        let Some(content_attribute_value) = self.html_element.element.get_attribute("src") else {
            // Step 3: If contentAttributeValue is null, then return the empty
            // string.
            return String::new();
        };

        // Step 4: Let urlString be the result of encoding-parsing-and-
        // serializing a URL given contentAttributeValue, relative to
        // element's node document.
        // Step 5: If urlString is not failure, then return urlString.
        // Step 6: Return contentAttributeValue.
        match document_url.join(&content_attribute_value) {
            Ok(url) => url.to_string(),
            Err(_) => content_attribute_value,
        }
    }

    /// <https://html.spec.whatwg.org/#dom-script-src>
    pub(crate) fn set_src(&self, value: &str) {
        // On setting, set the content attribute to the given value.
        self.html_element
            .element
            .set_an_attribute_value("src", value, None, None);
    }

    /// <https://html.spec.whatwg.org/#dom-script-type>
    pub(crate) fn type_(&self) -> String {
        // The type IDL attribute must reflect the type content attribute.
        self.html_element
            .element
            .get_attribute("type")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-script-type>
    pub(crate) fn set_type(&self, value: &str) {
        self.html_element
            .element
            .set_an_attribute_value("type", value, None, None);
    }

    /// <https://html.spec.whatwg.org/#dom-script-async>
    pub(crate) fn async_(&self) -> bool {
        // Step 1: If this's force async is true, then return true.
        // Step 2: If this's async content attribute is present, then return
        // true.
        // Step 3: Return false.
        // Note: The force async flag is not modeled; it is false once the
        // element is parser-inserted.
        self.html_element.element.has_attribute("async")
    }

    /// <https://html.spec.whatwg.org/#dom-script-async>
    pub(crate) fn set_async(&self, value: bool) {
        // Step 1: Set this's force async to false.
        // Step 2: If the given value is true, then set this's async content
        // attribute to the empty string.
        // Step 3: Otherwise, remove this's async content attribute.
        self.set_boolean_attribute("async", value);
    }

    /// <https://html.spec.whatwg.org/#dom-script-defer>
    pub(crate) fn defer(&self) -> bool {
        // The defer IDL attribute must reflect the defer content attribute.
        self.html_element.element.has_attribute("defer")
    }

    /// <https://html.spec.whatwg.org/#dom-script-defer>
    pub(crate) fn set_defer(&self, value: bool) {
        self.set_boolean_attribute("defer", value);
    }

    /// <https://html.spec.whatwg.org/#dom-script-nomodule>
    pub(crate) fn no_module(&self) -> bool {
        // The noModule IDL attribute must reflect the nomodule content
        // attribute.
        self.html_element.element.has_attribute("nomodule")
    }

    /// <https://html.spec.whatwg.org/#dom-script-nomodule>
    pub(crate) fn set_no_module(&self, value: bool) {
        self.set_boolean_attribute("nomodule", value);
    }

    /// <https://html.spec.whatwg.org/#dom-script-text>
    pub(crate) fn text(&self) -> String {
        // The text getter steps are to return this's child text content.
        self.html_element
            .element
            .node
            .text_content()
            .unwrap_or_default()
    }

    /// <https://webidl.spec.whatwg.org/#idl-boolean>
    fn set_boolean_attribute(&self, name: &str, value: bool) {
        // A boolean IDL attribute reflecting a boolean content attribute: on
        // setting, the content attribute is set to the empty string when the
        // value is true and removed otherwise.
        if value {
            self.html_element
                .element
                .set_an_attribute_value(name, "", None, None);
        } else {
            self.html_element.element.remove_an_attribute_by_name(name);
        }
    }
}

/// <https://dom.spec.whatwg.org/#concept-node-document>
fn node_document(ec: &mut dyn ExecutionContext<Types>) -> Completion<Option<Document>, Types> {
    with_global_scope(ec, |global_scope, ec| {
        let Some(document_object) = global_scope.document_object(ec) else {
            return Ok(None);
        };
        Ok(ec
            .with_object_any(&document_object)
            .and_then(|data| data.downcast_ref::<Document>().cloned()))
    })
}

/// <https://dom.spec.whatwg.org/#concept-node-document>
fn node_document_of_settings(settings: &mut EnvironmentSettingsObject) -> Result<Document, String> {
    let document = node_document(settings.ec()).map_err(|error| {
        format!(
            "failed to resolve the script's node document: {}",
            error.display()
        )
    })?;
    document.ok_or_else(|| String::from("the realm has no document for the script element"))
}

/// Mark the scripts the parser ran as already started (prepare the script
/// element step 5 ran in the parser path).
pub(crate) fn mark_parser_scripts_started(
    settings: &mut EnvironmentSettingsObject,
    node_ids: &[usize],
) -> Result<(), String> {
    let document = node_document_of_settings(settings)?;
    for node_id in node_ids {
        document.set_script_already_started(*node_id);
    }
    Ok(())
}

/// <https://html.spec.whatwg.org/#execute-the-script-element>
pub(crate) fn execute_the_script_element(
    settings: &mut EnvironmentSettingsObject,
    node_id: usize,
    source: &str,
) -> Result<(), String> {
    // Step 1: Let document be el's node document.
    let document = node_document_of_settings(settings)?;

    // Step 2: If el's preparation-time document is not equal to document,
    // then return.
    // Step 3: Unblock rendering on el.
    // Step 4: If el's result is null, then fire an event named error at el,
    // and return.
    // Step 5: If el's from an external file is true, or el's type is
    // "module", then increment document's ignore-destructive-writes counter.
    // Note: The preparation-time document, render blocking, the error event
    // and the ignore-destructive-writes counter are not modeled; the caller
    // has the script's result as `source`.
    // Step 6: Switch on el's type:
    // "classic":
    // Step 6.1: Let oldCurrentScript be the value to which document's
    // currentScript object was most recently set.
    let old_current_script = document.current_script();

    // Step 6.2: If el's root is not a shadow root, then set document's
    // currentScript attribute to el. Otherwise, set it to null.
    // Note: Shadow roots are not modeled: the root is never a shadow root.
    document.set_current_script(Some(node_id));

    // Step 6.3: Run the classic script given by el's result.
    let result = settings.evaluate_script(source);

    // Step 6.4: Set document's currentScript attribute to oldCurrentScript.
    document.set_current_script(old_current_script);

    // "module": not implemented; module scripts are not prepared.
    // Step 7: Decrement the ignore-destructive-writes counter of document, if
    // it was incremented in the earlier step.
    // Step 8: If el's from an external file is true, then fire an event named
    // load at el.
    // Note: Not implemented.
    result
}

/// <https://html.spec.whatwg.org/#script-processing-model>
/// The insertion steps of script elements that became connected since the
/// last run: each one is prepared once.
pub(crate) fn run_script_post_connection_steps_for_document(
    process: &mut ContentProcess,
    document_id: DocumentId,
) -> Result<(), String> {
    // A prepared script may insert further script elements; those are
    // prepared in the same run, until a pass finds none.
    for _ in 0..16 {
        let script_node_ids = {
            let Some(content_document) = process.documents.get_mut(&document_id) else {
                return Ok(());
            };
            let node_document = node_document_of_settings(&mut content_document.settings)?;
            let document = content_document.document.borrow();
            connected_script_node_ids(&document, 0)
                .into_iter()
                .filter(|node_id| !node_document.script_already_started(*node_id))
                .collect::<Vec<_>>()
        };
        if script_node_ids.is_empty() {
            return Ok(());
        }
        for node_id in script_node_ids {
            prepare_the_script_element(process, document_id, node_id)?;
        }
    }
    Ok(())
}

/// The connected script elements in the subtree rooted at `root_node_id`, in
/// tree order.
fn connected_script_node_ids(document: &BaseDocument, root_node_id: usize) -> Vec<usize> {
    let mut script_node_ids = Vec::new();
    let mut pending = vec![root_node_id];
    while let Some(node_id) = pending.pop() {
        let Some(node) = document.get_node(node_id) else {
            continue;
        };
        pending.extend(node.children.iter().rev().copied());
        let Some(element) = node.element_data() else {
            continue;
        };
        if element.name.ns != ns!(html)
            || element.name.local != local_name!("script")
            || !node.flags.is_in_document()
        {
            continue;
        }
        script_node_ids.push(node_id);
    }
    script_node_ids
}

/// <https://html.spec.whatwg.org/#the-script-element:html-element-post-connection-steps>
pub(crate) fn script_html_element_post_connection_steps(
    inserted_node_id: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: If insertedNode is not connected, then return.
    // Step 2: If insertedNode's parser document is not null, then return.
    // Step 3: Prepare the script element given insertedNode.
    // Note: The steps run for every script element in the inserted subtree
    // that is connected and not already started.  An inline classic script
    // runs here, in the inserting task; a script with a src attribute is
    // prepared in `run_script_post_connection_steps_for_document` once the
    // task completes, because its fetch goes through the content process.
    let Some(document) = node_document(ec)? else {
        return Ok(());
    };
    let scripts = with_global_scope(ec, |global_scope, _ec| {
        let base_document = global_scope.document();
        let base_document = base_document.borrow();
        Ok(connected_script_node_ids(&base_document, inserted_node_id)
            .into_iter()
            .filter_map(|node_id| {
                let node = base_document.get_node(node_id)?;
                let element = node.element_data()?;
                if element.attr(local_name!("src")).is_some() {
                    return None;
                }
                Some((
                    node_id,
                    node.text_content(),
                    element.attr(local_name!("type")).map(str::to_owned),
                    element.attr(local_name!("language")).map(str::to_owned),
                    element.attr(local_name!("nomodule")).is_some(),
                ))
            })
            .collect::<Vec<_>>())
    })?;
    for (node_id, source_text, type_attribute, language_attribute, no_module) in scripts {
        // Prepare the script element step 1: If el's already started is
        // true, then return.
        if document.script_already_started(node_id) {
            continue;
        }

        // Prepare the script element step 6: If el has no src attribute, and
        // source text is the empty string, then return.
        if source_text.is_empty() {
            continue;
        }

        // Prepare the script element steps 8 to 12: the script block's type.
        if !is_javascript_mime_type_essence_match(&script_block_type_string(
            type_attribute.as_deref(),
            language_attribute.as_deref(),
        )) {
            continue;
        }

        // Prepare the script element step 14: Set el's already started to
        // true.
        document.set_script_already_started(node_id);

        // Prepare the script element step 18: If el has a nomodule content
        // attribute and its type is "classic", then return.
        if no_module {
            continue;
        }

        // Prepare the script element step 34: Immediately execute the script
        // element el, even if other scripts are already executing.
        // Execute the script element step 6.1 to 6.4: set currentScript, run
        // the classic script, restore currentScript.
        let old_current_script = document.current_script();
        document.set_current_script(Some(node_id));
        if let Err(error) = ec.evaluate_script(&source_text) {
            log::error!(
                "inline script inserted by script threw: {}",
                error.display()
            );
        }
        document.set_current_script(old_current_script);
    }
    Ok(())
}

/// The script block's type string (prepare the script element step 8).
fn script_block_type_string(
    type_attribute: Option<&str>,
    language_attribute: Option<&str>,
) -> String {
    match (type_attribute, language_attribute) {
        (Some(""), _) => String::from("text/javascript"),
        (None, Some("")) => String::from("text/javascript"),
        (None, None) => String::from("text/javascript"),
        (Some(type_attribute), _) => type_attribute.trim_matches(ASCII_WHITESPACE).to_owned(),
        (None, Some(language)) => format!("text/{language}"),
    }
}

/// <https://html.spec.whatwg.org/#prepare-the-script-element>
fn prepare_the_script_element(
    process: &mut ContentProcess,
    document_id: DocumentId,
    node_id: usize,
) -> Result<(), String> {
    let Some(content_document) = process.documents.get_mut(&document_id) else {
        return Ok(());
    };

    // Step 1: If el's already started is true, then return.
    let node_document = node_document_of_settings(&mut content_document.settings)?;
    if node_document.script_already_started(node_id) {
        return Ok(());
    }

    // Step 2: Let parser document be el's parser document.
    // Step 3: Set el's parser document to null.
    // Step 4: If parser document is non-null and el does not have an async
    // attribute, then set el's force async to true.
    // Note: Parser-inserted scripts are prepared by the parser path
    // (`execute_parser_scripts`); every script prepared here has a null
    // parser document.
    let (source_text, src, type_attribute, language_attribute, no_module) = {
        let document = content_document.document.borrow();
        let Some(node) = document.get_node(node_id) else {
            return Ok(());
        };
        let Some(element) = node.element_data() else {
            return Ok(());
        };
        (
            node.text_content(),
            element.attr(local_name!("src")).map(str::to_owned),
            element.attr(local_name!("type")).map(str::to_owned),
            element.attr(local_name!("language")).map(str::to_owned),
            element.attr(local_name!("nomodule")).is_some(),
        )
    };

    // Step 5: Let source text be el's child text content.
    // Step 6: If el has no src attribute, and source text is the empty string,
    // then return.
    if src.is_none() && source_text.is_empty() {
        return Ok(());
    }

    // Step 7: If el is not connected, then return.
    // Note: The caller collected connected elements only.
    // Step 8: If any of the following are true: el has a type attribute whose
    // value is the empty string; el has no type attribute but it has a
    // language attribute and that attribute's value is the empty string; or
    // el has neither a type attribute nor a language attribute, then let the
    // script block's type string for this script element be
    // "text/javascript". Otherwise, if el has a type attribute, then let the
    // script block's type string be the value of that attribute with leading
    // and trailing ASCII whitespace stripped. Otherwise, el has a non-empty
    // language attribute; let the script block's type string be the
    // concatenation of "text/" and the value of el's language attribute.
    let type_string =
        script_block_type_string(type_attribute.as_deref(), language_attribute.as_deref());

    // Step 9: If the script block's type string is a JavaScript MIME type
    // essence match, then set el's type to "classic".
    // Step 10: Otherwise, if the script block's type string is an ASCII
    // case-insensitive match for the string "module", then set el's type to
    // "module".
    // Step 11: Otherwise, if the script block's type string is an ASCII
    // case-insensitive match for the string "importmap", then set el's type
    // to "importmap".
    // Step 12: Otherwise, return. (No script is executed, and el's type is
    // left as null.)
    // Note: Only classic scripts run; module scripts and import maps are
    // left unprepared.
    if !is_javascript_mime_type_essence_match(&type_string) {
        return Ok(());
    }

    // Step 13: If parser document is non-null, then set el's parser document
    // back to parser document and set el's force async to false.
    // Step 14: Set el's already started to true.
    node_document.set_script_already_started(node_id);

    // Step 15: Set el's preparation-time document to its node document.
    // Step 16: If parser document is non-null, and parser document is not
    // equal to el's preparation-time document, then return.
    // Step 17: If scripting is disabled for el, then return.
    // Step 18: If el has a nomodule content attribute and its type is
    // "classic", then return.
    if no_module {
        return Ok(());
    }

    // Step 19: If el does not have a src content attribute, and the Should
    // element's inline behavior be blocked by Content Security Policy?
    // algorithm returns "Blocked" when given el, "script", and source text,
    // then return.
    // Step 20: If el has an event attribute and a for attribute, and el's
    // type is "classic", then: ...
    // Steps 21 to 30: the encoding, CORS setting, credentials mode, nonce,
    // integrity, referrer policy, fetch priority, parser metadata and fetch
    // options.
    // Note: Content Security Policy, the event/for attributes and the fetch
    // options are not modeled.
    // Step 31: If el has a src content attribute, then:
    if let Some(src) = src {
        // Step 31.1: If el's type is "importmap", then queue an element task
        // on the DOM manipulation task source given el to fire an event named
        // error at el, and return.
        // Step 31.2: Let src be the value of el's src attribute.
        // Step 31.3: If src is the empty string, then queue an element task
        // on the DOM manipulation task source given el to fire an event named
        // error at el, and return.
        if src.is_empty() {
            return fire_event_at_script_element(process, document_id, node_id, "error");
        }

        // Step 31.4: Set el's from an external file to true.
        // Step 31.5: Let url be the result of encoding-parsing a URL given
        // src, relative to el's node document.
        let url = content_document.settings.creation_url.join(&src);

        // Step 31.6: If url is failure, then queue an element task on the DOM
        // manipulation task source given el to fire an event named error at
        // el, and return.
        let Ok(url) = url else {
            return fire_event_at_script_element(process, document_id, node_id, "error");
        };

        // Step 31.7: If el is potentially render-blocking, then block
        // rendering on el.
        // Step 31.8: Set el's delaying the load event to true.
        // Step 31.9: If el is currently render-blocking, then set options's
        // render-blocking to true.
        // Step 31.10: Let onComplete given result be the following steps:
        // Step 31.10.1: Mark as ready el given result.
        // Step 31.11: Switch on el's type: "classic": Fetch a classic script
        // given url, settings object, options, classic script CORS setting,
        // encoding, and onComplete.
        // Note: The fetch goes to the net process; its response completes
        // the preparation in `ContentProcess::complete_document_fetch`
        // (steps 33 to 34: the script runs as soon as its result is ready).
        let navigable_id = content_document.traversable_id;
        let handler_id = process.register_pending_handler(PendingNetworkHandler::Script {
            document_id,
            node_id,
        })?;
        return process.request_remote_fetch(handler_id, navigable_id, Request::get(url));
    }

    // Step 32: If el does not have a src content attribute: "classic":
    // Step 32.1: Let script be the result of creating a classic script using
    // source text, settings object, base URL, and options.
    // Step 32.2: Mark as ready el given script.
    // Step 33: If el's type is "classic" and el has a src attribute, or el's
    // type is "module": (not this branch)
    // Step 34: Otherwise: Immediately execute the script element el, even if
    // other scripts are already executing.
    execute_the_script_element(&mut content_document.settings, node_id, &source_text)
}

/// <https://html.spec.whatwg.org/#mark-as-ready>
/// The fetched classic script of a script element prepared with a src
/// attribute: the element's steps to run when the result is ready.
pub(crate) fn script_element_fetch_completed(
    process: &mut ContentProcess,
    document_id: DocumentId,
    node_id: usize,
    source: Option<String>,
) -> Result<(), String> {
    // Step 1: Set el's result to result.
    // Step 2: If el's steps to run when the result is ready are not null,
    // then run them.
    // Step 3: Set el's steps to run when the result is ready to null.
    // Step 4: Set el's delaying the load event to false.
    // Note: The steps to run when the result is ready (prepare the script
    // element step 33.3) execute the script element; a null result (a failed
    // fetch) fires error at the element through the execute steps' step 4.
    match source {
        Some(source) => {
            let Some(content_document) = process.documents.get_mut(&document_id) else {
                return Ok(());
            };
            execute_the_script_element(&mut content_document.settings, node_id, &source)?;
            // Execute the script element step 8: If el's from an external
            // file is true, then fire an event named load at el.
            fire_event_at_script_element(process, document_id, node_id, "load")
        }
        None => fire_event_at_script_element(process, document_id, node_id, "error"),
    }
}

/// <https://dom.spec.whatwg.org/#concept-event-fire>
fn fire_event_at_script_element(
    process: &mut ContentProcess,
    document_id: DocumentId,
    node_id: usize,
    event_type: &str,
) -> Result<(), String> {
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
            "failed to fire {event_type} at the script element: {}",
            error.display()
        )
    })
}

/// <https://mimesniff.spec.whatwg.org/#javascript-mime-type-essence-match>
fn is_javascript_mime_type_essence_match(string: &str) -> bool {
    // A string is a JavaScript MIME type essence match if it is an ASCII
    // case-insensitive match for one of the JavaScript MIME type essence
    // strings.
    const JAVASCRIPT_MIME_TYPE_ESSENCES: [&str; 16] = [
        "application/ecmascript",
        "application/javascript",
        "application/x-ecmascript",
        "application/x-javascript",
        "text/ecmascript",
        "text/javascript",
        "text/javascript1.0",
        "text/javascript1.1",
        "text/javascript1.2",
        "text/javascript1.3",
        "text/javascript1.4",
        "text/javascript1.5",
        "text/jscript",
        "text/livescript",
        "text/x-ecmascript",
        "text/x-javascript",
    ];
    JAVASCRIPT_MIME_TYPE_ESSENCES
        .iter()
        .any(|essence| essence.eq_ignore_ascii_case(string))
}

/// <https://infra.spec.whatwg.org/#ascii-whitespace>
const ASCII_WHITESPACE: [char; 5] = ['\t', '\n', '\u{C}', '\r', ' '];
