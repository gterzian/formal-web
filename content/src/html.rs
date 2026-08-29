use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::{Engine, Types};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;
use log::error;
mod activation_behavior;
pub(crate) mod channel_messaging;
pub(crate) use channel_messaging::ChannelMessaging;
pub(crate) mod dispatch;
pub(crate) mod environment_settings_object;
pub(crate) mod event_handler;
pub(crate) mod event_loop;
mod global_scope;
mod html_anchor_element;
mod html_dom_tree;
mod html_element;
pub(crate) mod html_iframe_element;
pub(crate) mod html_input_element;
pub(crate) mod html_media_element;
mod html_parser;
pub(crate) mod html_video_element;
mod hyperlink_element_utils;
mod location;
pub(crate) mod message_event;
pub(crate) mod messageport;
pub(crate) mod structured_data;
pub(crate) mod timers;
pub(crate) mod ui_events;
mod window;
mod window_or_worker_global_scope;
pub(crate) mod windowproxy;

use ipc::IpcSender;
use ipc_messages::content::{
    DocumentId, Event as ContentEvent, NavigableId, NavigateRequest, NavigationId,
    NewChildNavigableInfo, NewTraversableInfo, UserNavigationInvolvement,
};

use environment_settings_object::RealmWiring;

pub(crate) use activation_behavior::ActivationBehavior;
pub use environment_settings_object::EnvironmentSettingsObject;
pub use global_scope::GlobalScope;
pub use global_scope::GlobalScopeKind;
pub(crate) use global_scope::TimerHandler;

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
pub(crate) use html_iframe_element::{
    fire_deferred_iframe_load_events, run_iframe_load_event_steps_for_traversable,
};
pub use html_input_element::HTMLInputElement;
pub use html_media_element::{HTMLMediaElement, MediaError};
pub(crate) use html_parser::PendingParserScript;
pub use html_parser::{JsHtmlParserProvider, execute_parser_scripts, parse_html_into_document};
pub use html_video_element::HTMLVideoElement;
pub(crate) use hyperlink_element_utils::HyperlinkElementUtils;
pub use location::Location;
pub(crate) use location::LocationError;
pub(crate) use message_event::{MessageEvent, MessageEventInit};
pub(crate) use messageport::{MessageChannel, MessagePort};
pub use window::Window;
pub(crate) use window::window_computed_style_properties_for_element;
pub(crate) use window::{PostMessageOptions, window_post_message_steps};
pub(crate) use window_or_worker_global_scope::WindowOrWorkerGlobalScope;
pub(crate) use windowproxy::WindowProxy;

use blitz_dom::{BaseDocument, DocumentConfig};
use std::{cell::RefCell, rc::Rc};
use url::Url;

/// <https://html.spec.whatwg.org/#queue-a-microtask>
pub fn queue_a_microtask<F>(ec: &mut dyn ExecutionContext<crate::js::Types>, callback: F)
where
    F: FnOnce(&mut dyn ExecutionContext<crate::js::Types>) -> Completion<JsValue, crate::js::Types>
        + 'static,
{
    // Note: Steps 1-7 (asserting a surrounding agent, setting eventLoop,
    // creating a new task, setting its steps/source/document/settings-object
    // set) are handled by the engine's job queue.  The realm carries
    // the agent/event-loop association.
    //
    // Step 1: Assert: there is a surrounding agent. I.e., this algorithm is
    //         not called while in parallel.
    let realm = ec.current_realm();

    // Step 9: Enqueue microtask on eventLoop's microtask queue.
    ec.enqueue_job_with_realm(
        realm,
        Box::new(move |job_ec| {
            let _ = callback(job_ec);
        }),
    );
}

/// <https://html.spec.whatwg.org/#await-a-stable-state>
pub fn await_a_stable_state<F>(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    synchronous_section: F,
) where
    F: FnOnce(&mut dyn ExecutionContext<crate::js::Types>) -> Completion<JsValue, crate::js::Types>
        + 'static,
{
    // Note: The preamble ("queue a microtask that runs the following steps, and
    // must then stop executing") is implemented by delegating to
    // queue_a_microtask.  The "stop executing" semantics are inherent: queuing
    // a microtask returns immediately and the synchronous section runs later.
    //
    // Step 1: Run the algorithm's synchronous section.
    //
    // Step 2: Resume execution of the algorithm in parallel, if appropriate, as
    //         described in the algorithm's steps.
    //         (Implicit — after the synchronous section returns, control
    //         resumes in the calling algorithm's in-parallel context.)
    queue_a_microtask(ec, synchronous_section);
}

