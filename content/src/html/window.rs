use log::error;
use std::collections::{BTreeMap, HashMap};
use std::mem;

use ipc::IpcSender;
use ipc_messages::content::{Event as ContentEvent, UserNavigationInvolvement};

use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;

use crate::dom::Element;
use crate::dom::event::EventTarget;
use crate::webidl::Callback;

use super::resolved_style_properties_for_element;
use super::windowproxy::create_window_proxy;
use super::{GlobalScope, the_rules_for_choosing_a_navigable, the_rules_with_parent};
use js_engine::gc_struct;

/// <https://html.spec.whatwg.org/#window>
#[gc_struct]
pub struct Window {
    /// <https://dom.spec.whatwg.org/#interface-eventtarget>
    pub event_target: EventTarget,

    /// <https://html.spec.whatwg.org/#global-object>
    pub global_scope: GlobalScope,

    /// <https://html.spec.whatwg.org/#handler-onload>
    onload: Option<Callback>,
}

impl Window {
    pub(crate) fn new(global_scope: GlobalScope) -> Self {
        Self {
            event_target: EventTarget::default(),
            global_scope,
            onload: None,
        }
    }

    /// <https://html.spec.whatwg.org/#handler-onload>
    pub(crate) fn onload_value(&self) -> Option<Callback> {
        self.onload.clone()
    }

    /// <https://html.spec.whatwg.org/#handler-onload>
    pub(crate) fn replace_onload(&mut self, callback: Option<Callback>) -> Option<Callback> {
        mem::replace(&mut self.onload, callback)
    }

    /// <https://html.spec.whatwg.org/#dom-open>
    pub(crate) fn open(
        &self,
        url: &str,
        target: &str,
        features: &str,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        let Some(event_sender) = self.global_scope.event_sender() else {
            return Ok(ec.value_null());
        };
        window_open_steps(ec, url, target, features, &self.global_scope, &event_sender)
    }
}

/// <https://drafts.csswg.org/cssom/#dom-window-getcomputedstyle>
pub(crate) fn window_computed_style_properties_for_element(
    elt: &Element,
    pseudo_elt: Option<&str>,
) -> BTreeMap<String, String> {
    // Step 1: "Let doc be elt's node document."
    // Note: The style resolution helper reads elt's node document through the [Document](https://dom.spec.whatwg.org/#interface-document) [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object).

    // Step 2: "Let obj be elt."
    let mut obj = Some(elt);

    // Step 3: "If pseudoElt is provided, is not the empty string, and starts with a colon..."
    if let Some(pseudo_elt) = pseudo_elt.map(str::trim).filter(|value| !value.is_empty()) {
        if pseudo_elt.starts_with(':') {
            // Step 3.1: Parse pseudoElt as a <pseudo-element-selector>.
            // Step 3.2 / 3.3: Map invalid, ::slotted(), ::part(), or supported pseudo-element
            // requests to the corresponding pseudo-element object.
            //
            // Note: The implementation does not yet expose pseudo-element platform objects, so any
            // pseudo-element request leaves `obj` null and therefore produces an empty declaration
            // list below.
            obj = None;
        }
    }

    // Step 4: "Let decls be an empty list of CSS declarations."
    let mut decls = BTreeMap::new();

    // Step 5: "If obj is not null, and elt is connected, part of the flat tree, and its
    // shadow-including root has a browsing context ... being rendered, set decls ..."
    //
    // Note: The implementation represents the connected predicate, but it does not yet model flat
    // tree membership, pseudo-elements, or the browsing-context-container rendering gate. The
    // populated branch therefore uses the connected element that exists today.
    if let Some(obj) = obj.filter(|element| element.is_connected()) {
        decls = resolved_style_properties_for_element(obj);
    }

    // Step 6: "Return a live CSSStyleProperties object ... declarations decls ... owner node obj."
    // Note: The binding layer currently wraps this declaration snapshot in a plain JS object while
    // native CSSStyleProperties liveness is still pending.
    decls
}

// ──────────────────────────────────────────────────────────────────────────────
// Window open steps
// https://html.spec.whatwg.org/#window-open-steps
// ──────────────────────────────────────────────────────────────────────────────

