mod environment_settings_object;
mod global_scope;
mod html_anchor_element;
mod html_dom_tree;
mod html_element;
mod html_iframe_element;
mod html_parser;
mod hyperlink_element_utils;
mod location;
pub(crate) mod safe_passing_of_structured_data;
mod window;
mod window_or_worker_global_scope;
pub(crate) mod windowproxy;

use boa_engine::{Context, object::JsObject};
use ipc_channel::ipc::IpcSender;
use ipc_messages::content::{
    DocumentId, Event as ContentEvent, NavigableId, NavigateRequest, NavigationId,
    NewChildNavigableInfo, NewTraversableInfo, UserNavigationInvolvement,
};

pub use environment_settings_object::EnvironmentSettingsObject;
pub(crate) use global_scope::TimerHandler;
pub use global_scope::{GlobalScope, GlobalScopeKind};
pub use html_anchor_element::HTMLAnchorElement;
pub(crate) use html_dom_tree::{
    run_dom_post_connection_steps_for_document, run_dom_removing_steps_for_document,
};
pub use html_element::HTMLElement;
pub(crate) use html_element::{
    inline_style_properties_for_element, resolved_style_properties_for_element,
};
pub use html_iframe_element::HTMLIFrameElement;
pub(crate) use html_iframe_element::attach_same_origin_child_document_for_traversable;
pub(crate) use html_iframe_element::run_iframe_load_event_steps_for_traversable;
pub(crate) use html_parser::PendingParserScript;
pub use html_parser::{JsHtmlParserProvider, execute_parser_scripts, parse_html_into_document};
pub(crate) use hyperlink_element_utils::HyperlinkElementUtils;
pub use location::Location;
pub(crate) use location::LocationError;
pub use window::Window;
pub(crate) use window::window_computed_style_properties_for_element;
pub(crate) use window_or_worker_global_scope::WindowOrWorkerGlobalScope;
pub(crate) use windowproxy::WindowProxy;

use blitz_dom::{BaseDocument, DocumentConfig};
use std::{cell::RefCell, rc::Rc};
use url::Url;

/// <https://html.spec.whatwg.org/#creating-a-new-browsing-context>
///
/// Content-process portion of "create a new browsing context and document".
/// Creates a new Document (type "html", content type "text/html") with the
/// corresponding Window, realm, and environment settings object.
///
/// Steps that require UA-side state (browsing context allocation, group
/// membership, agent selection, session history) are delegated to the UA
/// by the calling algorithm (e.g. `create-a-new-child-navigable`,
/// `creating-a-new-top-level-traversable`).
///
/// The caller must store the returned `EnvironmentSettingsObject` in a Rust
/// container (e.g. `ContentProcess::documents` or the shared document registry)
/// — if it is dropped the embedded `Context` is dropped and `JsObject` handles
/// become dangling pointers. `EnvironmentSettingsObject` is not `Trace`; the
/// per-Context Boa GC is self-contained and does not trace through it.
pub(crate) fn create_a_new_browsing_context_and_document(
    event_sender: &IpcSender<ContentEvent>,
    traversable_id: NavigableId,
    document_id: DocumentId,
) -> Result<
    (
        JsObject,
        EnvironmentSettingsObject,
        Rc<RefCell<BaseDocument>>,
    ),
    String,
> {
    // Step 15: Create a new Document with type "html", content type "text/html"
    let document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig {
        viewport: None,
        base_url: None,
        net_provider: None,
        shell_provider: None,
        html_parser_provider: None,
        ..DocumentConfig::default()
    })));
    // Steps 9-10, 13: Obtain agent, create realm, set up window environment
    // settings object (handled inside EnvironmentSettingsObject::new).
    let settings = EnvironmentSettingsObject::new(
        Rc::clone(&document),
        Url::parse("about:blank").map_err(|error| error.to_string())?,
        Some(event_sender.clone()),
        Some(traversable_id),
        Some(document_id),
    )?;
    // Step 22: Populate with html/head/body given document.
    parse_html_into_document(
        &mut document.borrow_mut(),
        crate::EMPTY_HTML_DOCUMENT,
    );
    // Step 10 (continued): global object is the Window.
    let global_object = settings.context.global_object();
    Ok((global_object, settings, document))
}