/// <https://html.spec.whatwg.org/#creating-a-new-browsing-context>
pub(crate) fn create_a_new_browsing_context_and_document(
    parent_engine: Option<&mut Engine>,
    creator_origin: Option<environment_settings_object::Origin>,
    wiring: RealmWiring,
) -> Result<
    (
        JsObject,
        Window,
        EnvironmentSettingsObject,
        Rc<RefCell<BaseDocument>>,
    ),
    String,
> {
    // Step 1: Let browsingContext be a new browsing context.
    // Step 2: Let unsafeContextCreationTime be the unsafe shared current time.
    // Step 3: Let creatorOrigin be null.
    // Step 4: Let creatorBaseURL be null.
    // Step 5: If creator is non-null:
    // Step 5.1: Set creatorOrigin to creator's origin.
    // Step 5.2: Set creatorBaseURL to creator's document base URL.
    // Step 5.3: Set browsingContext's virtual browsing context group ID to
    //           creator's browsing context's top-level browsing context's
    //           virtual browsing context group ID.
    // Note: Step 1 runs in the user agent: `UserAgent::create_a_new_browsing_context`
    // allocates the browsing context and registers it in the browsing context
    // group (`traversable_id` is its navigable id).  Steps 2-5 are not
    // implemented: creation time and creator state are not tracked.
    // Step 6: Let sandboxFlags be the result of determining the creation
    //         sandboxing flags given browsingContext and embedder.
    // Note: Not implemented: no sandboxing flags are tracked.
    // Step 7: Let origin be the result of determining the origin given
    //         about:blank, sandboxFlags, and creatorOrigin.
    // Note: Implemented: the creator branch is threaded through as
    // `creator_origin` (the parent's origin for child navigables, the opener's
    // origin for window.open popups); the sandbox branch is not implemented.
    // The inherited origin is what lets the initial about:blank Window be
    // reused for a same-origin first navigation (step 6 of
    // `initialise-the-document-object`).
    // Step 8: Let permissionsPolicy be the result of creating a permissions
    //         policy given embedder and origin.
    // Note: Not implemented.
    // Step 9: Let agent be the result of obtaining a similar-origin window
    //         agent given origin, group, and false.
    // Note: Runs in the user agent: `UserAgent::create_a_new_browsing_context`
    // allocates the agent and agent cluster; child navigables reuse the
    // parent's event loop.
    //
    // Step 15: Let document be a new Document, with: type "html"; content type
    // "text/html"; mode "quirks"; origin origin; browsing context
    // browsingContext; permissions policy permissionsPolicy; active sandboxing
    // flag set sandboxFlags; load timing info loadTimingInfo; is initial
    // about:blank true; about base URL creatorBaseURL; allow declarative shadow
    // roots true; custom element registry a new CustomElementRegistry object.
    // Note: Run ahead of steps 10 and 13, which the spec performs before
    // creating the Document: the realm setup below needs the domain-level
    // Document to build the Document and Window platform objects.  The
    // platform Document object is created inside step 10, within the new
    // realm.  "is initial about:blank" is tracked in the user agent's
    // document state; the remaining Document properties are not implemented.
    let document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig {
        viewport: None,
        base_url: None,
        net_provider: None,
        shell_provider: None,
        html_parser_provider: None,
        ..DocumentConfig::default()
    })));

    // Step 10: Let realm execution context be the result of creating a new realm
    // given agent and the following customizations: for the global object, create
    // a new Window object; for the global this binding, use browsingContext's
    // WindowProxy object.
    // Step 13: Set up a window environment settings object with about:blank,
    // realm execution context, null, topLevelCreationURL, and topLevelOrigin.
    // Note: Steps 10 and 13 run together in `create_a_new_realm`, which returns
    // the settings object whose realm execution context backs the new realm.
    // Step 11: Let topLevelCreationURL be about:blank if embedder is null;
    // otherwise embedder's relevant settings object's top-level creation URL.
    // Step 12: Let topLevelOrigin be origin if embedder is null; otherwise
    // embedder's relevant settings object's top-level origin.
    // Step 14: Let loadTimingInfo be a new document load timing info with its
    // navigation start time set to the result of calling coarsen time with
    // unsafeContextCreationTime and the new environment settings object's
    // cross-origin isolated capability.
    // Note: Steps 11-12 and 14 are not implemented: top-level creation
    // URL/origin and load timing info are not tracked.
    let (global_object, window, settings) =
        create_a_new_realm(parent_engine, Rc::clone(&document), creator_origin, wiring)?;

    // Step 16: Let iframeReferrerPolicy be the result of determining the iframe
    //          element referrer policy given embedder.
    // Step 17: Set document's internal ancestor origin objects list to the
    //          result of running the internal ancestor origin objects list
    //          creation steps given document and iframeReferrerPolicy.
    // Step 18: Set document's ancestor origins list to the result of running
    //          the ancestor origins list creation steps given document.
    // Note: Steps 16-18 are not implemented: referrer and ancestor state is
    // not tracked.
    // Step 19: If creator is non-null:
    // Step 19.1: Set document's referrer to the serialization of creator's URL.
    // Step 19.2: Set document's policy container to a clone of creator's policy
    //            container.
    // Step 19.3: If creator's origin is same origin with creator's relevant
    //            settings object's top-level origin, then set document's opener
    //            policy to creator's browsing context's top-level browsing
    //            context's active document's opener policy.
    // Note: Not implemented: creator-based state is not tracked.
    // Step 20: Assert: document's URL and document's relevant settings object's
    //          creation URL are about:blank.
    // Note: Holds: the creation URL passed to `new_in_realm` is about:blank.
    // Step 21: Mark document as ready for post-load tasks.
    // Note: Not implemented.
    // Step 22: Populate with html/head/body given document.
    parse_html_into_document(&mut document.borrow_mut(), crate::EMPTY_HTML_DOCUMENT);

    // Step 23: Make active document.
    // Note: The user agent's `create_a_new_browsing_context` records the
    // active document; the caller of this function also records the document
    // in `active_documents_by_traversable`.
    // Step 24: Completely finish loading document.
    // Note: Not run here: the user-agent-initiated path executes
    // parser-discovered scripts of the initial about:blank document in
    // content/src/main.rs.
    // Step 25: Return browsingContext and document.
    // Note: The browsing context is `traversable_id` on the user-agent side.
    // The caller must keep the returned environment settings object alive —
    // dropping it drops the realm execution context and invalidates JsObject
    // handles.
    Ok((global_object, window, settings, document))
}

