use log::error;
use std::collections::{BTreeMap, HashMap};

use ipc::IpcSender;
use ipc_messages::content::{Event as ContentEvent, NavigableId, UserNavigationInvolvement};
use ipc_messages::safe_passing_of_structured_data::PostMessageRequest;

use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::{Engine, Types};

type JsValue = <Types as JsTypes>::JsValue;

use crate::dom::event::{EventTarget, EventTargetAccess};
use crate::dom::{Document, Element};
use crate::js::platform_objects::with_global_scope;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{relevant_realm_global_this_value, security_error_value, syntax_error_value};

use super::resolved_style_properties_for_element;
use super::structured_data::safe_passing_of_structured_data::structured_serialize_with_transfer;
use super::windowproxy::create_window_proxy;
use super::{GlobalScope, Location, the_rules_for_choosing_a_navigable};
use js_engine::gc_struct;

/// <https://html.spec.whatwg.org/#window>
#[gc_struct]
pub struct Window {
    /// <https://dom.spec.whatwg.org/#interface-eventtarget>
    pub event_target: EventTarget,

    /// <https://html.spec.whatwg.org/#global-object>
    pub global_scope: GlobalScope,
}

impl EventTargetAccess for Window {
    fn get_event_target(&self, _ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.event_target.clone()
    }
}