/// <https://html.spec.whatwg.org/#navigate>
pub(crate) fn navigate(
    event_sender: &IpcSender<ContentEvent>,
    source_navigable_id: NavigableId,
    chosen_navigable_id: Option<NavigableId>,
    destination_url: String,
    target: String,
    user_involvement: UserNavigationInvolvement,
    noopener: bool,
    referrer_policy: Option<String>,
    features_json: Option<String>,
    new_traversable_info: Option<NewTraversableInfo>,
    new_child_navigable: Option<NewChildNavigableInfo>,
) -> Result<(), String> {
    let request = NavigateRequest {
        navigation_id: Some(NavigationId::new()),
        source_navigable_id,
        chosen_navigable_id,
        destination_url,
        target,
        user_involvement,
        noopener,
        referrer_policy,
        features_json,
        new_traversable_info,
        new_child_navigable,
    };
    event_sender
        .send(ContentEvent::NavigationRequested(request))
        .map_err(|error| format!("failed to send navigation request: {error}"))
}

/// <https://html.spec.whatwg.org/#the-rules-for-choosing-a-navigable>
///
/// Result of the rules for choosing a navigable on the content side.
/// All fields are `Option` because some cases require UA-side continuation
/// (cross-process named-target lookup, new-traversable creation during
/// anchor navigation).
pub(crate) struct ChosenNavigableResult {
    /// The resolved navigable ID, if content could resolve it locally.
    pub(crate) chosen_navigable_id: Option<NavigableId>,
    /// New traversable info, if the content process created a new
    /// top-level traversable locally (window.open path).
    pub(crate) new_traversable_info: Option<NewTraversableInfo>,
    /// The Window JsObject to back the WindowProxy, for callers that
    /// need to return it to JavaScript (window.open).
    pub(crate) return_window: Option<JsObject>,
}

