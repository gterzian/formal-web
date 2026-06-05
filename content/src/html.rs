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
    DocumentId, Event as ContentEvent, EventLoopId, NavigableId, NavigateRequest, NavigationId,
    NewTraversableInfo, UserNavigationInvolvement,
};

pub use environment_settings_object::EnvironmentSettingsObject;
pub(crate) use global_scope::{CreateDocumentCallback, TimerHandler};
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
/// Implements the full algorithm.  The function takes an optional
/// `GlobalScope` and `Context` — when present (window.open), new-traversable
/// creation happens locally via `CreateDocumentCallback`.  When absent
/// (anchor navigation), new traversables are delegated to the UA.
///
///   1. Let chosen be null.
///   2. Let currentNavigable be sourceNavigable.
///   3. If name is empty or `_self`, set chosen to currentNavigable.
///   4. If name is `_parent`, set chosen to parent (or currentNavigable).
///   5. If name is `_top`, set chosen to traversable.
///   6. Otherwise, if name is not `_blank` and noopener is false,
///      set chosen to the result of finding a navigable by target name.
///   7. If chosen is null, a new top-level traversable is being requested.
///   8. Return chosen.
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

    // Step 3: Handle empty / _self.
    if target_name.is_empty() || target_name.eq_ignore_ascii_case("_self") {
        chosen = Some(source_navigable_id);
    }

    // Step 4: Handle _parent.
    if chosen.is_none() && target_name.eq_ignore_ascii_case("_parent") {
        chosen = Some(parent_navigable_id.unwrap_or(source_navigable_id));
    }

    // Step 5: Handle _top.
    if chosen.is_none() && target_name.eq_ignore_ascii_case("_top") {
        chosen = Some(top_level_navigable_id);
    }

    // Step 6: Handle named targets (local lookup only — skip if noopener or _blank).
    if chosen.is_none() && !target_name.eq_ignore_ascii_case("_blank") && !noopener {
        // Content cannot cross-process lookup; delegate to UA.
        // TODO: implement local same-process target-name lookup against
        //       navigable registry.
    }

    // Step 7: If chosen is still null, a new top-level traversable is needed.
    //
    // <https://html.spec.whatwg.org/#creating-a-new-top-level-traversable>
    let Some(chosen) = chosen else {
        if let (Some(global_scope), Some(_context)) = (global_scope, context) {
            // window.open path: content creates the about:blank document locally
            // so the caller can return a WindowProxy immediately.  The UA
            // continues via `new_traversable_info` in the NavigateRequest.
            //
            // Steps 1–3 (browsing context, document, opener) → content side:
            //   The CreateDocumentCallback creates an about:blank document with
            //   its own Window and JS Context.  The Window will back the
            //   WindowProxy.
            // Steps 4–6 (documentState, navigable record, initialise) → UA side:
            //   The UA's `create_new_top_level_traversable_from_content` sets up
            //   navigable, BCG, agent, event-loop registration without sending
            //   CreateEmptyDocument back.
            let new_traversable_id = NavigableId::new();
            let new_document_id = DocumentId::new();

            let created_window = match global_scope.create_document(
                new_traversable_id,
                new_document_id,
                None,
                new_traversable_id,
            ) {
                Some(Ok((global_object, settings, document))) => {
                    global_scope.store_pending_window_open_document(
                        new_document_id,
                        settings,
                        document,
                    );
                    global_object
                }
                Some(Err(error)) => {
                    eprintln!(
                        "the_rules_for_choosing_a_navigable: failed to create document: {error}"
                    );
                    return ChosenNavigableResult {
                        chosen_navigable_id: None,
                        new_traversable_info: None,
                        return_window: None,
                    };
                }
                None => {
                    eprintln!(
                        "the_rules_for_choosing_a_navigable: no create-document callback"
                    );
                    return ChosenNavigableResult {
                        chosen_navigable_id: None,
                        new_traversable_info: None,
                        return_window: None,
                    };
                }
            };

            let event_loop_id = global_scope
                .event_loop_id()
                .unwrap_or_else(EventLoopId::new);
            let new_info = NewTraversableInfo {
                document_id: new_document_id,
                event_loop_id,
                target_name: target_name.to_owned(),
            };

            return ChosenNavigableResult {
                chosen_navigable_id: Some(new_traversable_id),
                new_traversable_info: Some(new_info),
                return_window: Some(created_window),
            };
        }

        // Anchor-navigation path (or missing callback): delegate to UA.
        return ChosenNavigableResult {
            chosen_navigable_id: None,
            new_traversable_info: None,
            return_window: None,
        };
    };

    // Step 13 (window.open) / follow-hyperlink: chosen is resolved.
    // Return the navigable ID. The return_window for _self / _parent / _top
    // is the source document's global object (correct for _self; _parent and
    // _top that target a different process are a known gap documented in
    // content/src/html/README.md).
    let return_window = context.map(|ctx| ctx.global_object());
    ChosenNavigableResult {
        chosen_navigable_id: Some(chosen),
        new_traversable_info: None,
        return_window,
    }
}
