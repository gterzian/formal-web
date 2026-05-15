use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::{JsData, JsNativeError, JsResult, object::JsObject};
use boa_gc::{Finalize, Trace};
use ipc_channel::ipc::IpcSender;
use ipc_messages::content::{
    Event as ContentEvent, NavigateRequest, NavigationId, UserNavigationInvolvement,
};
use url::Url;

use crate::html::{HTMLElement, HyperlinkElementUtils};

/// <https://html.spec.whatwg.org/multipage/#the-rules-for-choosing-a-navigable>
/// Note: This helper runs the content-local prefix of the algorithm so content can resolve
/// `_self`, `_parent`, and `_top` without a blocking round-trip. The user agent later continues
/// the remaining target-name and new-top-level branches from the explicit request fields.
fn choose_navigable_for_hyperlink_activation(
    source_navigable_id: u64,
    parent_navigable_id: Option<u64>,
    top_level_navigable_id: u64,
    target_name: &str,
    noopener: bool,
) -> Option<u64> {
    // Step 4: "If name is the empty string or an ASCII case-insensitive match for \"_self\", then set chosen to currentNavigable."
    let normalized_target_name = if target_name.eq_ignore_ascii_case("_self") {
        String::new()
    } else {
        target_name.to_owned()
    };

    // Step 8: "If chosen is null, then a new top-level traversable is being requested."
    // Note: Content leaves this branch unresolved so the user agent can continue the navigation
    // entrypoint asynchronously when a new top-level traversable is required.
    if noopener || normalized_target_name.eq_ignore_ascii_case("_blank") {
        return None;
    }

    // Step 4: "If name is the empty string or an ASCII case-insensitive match for \"_self\", then set chosen to currentNavigable."
    if normalized_target_name.is_empty() {
        return Some(source_navigable_id);
    }

    // Step 5: "Otherwise, if name is an ASCII case-insensitive match for \"_parent\", set chosen to currentNavigable's parent, if any, and currentNavigable otherwise."
    if normalized_target_name.eq_ignore_ascii_case("_parent") {
        return Some(parent_navigable_id.unwrap_or(source_navigable_id));
    }

    // Step 6: "Otherwise, if name is an ASCII case-insensitive match for \"_top\", set chosen to currentNavigable's traversable navigable."
    if normalized_target_name.eq_ignore_ascii_case("_top") {
        return Some(top_level_navigable_id);
    }

    // Step 7: "Otherwise, if name is not an ASCII case-insensitive match for \"_blank\" and noopener is false, then set chosen to the result of finding a navigable by target name given name and currentNavigable."
    // Note: Content does not own the full cross-process target-name registry, so unresolved names
    // continue in the user agent.
    None
}

/// <https://html.spec.whatwg.org/#htmlanchorelement>
#[derive(Trace, Finalize, JsData)]
pub struct HTMLAnchorElement {
    /// <https://html.spec.whatwg.org/#htmlelement>
    pub html_element: HTMLElement,
}

impl HTMLAnchorElement {
    pub fn new(document: Rc<RefCell<BaseDocument>>, node_id: usize) -> Self {
        Self {
            html_element: HTMLElement::new(document, node_id),
        }
    }

    fn href_attribute(&self) -> Option<String> {
        self.html_element.element.get_attribute("href")
    }

    pub(crate) fn has_download_attribute(&self) -> bool {
        self.html_element.element.has_attribute("download")
    }

