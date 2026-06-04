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
    Event as ContentEvent, NavigableId, NavigateRequest, NavigationId, UserNavigationInvolvement,
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

/// <https://html.spec.whatwg.org/#navigate>
/// Shared content-process helper to send a navigation request to the user agent.
///
/// Builds a `NavigateRequest` message and sends it via the content-to-user-agent IPC
/// channel. This is the common call point for hyperlink activation, iframe navigation,
/// and Location-object navigation.
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
    };
    event_sender
        .send(ContentEvent::NavigationRequested(request))
        .map_err(|error| format!("failed to send navigation request: {error}"))
}