/// <https://html.spec.whatwg.org/#window-open-steps>
pub(crate) fn window_open_steps(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    url: &str,
    target: &str,
    features: &str,
    global_scope: &GlobalScope,
    event_sender: &IpcSender<ContentEvent>,
) -> Completion<JsValue, crate::js::Types> {
    // Step 1: "If the event loop's termination nesting level is nonzero, then return null."
    // TODO: Content process does not yet track termination nesting.

    // Step 2: "Let sourceDocument be the entry global object's associated Document."
    let source_navigable_id = match global_scope.source_navigable_id() {
        Some(id) => id,
        None => {
            return Err(ec.new_type_error("window.open: no source navigable"));
        }
    };

    // Step 3: "Let urlRecord be null."
    // Step 4: "If url is not the empty string:"
    // Step 3: "Let urlRecord be null."
    // Step 4: "If url is not the empty string:"
    let url_record = if url.is_empty() {
        None
    } else {
        // Resolve relative URLs against the document's creation URL before
        // parsing, so that window.open("page.html") works from a file:// or
        // http:// origin.
        let resolved = match url::Url::parse(url) {
            Ok(absolute) => absolute,
            Err(_) => {
                // Try resolving as a relative URL.
                match global_scope.creation_url() {
                    Some(base_url) => match base_url.join(url) {
                        Ok(resolved) => resolved,
                        Err(_) => {
                            return Err(ec.new_type_error(
                                "SyntaxError: failed to parse URL in window.open",
                            ));
                        }
                    },
                    None => {
                        return Err(
                            ec.new_type_error("SyntaxError: failed to parse URL in window.open")
                        );
                    }
                }
            }
        };
        // <https://html.spec.whatwg.org/#cannot-navigate>
        // If the destination URL has a different origin from the source
        // document, the source cannot navigate the target.
        //
        // Note 1: `url::Url::origin()` creates a fresh opaque origin for
        // `about:` scheme URLs (about:blank inherits its creator's origin
        // per the HTML spec), so we skip the check for about:blank destinations.
        //
        // Note 2: The full sandboxing portion of "cannot navigate" is
        // deferred.  For now we block any cross-origin navigation through
        // window.open — same-origin navigations work, cross-origin throws.
        //
        // `file:` URLs produce opaque origins (unique per URL); skip the
        // check for them since all local files are treated as same-origin
        // for navigation purposes.
        if resolved.scheme() != "about" && resolved.scheme() != "file" {
            if let Some(creation_url) = global_scope.creation_url() {
                if creation_url.origin() != resolved.origin() {
                    let msg = format!(
                        "SecurityError: cross-origin navigation to {} is blocked",
                        resolved,
                    );
                    return Err(ec.new_type_error(&msg));
                }
            }
        }
        Some(resolved.to_string())
    };

    // Step 5: "If target is the empty string, then set target to '_blank'."
    let target = if target.is_empty() { "_blank" } else { target };

    // Step 6: "Let tokenizedFeatures be the result of tokenizing features."
    let tokenized_features = tokenize_features(features);

    // Step 7: "Let noreferrer be false."
    // Step 8: "If tokenizedFeatures['noreferrer'] exists..."
    let noreferrer = tokenized_features
        .get("noreferrer")
        .map(|value| parse_boolean_feature(value))
        .unwrap_or(false);

    // Step 9: "Let noopener be the result of getting noopener for window open..."
    let noopener = get_noopener_for_window_open(&tokenized_features, url_record.as_deref());

    // Step 10: "Remove tokenizedFeatures['noopener'] and tokenizedFeatures['noreferrer']."
    let mut remaining_features = tokenized_features;
    remaining_features.remove("noopener");
    remaining_features.remove("noreferrer");

    // Step 11: "Let referrerPolicy be the empty string."
    // Step 12: "If noreferrer is true, then set noopener to true and set
    //           referrerPolicy to 'no-referrer'."
    let (noopener, referrer_policy) = if noreferrer {
        (true, String::from("no-referrer"))
    } else {
        (noopener, String::new())
    };

    // Serialize remaining features for the user agent (IPC boundary).
    let features_json =
        serde_json::to_string(&remaining_features).unwrap_or_else(|_| String::from("{}"));

    // Step 13: "Apply the rules for choosing a navigable given name, window's
    //          navigable, and noopener."
    // <https://html.spec.whatwg.org/#the-rules-for-choosing-a-navigable>
    let parent_traversable_id = global_scope.parent_traversable_id();
    let top_level_traversable_id = global_scope
        .top_level_traversable_id()
        .unwrap_or(source_navigable_id);

    // The parent engine is passed to `the_rules_with_parent` so that
    // new windows created by `window.open` share the same JS engine
    // context (same GC heap on JSC).  We obtain it via a raw pointer
    // since `ec` is `&mut dyn ExecutionContext<Types>`.
    let parent_engine: Option<&mut crate::js::Engine> = None; // TODO: thread engine through ec

    let result = the_rules_with_parent(
        parent_engine,
        source_navigable_id,
        parent_traversable_id,
        top_level_traversable_id,
        target,
        noopener,
        Some(global_scope),
        Some(ec.global_object()),
    );

    let navigate_url = url_record.unwrap_or_else(|| String::from("about:blank"));

    // Step 14: "If chosen is a navigable, then set targetNavigable to chosen."
    //          <https://html.spec.whatwg.org/#window-open-steps>
    //
    // Step 14 is handled inside `the_rules_for_choosing_a_navigable`, which
    // resolves _self, _parent, _top (steps 3–5) and creates new traversables
    // locally (step 7/8) when called with a `GlobalScope`.  The result struct
    // carries back the chosen navigable ID, any new traversable info, and
    // the Window backing the WindowProxy.

    if let Err(error) = super::navigate(
        event_sender,
        source_navigable_id,
        result.chosen_navigable_id,
        navigate_url,
        target.to_owned(),
        UserNavigationInvolvement::Activation,
        noopener,
        Some(referrer_policy),
        Some(features_json),
        result.new_traversable_info,
        None,
    ) {
        error!("window.open: {error}");
    }

    // Step 17: "If noopener is true or windowType is 'new with no opener',
    //           then return null."
    if noopener {
        return Ok(ec.value_null());
    }

    // Step 18: Return targetNavigable's active WindowProxy.
    // <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
    let window = result
        .return_window
        .expect("window_open_steps: all navigable branches set a return window");
    create_window_proxy(&window, ec)
}