    /// <https://html.spec.whatwg.org/#links-created-by-a-and-area-elements:activation-behaviour-2>
    pub(crate) fn activation_behavior(
        &self,
        source_navigable_id: u64,
        parent_navigable_id: Option<u64>,
        top_level_navigable_id: u64,
        document_creation_url: &Url,
        _event: &JsObject,
        event_sender: &IpcSender<ContentEvent>,
    ) -> JsResult<()> {
        // Step 1: "If element has no href attribute, then return."
        if self.href_attribute().is_none() {
            return Ok(());
        }

        // Step 2: "Let hyperlinkSuffix be null."
        let hyperlink_suffix: Option<&str> = None;

        // Step 3: "If element is an a element, and event's target is an img with an ismap attribute specified, then:"
        // TODO: Model `img[ismap]` click coordinates and derive `hyperlinkSuffix` from the event target.

        // Step 4: "Let userInvolvement be event's user navigation involvement."
        // Note: Blitz-driven pointer activation currently reaches this hook only for direct user click dispatch, so the runtime collapses this to `activation`.
        let user_involvement = UserNavigationInvolvement::Activation;

        // Step 5: "If the user has expressed a preference to download the hyperlink, then set userInvolvement to \"browser UI\"."
        // Note: The current runtime does not yet model a separate browser-UI download preference channel.

        // Step 6: "If element has a download attribute, or if the user has expressed a preference to download the hyperlink, then download the hyperlink created by element with hyperlinkSuffix set to hyperlinkSuffix and userInvolvement set to userInvolvement."
        // Note: Download handling is deferred; the current runtime treats download anchors as non-navigating activation targets.
        if self.has_download_attribute() {
            return Ok(());
        }

        // Step 7: "Otherwise, follow the hyperlink created by element with hyperlinkSuffix set to hyperlinkSuffix and userInvolvement set to userInvolvement."
        let Some(destination_url) = self.follow_hyperlink(document_creation_url, hyperlink_suffix)
        else {
            return Ok(());
        };

        let target = self.target();
        let noopener = self.noopener();
        let chosen_navigable_id = choose_navigable_for_hyperlink_activation(
            source_navigable_id,
            parent_navigable_id,
            top_level_navigable_id,
            &target,
            noopener,
        );

        // Note: Content sends the locally resolved target selection, when available, so the
        // user agent can continue `navigate` from the remaining target-name and top-level-creation
        // branches instead of repeating these local steps or blocking on a reply path.
        let request = NavigateRequest {
            navigation_id: Some(NavigationId::new()),
            source_navigable_id,
            chosen_navigable_id,
            destination_url,
            target,
            user_involvement,
            noopener,
        };
        event_sender
            .send(ContentEvent::NavigationRequested(request))
            .map_err(|error| {
                JsNativeError::typ()
                    .with_message(format!(
                        "failed to send hyperlink activation navigation request: {error}"
                    ))
                    .into()
            })
    }

    /// <https://html.spec.whatwg.org/#get-an-element's-noopener>
    pub(crate) fn noopener(&self) -> bool {
        // Step 1: "Let noopener be false."
        let rel_tokens = self
            .rel()
            .split_ascii_whitespace()
            .map(|token| token.to_ascii_lowercase())
            .collect::<Vec<_>>();

        // Step 2: "If element's link types include the `noopener` or `noreferrer` keyword, then set noopener to true."
        if rel_tokens
            .iter()
            .any(|token| token == "noopener" || token == "noreferrer")
        {
            return true;
        }

        // Step 3: "If element's link types do not include the `opener` keyword and element's target is `_blank`, then set noopener to true."
        let target = self.target();
        target.eq_ignore_ascii_case("_blank") && !rel_tokens.iter().any(|token| token == "opener")
    }

    /// <https://html.spec.whatwg.org/#following-hyperlinks-2>
    /// Note: This helper computes the destination URL for hyperlink following. The caller keeps
    /// the surrounding activation state, runs the content-local target-selection prefix, and then
    /// raises `NavigationRequested` so the user agent continues `navigate` and later finalization.
    pub(crate) fn follow_hyperlink(
        &self,
        document_creation_url: &Url,
        hyperlink_suffix: Option<&str>,
    ) -> Option<String> {
        // Step 1: "If subject cannot navigate, then return."
        // Note: The current content runtime does not yet model sandboxing or disconnected-navigable checks, so the missing-`href` case is the only early return handled here.
        let mut url = self.reinitialize_url(document_creation_url)?;

        // Step 6: "If hyperlinkSuffix is non-null, then append hyperlinkSuffix to url, appropriately encoded."
        if let Some(hyperlink_suffix) = hyperlink_suffix {
            let serialized = format!("{}{}", url, hyperlink_suffix);
            url = Url::parse(&serialized).ok()?;
        }

        // Step 14: "Navigate targetNavigable to url."
        // Note: `activation_behavior` continues this step by raising `NavigationRequested` with
        // the resolved URL plus explicit target-selection state.
        Some(url.to_string())
    }

