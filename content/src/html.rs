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
    NewTraversableInfo, UserNavigationInvolvement,
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
    let Some(chosen) = chosen else {
        if let (Some(global_scope), Some(_context)) = (global_scope, context) {
            // window.open path: the content process creates the about:blank
            // document locally so the caller can return a WindowProxy
            // immediately.  The UA continues via `new_traversable_info` in the
            // NavigateRequest.
            //
            // `GlobalScope::create_document` creates an about:blank document
            // with its own Window and JS Context directly (no callback
            // indirection).  The UA's `create_new_top_level_traversable_from_content`
            // sets up navigable, BCG, agent, and event-loop registration without
            // sending CreateEmptyDocument back.
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
            global_scope.store_pending_window_open_document(
                new_document_id,
                settings,
                document,
            );

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

        // Anchor-navigation path (or missing callback): chosen stays null,
        // delegate to UA to create the new traversable.
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