/// <https://html.spec.whatwg.org/#get-noopener-for-window-open>
fn get_noopener_for_window_open(
    tokenized_features: &HashMap<String, String>,
    url: Option<&str>,
) -> bool {
    // Step 1: "If url is not null and url's blob URL entry is not null:"
    // Note: Blob URL origin checks are not yet implemented.
    let _ = url;

    // Step 2: "Let noopener be false."
    // Step 3: "If tokenizedFeatures['noopener'] exists, then set noopener to the result of
    //          parsing tokenizedFeatures['noopener'] as a boolean feature."
    // Step 4: "Return noopener."
    tokenized_features
        .get("noopener")
        .map(|value| parse_boolean_feature(value))
        .unwrap_or(false)
}

/// <https://html.spec.whatwg.org/#tokenize-the-features-argument>
fn tokenize_features(features: &str) -> HashMap<String, String> {
    // Step 1: "Let tokenizedFeatures be a new ordered map."
    let mut tokenized_features = HashMap::new();

    // Step 2: "Let position point at the first code point of features."
    let bytes = features.as_bytes();
    let mut position = 0;
    let len = bytes.len();

    // Step 3: "While position is not past the end of features:"
    while position < len {
        // Skip leading separators before name.
        while position < len && is_feature_separator(bytes[position]) {
            position += 1;
        }
        if position >= len {
            break;
        }

        // Collect name: not-feature-separator characters, lowercased.
        let name_start = position;
        while position < len && !is_feature_separator(bytes[position]) {
            position += 1;
        }
        let mut name: String = features[name_start..position]
            .chars()
            .flat_map(|c| c.to_lowercase())
            .collect();

        // "Set name to the result of normalizing the feature name name."
        name = normalize_feature_name(&name);

        // Skip to first '=' but not past ','
        while position < len && bytes[position] != b'=' && bytes[position] != b',' {
            position += 1;
        }

        // Skip past '='
        if position < len && bytes[position] == b'=' {
            position += 1;
        }

        // Skip separators (but not comma)
        while position < len && is_feature_separator(bytes[position]) && bytes[position] != b',' {
            position += 1;
        }

        // Collect value: not-feature-separator characters, lowercased.
        let value_start = position;
        while position < len && !is_feature_separator(bytes[position]) {
            position += 1;
        }
        let value: String = features[value_start..position]
            .chars()
            .flat_map(|c| c.to_lowercase())
            .collect();

        // "If name is not the empty string, then set tokenizedFeatures[name] to value."
        if !name.is_empty() {
            tokenized_features.insert(name, value);
        }

        // Skip separators (including comma) before next iteration.
        while position < len && is_feature_separator(bytes[position]) {
            position += 1;
        }
    }

    // Step 4: "Return tokenizedFeatures."
    tokenized_features
}