    /// <https://html.spec.whatwg.org/#api-for-a-and-area-elements:dom-hyperlink-href>
    pub(crate) fn href(&self, document_creation_url: &Url) -> String {
        // Step 1: "Reinitialize url."
        let url = self.reinitialize_url(document_creation_url);

        // Step 2: "Let url be this's url."

        if url.is_none() && self.href_attribute().is_none() {
            // Step 3: "If url is null and this has no href content attribute, return the empty string."
            return String::new();
        }

        if let Some(href) = self.href_attribute().filter(|_| url.is_none()) {
            // Step 4: "Otherwise, if url is null, return this's href content attribute's value."
            return href;
        }

        // Step 5: "Return url, serialized."
        url.map(|url| url.to_string()).unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-href>
    pub(crate) fn set_href(&self, href: &str) {
        // Step 1: "Set the href content attribute to the given value."
        self.html_element.element.set_attribute("href", href);
    }

    /// <https://html.spec.whatwg.org/#dom-a-target>
    pub(crate) fn target(&self) -> String {
        // Step 1: "Return the value of the target content attribute."
        self.html_element
            .element
            .get_attribute("target")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-a-target>
    pub(crate) fn set_target(&self, target: &str) {
        // Step 1: "Set the target content attribute to the given value."
        self.html_element.element.set_attribute("target", target);
    }

    /// <https://html.spec.whatwg.org/#dom-a-download>
    pub(crate) fn download(&self) -> String {
        // Step 1: "Return the value of the download content attribute."
        self.html_element
            .element
            .get_attribute("download")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-a-download>
    pub(crate) fn set_download(&self, download: &str) {
        // Step 1: "Set the download content attribute to the given value."
        self.html_element
            .element
            .set_attribute("download", download);
    }

    /// <https://html.spec.whatwg.org/#dom-a-rel>
    pub(crate) fn rel(&self) -> String {
        // Step 1: "Return the value of the rel content attribute."
        self.html_element
            .element
            .get_attribute("rel")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-a-rel>
    pub(crate) fn set_rel(&self, rel: &str) {
        // Step 1: "Set the rel content attribute to the given value."
        self.html_element.element.set_attribute("rel", rel);
    }

    /// <https://html.spec.whatwg.org/#dom-a-referrerpolicy>
    pub(crate) fn referrer_policy(&self) -> String {
        // Step 1: "Return the value of the referrerpolicy content attribute."
        self.html_element
            .element
            .get_attribute("referrerpolicy")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-a-referrerpolicy>
    pub(crate) fn set_referrer_policy(&self, referrer_policy: &str) {
        // Step 1: "Set the referrerpolicy content attribute to the given value."
        self.html_element
            .element
            .set_attribute("referrerpolicy", referrer_policy);
    }
}

impl HyperlinkElementUtils for HTMLAnchorElement {
    /// <https://html.spec.whatwg.org/#api-for-a-and-area-elements:concept-hyperlink-url-set-2>
    fn set_the_url(&self, document_creation_url: &Url) -> Option<Url> {
        // Step 1: "Set this element's url to null."
        // Note: The current runtime does not persist the associated hyperlink URL, so this method returns the computed URL instead of storing it on the carrier.

        // Step 2: "If this element's href content attribute is absent, then return."
        let href = self.href_attribute()?;

        // Step 3: "Let url be the result of encoding-parsing a URL given this element's href content attribute's value, relative to this element's node document."
        // Note: The current runtime resolves relative URLs against the document creation URL because the document base URL is not yet exposed on the DOM carrier.
        let url = document_creation_url.join(&href).ok();

        // Step 4: "If url is not failure, then set this element's url to url."
        url
    }

    /// <https://html.spec.whatwg.org/#api-for-a-and-area-elements:update-href>
    fn update_href(&self, url: &Url) {
        // Step 1: "Set the element's href content attribute's value to the element's url, serialized."
        self.html_element
            .element
            .set_attribute("href", url.as_str());
    }
}