/// <https://html.spec.whatwg.org/#creating-a-new-javascript-realm>
pub(crate) fn create_a_new_realm(
    parent_engine: Option<&mut Engine>,
    document: Rc<RefCell<BaseDocument>>,
    creator_origin: Option<environment_settings_object::Origin>,
    wiring: RealmWiring,
) -> Result<(JsObject, Window, EnvironmentSettingsObject), String> {
    // Step 1: Perform InitializeHostDefinedRealm() with the provided
    // customizations for creating the global object and the global this binding.
    // Note: The customizations come from step 10 of "create a new browsing
    // context and document": for the global object, create a new Window object
    // (created by the realm setup and extracted below); for the global this
    // binding, use the browsing context's WindowProxy object (approximated —
    // the global this binding is the realm's global object; WindowProxy
    // platform objects are created lazily for cross-realm access).  The realm
    // is built by `EnvironmentSettingsObject::new_in_realm` via
    // build_realm/build_context, which also creates the Document platform
    // object for `document` (step 15 of the parent algorithm).
    // Steps 2-4: Let realm execution context be the running JavaScript
    // execution context.  Remove realm execution context from the JavaScript
    // execution context stack.  Let realm be realm execution context's Realm
    // component.
    // Note: Execution-context bookkeeping is performed inside the engine; the
    // resulting realm execution context is exposed as
    // `settings.realm_execution_context`.
    // Step 5: If agent's agent cluster's cross-origin isolation mode is "none":
    // Step 5.1: Let global be realm's global object.
    // Step 5.2: Let status be ! global.[[Delete]]("SharedArrayBuffer").
    // Step 5.3: Assert: status is true.
    // Note: Not performed: the SharedArrayBuffer property is not deleted from
    // the new realm's global object, even though the cross-origin isolation
    // mode of the browsing context groups this browser creates is "none" (see
    // `CrossOriginIsolationMode` in the user agent).
    // Step 6: Return realm execution context.
    // Note: Step 13 of the parent algorithm (set up a window environment
    // settings object) is performed by the same call, which is why this
    // function returns the settings object rather than a bare realm execution
    // context.
    let settings = EnvironmentSettingsObject::new_in_realm(
        parent_engine,
        document,
        Url::parse("about:blank").map_err(|error| error.to_string())?,
        creator_origin,
        Some(wiring),
    )?;

    // Note: The Window platform object for the step 10 customization (for the
    // global object, create a new Window object) was created during realm
    // setup; extract it from the new realm's global object while the realm is
    // current.
    let global_object = settings.realm_execution_context.realm_global_object();
    let window = settings
        .realm_execution_context
        .with_object_any(&global_object)
        .and_then(|data| data.downcast_ref::<Window>().cloned())
        .ok_or_else(|| String::from("realm global object is not a Window"))?;
    Ok((global_object, window, settings))
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
pub(crate) struct ChosenNavigableResult {
    pub(crate) chosen_navigable_id: Option<NavigableId>,
    pub(crate) new_traversable_info: Option<NewTraversableInfo>,
    pub(crate) return_window: Option<(Window, JsObject)>,
}

/// <https://html.spec.whatwg.org/#the-rules-for-choosing-a-navigable>
pub(crate) fn the_rules_for_choosing_a_navigable(
    source_navigable_id: NavigableId,
    parent_navigable_id: Option<NavigableId>,
    top_level_navigable_id: NavigableId,
    target_name: &str,
    noopener: bool,
    global_scope: Option<&GlobalScope>,
    window_global: Option<<crate::js::Types as js_engine::JsTypes>::JsObject>,
    parent_engine: Option<&mut Engine>,
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
        if let Some(global_scope) = global_scope {
            if window_global.is_some() {
                // window.open path with opener: create the about:blank document
                // locally since the new auxiliary BC reuses the opener's BCG.
                // The UA continues via `new_traversable_info` in NavigateRequest.
                let new_traversable_id = NavigableId::new();
                let new_document_id = DocumentId::new();

                let (global_object, window, settings, document) = match global_scope
                    .create_auxiliary_context_document(
                        parent_engine,
                        new_traversable_id,
                        new_document_id,
                    ) {
                    Ok(result) => result,
                    Err(error) => {
                        error!(
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
                    error!(
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
                    return_window: Some((window, global_object)),
                };
            }

            // Anchor-navigation path (or missing window context): delegate to UA.
            return ChosenNavigableResult {
                chosen_navigable_id: None,
                new_traversable_info: None,
                return_window: None,
            };
        }

        // No GlobalScope: delegate to UA.
        return ChosenNavigableResult {
            chosen_navigable_id: None,
            new_traversable_info: None,
            return_window: None,
        };
    };

    // Step 9: Return chosen and windowType.
    // Note: windowType is always "existing or none" (Step 2 deferred).
    // The return_window for _self / _parent / _top is the source document's
    // Window (correct for _self; _parent and _top that target a
    // different process are a known gap — see content/src/html/README.md).
    // The realm is the source window's realm here, so its platform data is
    // reachable from the parent engine's execution context.
    let return_window = window_global.as_ref().and_then(|object| {
        parent_engine
            .as_ref()
            .and_then(|engine| engine.with_object_any(object))
            .and_then(|data| data.downcast_ref::<Window>().cloned())
            .map(|window| (window, object.clone()))
    });
    ChosenNavigableResult {
        chosen_navigable_id: Some(chosen),
        new_traversable_info: None,
        return_window,
    }
}