/// <https://html.spec.whatwg.org/#feature-separator>
fn is_feature_separator(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == b'\x0C' || c == b'=' || c == b','
}

/// <https://html.spec.whatwg.org/#normalize-feature-name>
fn normalize_feature_name(name: &str) -> String {
    match name {
        "screenx" => String::from("left"),
        "screeny" => String::from("top"),
        "innerwidth" => String::from("width"),
        "innerheight" => String::from("height"),
        other => other.to_owned(),
    }
}

/// <https://html.spec.whatwg.org/#parse-a-boolean-feature>
fn parse_boolean_feature(value: &str) -> bool {
    // Step 1: "If value is the empty string, then return true."
    if value.is_empty() {
        return true;
    }
    // Step 2: "If value is 'yes', then return true."
    // Step 3: "If value is 'true', then return true."
    if value == "yes" || value == "true" {
        return true;
    }
    // Step 4: "Let parsed be the result of parsing value as an integer."
    // Step 5: "If parsed is an error, then set it to 0."
    // Step 6: "Return false if parsed is 0, and true otherwise."
    let parsed: i64 = value.parse().unwrap_or(0);
    parsed != 0
}

/// <https://html.spec.whatwg.org/#check-if-a-popup-window-is-requested>
#[allow(dead_code)]
pub(crate) fn check_if_popup_window_is_requested(
    tokenized_features: &HashMap<String, String>,
) -> bool {
    // Step 1: "If tokenizedFeatures is empty, then return false."
    if tokenized_features.is_empty() {
        return false;
    }
    // Step 2: "If tokenizedFeatures['popup'] exists, then return the result of parsing..."
    if let Some(value) = tokenized_features.get("popup") {
        return parse_boolean_feature(value);
    }
    // Steps 3–13: check individual features
    let location = check_if_window_feature_is_set(tokenized_features, "location", false);
    let toolbar = check_if_window_feature_is_set(tokenized_features, "toolbar", false);
    if !location && !toolbar {
        return true;
    }
    let menubar = check_if_window_feature_is_set(tokenized_features, "menubar", false);
    if !menubar {
        return true;
    }
    let resizable = check_if_window_feature_is_set(tokenized_features, "resizable", true);
    if !resizable {
        return true;
    }
    let scrollbars = check_if_window_feature_is_set(tokenized_features, "scrollbars", false);
    if !scrollbars {
        return true;
    }
    let status = check_if_window_feature_is_set(tokenized_features, "status", false);
    if !status {
        return true;
    }
    // Step 14: "Return false."
    false
}

/// <https://html.spec.whatwg.org/#check-if-a-window-feature-is-set>
pub(crate) fn check_if_window_feature_is_set(
    tokenized_features: &HashMap<String, String>,
    feature_name: &str,
    default_value: bool,
) -> bool {
    // Step 1: "If tokenizedFeatures[featureName] exists, then return the result of parsing
    //          tokenizedFeatures[featureName] as a boolean feature."
    if let Some(value) = tokenized_features.get(feature_name) {
        return parse_boolean_feature(value);
    }
    // Step 2: "Return defaultValue."
    default_value
}