/// <https://html.spec.whatwg.org/#the-rules-for-choosing-a-navigable>
///
/// Content-process side of the split algorithm.  Steps 1–7 are content-local
/// (resolving `_self`, `_parent`, `_top`).  Step 7 (find-by-target-name) is
/// delegated to the user agent because the content process does not own the
/// global navigable registry.  Step 8 (new top-level traversable) is handled
/// either locally (window.open, via `GlobalScope::create_document`) or delegated to
/// the UA (anchor navigation).
///
/// Gaps: Step 2 (windowType) and Step 3 (sandboxingFlagSet) are not
/// implemented.  windowType is always "existing or none" and sandboxing
/// is not checked.
pub(crate) fn the_rules_for_choosing_a_navigable(
    source_navigable_id: NavigableId,
    parent_navigable_id: Option<NavigableId>,
    top_level_navigable_id: NavigableId,
    target_name: &str,
    noopener: bool,
    global_scope: Option<&GlobalScope>,
    context: Option<&mut Context>,
) -> ChosenNavigableResult {
    // Step 1: Let chosen be null.
    let mut chosen: Option<NavigableId> = None;

    // Note: Step 2 (Let windowType be "existing or none") and Step 3
    // (sandboxingFlagSet) are not yet implemented.  windowType is
    // always "existing or none", which is correct for the resolved
    // cases below; when creating a new traversable windowType should
    // distinguish "new and unrestricted" vs "new with no opener".

    // Step 4: If name is the empty string or an ASCII case-insensitive match for
    //         "_self", then set chosen to currentNavigable.
    if target_name.is_empty() || target_name.eq_ignore_ascii_case("_self") {
        chosen = Some(source_navigable_id);
    }

    // Step 5: Otherwise, if name is an ASCII case-insensitive match for "_parent",
    //         set chosen to currentNavigable's parent, if any, and currentNavigable
    //         otherwise.
    if chosen.is_none() && target_name.eq_ignore_ascii_case("_parent") {
        chosen = Some(parent_navigable_id.unwrap_or(source_navigable_id));
    }

    // Step 6: Otherwise, if name is an ASCII case-insensitive match for "_top", set
    //         chosen to currentNavigable's traversable navigable.
    if chosen.is_none() && target_name.eq_ignore_ascii_case("_top") {
        chosen = Some(top_level_navigable_id);
    }

    // Step 7: Otherwise, if name is not an ASCII case-insensitive match for "_blank"
    //         and noopener is false, then set chosen to the result of finding a
    //         navigable by target name given name and currentNavigable.
    if chosen.is_none() && !target_name.eq_ignore_ascii_case("_blank") && !noopener {
        // Content cannot cross-process lookup; delegate to UA.
        // TODO: implement local same-process target-name lookup against
        //       navigable registry.
    }

    // Step 8: If chosen is null, then a new top-level traversable is being requested.
    // <https://html.spec.whatwg.org/#creating-a-new-top-level-traversable>
    //
    // Spec branches within Step 8:
    //   1. Null opener (noopener=true, COOP enforcement, etc.): calls
    //      `create a new top-level traversable` with null opener, which
    //      creates a new BCG. Requires UA.
    //   2. Non-null opener (noopener=false): calls `create a new top-level
    //      traversable` with the opener BC, which creates an auxiliary BC
    //      in the same BCG. Document can be created in content.
    let Some(chosen) = chosen else {
        // ---- Null-opener branch (noopener or equivalent) ----
        // <https://html.spec.whatwg.org/#creating-a-new-top-level-browsing-context>
        if noopener {
            // Delegate to UA: creates a new top-level browsing context
            // (new BCG) and sends CreateEmptyDocument back.
            return ChosenNavigableResult {
                chosen_navigable_id: None,
                new_traversable_info: None,
                return_window: None,
            };
        }

        // ---- Opener branch (auxiliary BC) ----
        // <https://html.spec.whatwg.org/#creating-a-new-auxiliary-browsing-context>
        if let (Some(global_scope), Some(_context)) = (global_scope, context) {
            // window.open path with opener: create the about:blank document
            // locally since the new auxiliary BC reuses the opener's BCG.
            // The UA continues via `new_traversable_info` in NavigateRequest.
            let new_traversable_id = NavigableId::new();
            let new_document_id = DocumentId::new();

            let (global_object, settings, document) = match global_scope.create_document(
                new_traversable_id,
                new_document_id,
                None,
                new_traversable_id,
            ) {
                Ok(result) => result,
                Err(error) => {
                    eprintln!(
                        "the_rules_for_choosing_a_navigable: failed to create document: {error}"
                    );
                    return ChosenNavigableResult {
                        chosen_navigable_id: None,
                        new_traversable_info: None,
                        return_window: None,
                    };
                }
            };
            if let Err(error) = global_scope.register_new_traversable_document(
                new_document_id,
                settings,
                document,
            ) {
                eprintln!(
                    "the_rules_for_choosing_a_navigable: failed to register document: {error}"
                );
            }

            let new_info = NewTraversableInfo {
                document_id: new_document_id,
                target_name: target_name.to_owned(),
            };

            return ChosenNavigableResult {
                chosen_navigable_id: Some(new_traversable_id),
                new_traversable_info: Some(new_info),
                return_window: Some(global_object),
            };
        }

        // Anchor-navigation path (or missing GlobalScope): delegate to UA.
        return ChosenNavigableResult {
            chosen_navigable_id: None,
            new_traversable_info: None,
            return_window: None,
        };
    };

    // Step 9: Return chosen and windowType.
    // Note: windowType is always "existing or none" (Step 2 deferred).
    // The return_window for _self / _parent / _top is the source document's
    // global object (correct for _self; _parent and _top that target a
    // different process are a known gap — see content/src/html/README.md).
    let return_window = context.map(|ctx| ctx.global_object());
    ChosenNavigableResult {
        chosen_navigable_id: Some(chosen),
        new_traversable_info: None,
        return_window,
    }
}
