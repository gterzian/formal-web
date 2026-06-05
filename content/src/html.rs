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

use ipc_channel::ipc::IpcSender;
use ipc_messages::content::{
    Event as ContentEvent, NavigableId, NavigateRequest, NavigationId, NewTraversableInfo,
    UserNavigationInvolvement,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChosenNavigable {
    /// The target navigable was resolved to a known ID.
    Resolved(NavigableId),
    /// The target requires the user agent to find or create a navigable.
    NeedsUserAgentAction,
}

/// <https://html.spec.whatwg.org/#the-rules-for-choosing-a-navigable>
///
/// Content-side subset.  The UA continues with steps 6–8 when this returns
/// `NeedsUserAgentAction`.
///
///   1. Let chosen be null.
///   2. Let currentNavigable be sourceNavigable.
///   3. If name is empty or `_self`, set chosen to currentNavigable.
///   4. If name is `_parent`, set chosen to parent (or currentNavigable).
///   5. If name is `_top`, set chosen to traversable.
///   -- content-side subset ends here; remaining steps run in the UA --
///   6. Otherwise, if name is not `_blank` and noopener is false,
///      set chosen to the result of
///      <https://html.spec.whatwg.org/#finding-a-navigable-by-target-name>.
///   7. If chosen is null, a new top-level traversable is being requested.
///   8. Return chosen.
pub(crate) fn choose_navigable(
    source_navigable_id: NavigableId,
    parent_navigable_id: Option<NavigableId>,
    top_level_navigable_id: NavigableId,
    target_name: &str,
    noopener: bool,
) -> ChosenNavigable {
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
    match chosen {
        Some(id) => ChosenNavigable::Resolved(id),
        None => ChosenNavigable::NeedsUserAgentAction,
    }
}