impl Window {
    pub(crate) fn new(global_scope: GlobalScope, ec: &mut dyn ExecutionContext<Types>) -> Self {
        Self {
            event_target: EventTarget::new(ec),
            global_scope,
        }
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

    /// <https://html.spec.whatwg.org/#dom-window>
    pub(crate) fn window_value(&self, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
        // The window, frames, and self getter steps are to return this's
        // relevant realm.[[GlobalEnv]].[[GlobalThisValue]].
        // <https://html.spec.whatwg.org/#concept-relevant-realm>
        relevant_realm_global_this_value(ec)
    }

    /// <https://html.spec.whatwg.org/#dom-self>
    pub(crate) fn self_value(&self, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
        // The window, frames, and self getter steps are to return this's
        // relevant realm.[[GlobalEnv]].[[GlobalThisValue]].
        // <https://html.spec.whatwg.org/#concept-relevant-realm>
        relevant_realm_global_this_value(ec)
    }

    /// <https://html.spec.whatwg.org/#dom-frames>
    pub(crate) fn frames_value(&self, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
        // The window, frames, and self getter steps are to return this's
        // relevant realm.[[GlobalEnv]].[[GlobalThisValue]].
        // <https://html.spec.whatwg.org/#concept-relevant-realm>
        relevant_realm_global_this_value(ec)
    }

    /// <https://html.spec.whatwg.org/#dom-name>
    pub(crate) fn name_value(&self) -> String {
        // Step 1: If this's navigable is null, then return the empty string.
        if self.global_scope.source_navigable_id().is_none() {
            return String::new();
        }
        // Step 2: Return this's navigable's target name.
        // Note: The navigable target name is tracked by the user agent
        // (`traversable_target_names` in `user_agent/src/user_agent.rs`) and
        // is not sent to the content process, so the getter returns the
        // empty string.
        String::new()
    }

    /// <https://html.spec.whatwg.org/#dom-name>
    pub(crate) fn set_name_value(&self, _: String) {
        // Step 1: If this's navigable is null, then return.
        // Step 2: Set this's navigable's active session history entry's
        //         document state's navigable target name to the given value.
        // Note: The navigable target name is tracked by the user agent
        // (`traversable_target_names` in `user_agent/src/user_agent.rs`);
        // setting it from the content process is not yet wired.
    }

    /// <https://html.spec.whatwg.org/#dom-length>
    pub(crate) fn length_value(&self) -> u32 {
        // The length getter steps are to return this's associated Document's
        // document-tree child navigables's size.
        // Note: Document-tree child navigable tracking is not yet
        // implemented, so the getter returns 0.
        0
    }

    /// <https://html.spec.whatwg.org/#dom-top>
    pub(crate) fn top_value(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        // Step 1: If this's navigable is null, then return null.
        let Some(navigable_id) = self.global_scope.source_navigable_id() else {
            return Ok(ec.value_null());
        };
        // Step 2: Return this's navigable's top-level traversable's active
        //         WindowProxy.
        let top_level_id = self
            .global_scope
            .top_level_traversable_id()
            .unwrap_or(navigable_id);
        create_window_proxy(top_level_id, None, ec)
    }

    /// <https://html.spec.whatwg.org/#dom-parent>
    pub(crate) fn parent_value(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        // Step 1: Let navigable be this's navigable.
        let Some(navigable_id) = self.global_scope.source_navigable_id() else {
            // Step 2: If navigable is null, then return null.
            return Ok(ec.value_null());
        };
        // Step 3: If navigable's parent is not null, then set navigable to
        //         navigable's parent.
        // Note: The realm tracks the parent navigable's id; the top-level
        // window has no parent and keeps its own navigable.
        let navigable_id = match self.global_scope.parent_traversable_id() {
            Some(parent_id) => parent_id,
            None => navigable_id,
        };

        // Step 4: Return navigable's active WindowProxy.
        create_window_proxy(navigable_id, None, ec)
    }

    /// <https://html.spec.whatwg.org/#dom-opener>
    pub(crate) fn opener_value(&self, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
        // Step 1: Let current be this's browsing context.
        // Step 2: If current is null, then return null.
        // Step 3: If current's opener browsing context is null, then return
        //         null.
        // Step 4: Return current's opener browsing context's WindowProxy
        //         object.
        // Note: The opener browsing context id is tracked by the user agent
        // (`BrowsingContext.opener_browsing_context` in
        // `user_agent/src/user_agent.rs`); the content process does not
        // receive it, so the getter cannot resolve the opener's WindowProxy
        // and returns null.
        ec.value_null()
    }

    /// <https://html.spec.whatwg.org/#dom-document>
    pub(crate) fn document_value(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        // The document getter steps are to return this's associated Document.
        // The associated Document platform object is cached on the realm's
        // global scope; it is null only while the window is being torn down.
        match self.global_scope.document_object(ec) {
            Some(document) => Ok(<Types as JsTypes>::value_from_object(document)),
            None => Ok(ec.value_null()),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-location>
    pub(crate) fn location_value(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Location, Types> {
        // The Window object's location getter steps are to return this's
        // Location object.
        // Note: Each Window object is associated with a unique Location
        // object; it is created on first access and cached on the realm's
        // global scope.  The binding layer converts the returned Location to
        // the cached JS object.
        if let Some(location_object) = self.global_scope.location_object(ec) {
            let location = ec
                .with_object_any(&location_object)
                .and_then(|data| data.downcast_ref::<Location>().cloned())
                .ok_or_else(|| ec.new_type_error("location object is not a Location"))?;
            return Ok(location);
        }
        let document_object = self.global_scope.document_object(ec);
        let Some(document_object) = document_object else {
            return Err(ec.new_type_error("window has no document"));
        };
        let document = ec
            .with_object_any(&document_object)
            .and_then(|data| data.downcast_ref::<Document>().cloned())
            .ok_or_else(|| ec.new_type_error("document object is not a Document"))?;
        let location = Location::new(
            document.creation_url.clone(),
            self.global_scope.source_navigable_id(),
            self.global_scope.event_sender(),
        );
        let object = create_interface_instance::<Types, Location>(location.clone(), ec)?;
        self.global_scope.store_location_object(object, ec);
        Ok(location)
    }

    /// <https://html.spec.whatwg.org/#dom-window-postmessage>
    pub(crate) fn post_message(
        &self,
        message: JsValue,
        options: PostMessageOptions,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let target_navigable_id = self
            .global_scope
            .source_navigable_id()
            .ok_or_else(|| ec.new_type_error("postMessage: no target navigable"))?;
        window_post_message_steps(target_navigable_id, message, options, ec)
    }

    /// <https://html.spec.whatwg.org/#dom-window-close>
    pub(crate) fn close(&self) {
        // Step 1: Let thisTraversable be this's navigable.
        // Step 2: If thisTraversable is not a top-level traversable, then
        //         return.
        // Step 3: If thisTraversable's is closing is true, then return.
        // Step 4: Let browsingContext be thisTraversable's active browsing
        //         context.
        // Step 5: Let sourceSnapshotParams be the result of snapshotting
        //         source snapshot params given thisTraversable's active
        //         document.
        // Step 6: If all the following are true: thisTraversable is
        //         script-closable; the incumbent global object's browsing
        //         context is familiar with browsingContext; and the
        //         incumbent global object's navigable is allowed by
        //         sandboxing to navigate thisTraversable, given
        //         sourceSnapshotParams, then:
        // Step 6.1: Set thisTraversable's is closing to true.
        // Step 6.2: Queue a task on the DOM manipulation task source to
        //           definitely close thisTraversable.
        // TODO: The is closing flag and the close task are not implemented
        // in any process yet; closing is not wired.
    }

    /// <https://html.spec.whatwg.org/#dom-window-closed>
    pub(crate) fn closed_value(&self) -> bool {
        // The closed getter steps are to return true if this's browsing
        // context is null or its is closing is true; otherwise false.
        // Note: The is closing flag is not implemented in any process yet;
        // the getter returns false.
        false
    }

    /// <https://html.spec.whatwg.org/#dom-window-focus>
    pub(crate) fn focus(&self) {
        // Step 1: Let current be this's navigable.
        // Step 2: If current is null, then return.
        // Step 3: If the allow focus steps given current's active document
        //         return false, then return.
        // Step 4: Run the focusing steps with current.
        // Step 5: If current is a top-level traversable, user agents are
        //         encouraged to trigger some sort of notification to indicate
        //         to the user that the page is attempting to gain focus.
        // TODO: The allow focus steps and the focusing steps are not yet
        // implemented.
    }

    /// <https://html.spec.whatwg.org/#dom-window-blur>
    pub(crate) fn blur(&self) {
        // The Window blur() method steps are to do nothing.
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

// Window open steps
// https://html.spec.whatwg.org/#window-open-steps

/// <https://html.spec.whatwg.org/#windowpostmessageoptions>
#[derive(Default)]
pub(crate) struct PostMessageOptions {
    /// <https://html.spec.whatwg.org/#dom-windowpostmessageoptions-targetorigin>
    pub target_origin: String,
    /// <https://html.spec.whatwg.org/#dom-windowpostmessageoptions-transfer>
    pub transfer: Vec<JsValue>,
}

/// <https://html.spec.whatwg.org/#window-post-message-steps>
pub(crate) fn window_post_message_steps(
    target_navigable_id: NavigableId,
    message: JsValue,
    options: PostMessageOptions,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step 1: Let targetRealm be targetWindow's realm.
    // Note: The target realm is resolved in the target content process, which
    // owns the targetWindow's realm; the deserialization of step 8.4 runs
    // there.  The message therefore carries only the serialized data.

    // Step 2: Let incumbentSettings be the incumbent settings object.
    let (source_navigable_id, source_origin, event_sender) = with_global_scope(
        ec,
        |global_scope,
         _ec|
         -> Completion<
            (
                Option<NavigableId>,
                Option<String>,
                Option<IpcSender<ContentEvent>>,
            ),
            crate::js::Types,
        > {
            Ok((
                global_scope.source_navigable_id(),
                global_scope
                    .creation_url()
                    .map(|url| url.origin().unicode_serialization()),
                global_scope.event_sender(),
            ))
        },
    )?;
    let Some(source_navigable_id) = source_navigable_id else {
        return Err(ec.new_type_error("postMessage: no source navigable"));
    };
    let Some(source_origin) = source_origin else {
        return Err(ec.new_type_error("postMessage: no source origin"));
    };
    let Some(event_sender) = event_sender else {
        return Err(ec.new_type_error("postMessage: no event sender"));
    };

    // Step 3: Let targetOrigin be options["targetOrigin"].
    let mut target_origin = options.target_origin;

    // Step 4: If targetOrigin is a single U+002F SOLIDUS character (/), then
    //         set targetOrigin to incumbentSettings's origin.
    if target_origin == "/" {
        target_origin = source_origin.clone();
    } else if target_origin != "*" {
        // Step 5: Otherwise, if targetOrigin is not a single U+002A ASTERISK
        //         character (*):
        // Step 5.1: Let parsedURL be the result of running the URL parser on
        //           targetOrigin.
        // Step 5.2: If parsedURL is failure, then throw a "SyntaxError"
        //           DOMException.
        let parsed_url = url::Url::parse(&target_origin).map_err(|_| syntax_error_value(ec))?;

        // Step 5.3: Set targetOrigin to parsedURL's origin.
        target_origin = parsed_url.origin().unicode_serialization();
    }

    // Step 6: Let transfer be options["transfer"].
    let transfer = options.transfer;

    // Step 7: Let serializeWithTransferResult be
    //         StructuredSerializeWithTransfer(message, transfer). Rethrow
    //         any exceptions.
    let serialize_result = structured_serialize_with_transfer(&message, transfer, ec)?;

    // Step 8: Queue a global task on the posted message task source given
    //         targetWindow to run the following steps.
    // Note: The content process runs steps 1–7; the user agent runs step 8
    // (routing the message to the target window's event loop), and the target
    // content process runs the substeps of step 8.
    event_sender
        .send(ContentEvent::PostMessageRequested(PostMessageRequest {
            target_navigable_id,
            target_origin,
            source_navigable_id,
            source_origin,
            serialized: serialize_result.serialized,
            transfer_data_holders: serialize_result.transfer_data_holders,
        }))
        .map_err(|error| ec.new_type_error(&format!("postMessage: {error}")))
}

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
    let url_record = if url.is_empty() {
        None
    } else {
        // Step 4.1: "Set urlRecord to the result of encoding-parsing a URL
        //            given url, relative to sourceDocument."
        // <https://html.spec.whatwg.org/#encoding-parsing-a-url>
        let resolved = match url::Url::parse(url) {
            Ok(absolute) => Some(absolute),
            Err(_) => global_scope
                .creation_url()
                .and_then(|base_url| base_url.join(url).ok()),
        };

        // Step 4.2: "If urlRecord is failure, then throw a 'SyntaxError'
        //            DOMException."
        let Some(resolved) = resolved else {
            return Err(syntax_error_value(ec));
        };

        // <https://html.spec.whatwg.org/#beginning-navigation:allowed-to-navigate>
        // Note: The navigate algorithm's step 6.2 ("If sourceDocument's node
        // navigable is not allowed by sandboxing to navigate navigable
        // [...] if exceptionsEnabled is true, then throw a 'SecurityError'
        // DOMException") runs here, for the sole case implemented — a
        // destination whose origin differs from the source document's —
        // because the content-side navigate hands the navigation to the user
        // agent and cannot throw; running it before step 13 also leaves no
        // navigable created for a blocked navigation.  `about:` and `file:`
        // URLs get a fresh opaque origin from the URL parser (about:blank
        // inherits its creator's origin, and local files are treated as
        // same-origin here), so they are exempt.
        if resolved.scheme() != "about"
            && resolved.scheme() != "file"
            && let Some(creation_url) = global_scope.creation_url()
            && creation_url.origin() != resolved.origin()
        {
            return Err(security_error_value(ec));
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

    // Step 13: "Let targetNavigable and windowType be the result of applying
    //           the rules for choosing a navigable given target,
    //           sourceDocument's node navigable, and noopener."
    // <https://html.spec.whatwg.org/#the-rules-for-choosing-a-navigable>
    let parent_traversable_id = global_scope.parent_traversable_id();
    let top_level_traversable_id = global_scope
        .top_level_traversable_id()
        .unwrap_or(source_navigable_id);

    let window_global = ec.global_object();
    let parent_engine = ec.as_any_mut().downcast_mut::<Engine>();
    let result = the_rules_for_choosing_a_navigable(
        source_navigable_id,
        parent_traversable_id,
        top_level_traversable_id,
        target,
        noopener,
        Some(global_scope),
        Some(window_global),
        parent_engine,
    );

    // Step 14: "If targetNavigable is null, then return null."
    // Note: The rules for choosing a navigable also report no navigable when
    // the user agent is the side that creates the new traversable (the
    // null-opener branch of their step 8), and that case still has to send
    // the navigation request below; no navigable with noopener false is the
    // local auxiliary document creation having failed.
    if result.chosen_navigable_id.is_none() && !noopener {
        return Ok(ec.value_null());
    }

    // Step 15: "If windowType is either 'new and unrestricted' or 'new with
    //           no opener':"
    // Note: windowType is not tracked (step 2 of the rules for choosing a
    // navigable is deferred); a navigable those rules created — locally or
    // by the user agent — is the "new" window type.
    let is_new_navigable =
        result.new_traversable_info.is_some() || result.chosen_navigable_id.is_none();

    // Step 15.1: "Set targetNavigable's active browsing context's is popup to
    //             the result of checking if a popup window is requested,
    //             given tokenizedFeatures."
    // TODO: Not yet implemented.

    // Step 15.2: "Set up browsing context features for targetNavigable's
    //             active browsing context given tokenizedFeatures."
    // Note: The remaining features travel with the navigation request for
    // the user agent, which owns the new traversable's window.

    // Step 15.3: "If urlRecord is null, then set urlRecord to a URL record
    //             representing about:blank."
    // Step 15.4: "If urlRecord matches about:blank, then perform the URL and
    //             history update steps given targetNavigable's active
    //             document and urlRecord."
    // Note: The URL and history update steps are not implemented; an
    // about:blank urlRecord takes step 15.5's navigation instead.
    // Step 15.5: "Otherwise, navigate targetNavigable to urlRecord using
    //             sourceDocument, with referrerPolicy set to referrerPolicy
    //             and exceptionsEnabled set to true."
    // Step 16: "Otherwise:"
    // Step 16.1: "If urlRecord is not null, then navigate targetNavigable to
    //             urlRecord using sourceDocument, with referrerPolicy set to
    //             referrerPolicy and exceptionsEnabled set to true."
    let should_navigate = is_new_navigable || url_record.is_some();
    let navigate_url = url_record.unwrap_or_else(|| String::from("about:blank"));
    if should_navigate
        && let Err(error) = super::navigate(
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
        )
    {
        error!("window.open: {error}");
    }

    // Step 16.2: "If noopener is false, then set targetNavigable's active
    //             browsing context's opener browsing context to
    //             sourceDocument's browsing context."
    // Note: The opener browsing context is user-agent state, set from the
    // navigation request sent above (`setup_opener_for_window_open`).

    // Step 17: "If windowType is 'new with no opener', then return null."
    // Step 18: "If noopener is true and target is not an ASCII
    //           case-insensitive match for '_self', '_parent', or '_top',
    //           then return null."
    // Note: windowType is not tracked, so step 17's "new with no opener"
    // window is the one the rules for choosing a navigable created with a
    // null opener, i.e. noopener being true, which step 18 already covers
    // for every target but _self, _parent, and _top.
    if noopener
        && !target.eq_ignore_ascii_case("_self")
        && !target.eq_ignore_ascii_case("_parent")
        && !target.eq_ignore_ascii_case("_top")
    {
        return Ok(ec.value_null());
    }

    // Step 19: "Return targetNavigable's active WindowProxy."
    // <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
    let navigable_id = result
        .chosen_navigable_id
        .expect("window_open_steps: all navigable branches set a chosen navigable");
    create_window_proxy(navigable_id, result.return_window, ec)
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
