use boa_engine::{JsData, object::JsObject};
use boa_gc::{Finalize, Trace};
use ipc_messages::content::UserNavigationInvolvement;
use url::{Host, Url};

use super::Window;

/// <https://html.spec.whatwg.org/#location>
#[derive(Trace, Finalize, JsData)]
pub struct Location {
    /// Model-local backing URL used for Location attribute serialization and URL parsing.
    ///
    /// Note: The spec defines Location.url in terms of the relevant Document URL. This implementation
    /// currently snapshots that URL when creating the Location object.
    #[unsafe_ignore_trace]
    url: Url,

    /// <https://html.spec.whatwg.org/#relevant-document>
    #[unsafe_ignore_trace]
    relevant_document_origin: Option<String>,

    /// <https://html.spec.whatwg.org/#concept-relevant-global>
    /// The Window JS object that owns the GlobalScope with navigation state.
    /// Location uses `downcast_ref` through this handle to access the native
    /// Window struct — this is safe and doesn't require raw pointer manipulation.
    window: JsObject,
}

pub(crate) enum LocationError {
    Security,
    Syntax,
    NotSupported(String),
}

/// <https://html.spec.whatwg.org/#navigationhistorybehavior>
enum NavigationHistoryBehavior {
    Auto,
    Replace,
}

impl Location {
    pub(crate) fn new(url: Url, window: JsObject) -> Self {
        Self {
            relevant_document_origin: Some(url.origin().unicode_serialization()),
            url,
            window,
        }
    }

    /// <https://html.spec.whatwg.org/#dom-location-href>
    pub(crate) fn href(&self, entry_settings_origin: &str) -> Result<String, LocationError> {
        // Step 1: "If this's relevant Document is non-null and its origin is not same
        // origin-domain with the entry settings object's origin, then throw a SecurityError
        // DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 2: "Return this's url, serialized."
        Ok(self.url.as_str().to_owned())
    }

    /// <https://html.spec.whatwg.org/#dom-location-origin>
    pub(crate) fn origin(&self, entry_settings_origin: &str) -> Result<String, LocationError> {
        // Step 1: "If this's relevant Document is non-null and its origin is not same
        // origin-domain with the entry settings object's origin, then throw a SecurityError
        // DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 2: "Return the serialization of this's url's origin."
        Ok(self.url.origin().unicode_serialization())
    }

    /// <https://html.spec.whatwg.org/#dom-location-protocol>
    pub(crate) fn protocol(&self, entry_settings_origin: &str) -> Result<String, LocationError> {
        // Step 1: "If this's relevant Document is non-null and its origin is not same
        // origin-domain with the entry settings object's origin, then throw a SecurityError
        // DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 2: "Return this's url's scheme, followed by ':'."
        Ok(format!("{}:", self.url.scheme()))
    }

    /// <https://html.spec.whatwg.org/#dom-location-host>
    pub(crate) fn host(&self, entry_settings_origin: &str) -> Result<String, LocationError> {
        // Step 1: "If this's relevant Document is non-null and its origin is not same
        // origin-domain with the entry settings object's origin, then throw a SecurityError
        // DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 2: "Let url be this's url."
        let url = &self.url;

        // Step 3: "If url's host is null, return the empty string."
        if url.host().is_none() {
            return Ok(String::new());
        }

        // Step 4: "If url's port is null, return url's host, serialized."
        if url.port().is_none() {
            return Ok(url.host_str().unwrap_or_default().to_owned());
        }

        // Step 5: "Return url's host, serialized, followed by ':' and url's port,
        // serialized."
        let hostname = url.host_str().unwrap_or_default();
        let port = url.port().unwrap_or_default();
        Ok(format!("{hostname}:{port}"))
    }

    /// <https://html.spec.whatwg.org/#dom-location-hostname>
    pub(crate) fn hostname(&self, entry_settings_origin: &str) -> Result<String, LocationError> {
        // Step 1: "If this's relevant Document is non-null and its origin is not same
        // origin-domain with the entry settings object's origin, then throw a SecurityError
        // DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 2: "If this's url's host is null, return the empty string."
        if self.url.host().is_none() {
            return Ok(String::new());
        }

        // Step 3: "Return this's url's host, serialized."
        Ok(self.url.host_str().unwrap_or_default().to_owned())
    }

    /// <https://html.spec.whatwg.org/#dom-location-port>
    pub(crate) fn port(&self, entry_settings_origin: &str) -> Result<String, LocationError> {
        // Step 1: "If this's relevant Document is non-null and its origin is not same
        // origin-domain with the entry settings object's origin, then throw a SecurityError
        // DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 2: "If this's url's port is null, return the empty string."
        let Some(port) = self.url.port() else {
            return Ok(String::new());
        };

        // Step 3: "Return this's url's port, serialized."
        Ok(port.to_string())
    }

    /// <https://html.spec.whatwg.org/#dom-location-pathname>
    pub(crate) fn pathname(&self, entry_settings_origin: &str) -> Result<String, LocationError> {
        // Step 1: "If this's relevant Document is non-null and its origin is not same
        // origin-domain with the entry settings object's origin, then throw a SecurityError
        // DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 2: "Return this's url's path."
        Ok(self.url.path().to_owned())
    }

    /// <https://html.spec.whatwg.org/#dom-location-search>
    pub(crate) fn search(&self, entry_settings_origin: &str) -> Result<String, LocationError> {
        // Step 1: "If this's relevant Document is non-null and its origin is not same
        // origin-domain with the entry settings object's origin, then throw a SecurityError
        // DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 2: "If this's url's query is either null or the empty string, return the empty
        // string."
        let Some(query) = self.url.query() else {
            return Ok(String::new());
        };
        if query.is_empty() {
            return Ok(String::new());
        }

        // Step 3: "Return '?', followed by this's url's query."
        Ok(format!("?{query}"))
    }

    /// <https://html.spec.whatwg.org/#dom-location-hash>
    pub(crate) fn hash(&self, entry_settings_origin: &str) -> Result<String, LocationError> {
        // Step 1: "If this's relevant Document is non-null and its origin is not same
        // origin-domain with the entry settings object's origin, then throw a SecurityError
        // DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 2: "If this's url's fragment is either null or the empty string, return the
        // empty string."
        let Some(fragment) = self.url.fragment() else {
            return Ok(String::new());
        };
        if fragment.is_empty() {
            return Ok(String::new());
        }

        // Step 3: "Return '#', followed by this's url's fragment."
        Ok(format!("#{fragment}"))
    }

    /// <https://html.spec.whatwg.org/#dom-location-href>
    pub(crate) fn set_href_with_origin(
        &self,
        value: &str,
        entry_settings_base_url: &Url,
    ) -> Result<(), LocationError> {
        // Step 1: "If this's relevant Document is null, then return."
        if self.relevant_document_is_null() {
            return Ok(());
        }

        // Step 2: "Let url be the result of encoding-parsing a URL given the given value,
        // relative to the entry settings object."
        let url = self.parse_url_relative_to_entry_settings(value, entry_settings_base_url)?;

        // Step 3: "If url is failure, then throw a SyntaxError DOMException."
        // Note: parse_url_relative_to_entry_settings maps parse failure to SyntaxError.

        // Step 4: "Location-object navigate this to url."
        self.location_object_navigate(&url, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-protocol>
    pub(crate) fn set_protocol_with_origin(
        &self,
        value: &str,
        entry_settings_origin: &str,
    ) -> Result<(), LocationError> {
        // Step 1: "If this's relevant Document is null, then return."
        if self.relevant_document_is_null() {
            return Ok(());
        }

        // Step 2: "If this's relevant Document's origin is not same origin-domain with the
        // entry settings object's origin, then throw a SecurityError DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 3: "Let copyURL be a copy of this's url."
        let mut copy_url = self.url.clone();

        // Step 4: "Let possibleFailure be the result of basic URL parsing ... with scheme start
        // state as state override."
        let scheme = value.trim_end_matches(':');
        if scheme.is_empty() || copy_url.set_scheme(scheme).is_err() {
            // Step 5: "If possibleFailure is failure, then throw a SyntaxError DOMException."
            return Err(LocationError::Syntax);
        }

        // Step 6: "If copyURL's scheme is not an HTTP(S) scheme, then terminate these steps."
        if !matches!(copy_url.scheme(), "http" | "https") {
            return Ok(());
        }

        // Step 7: "Location-object navigate this to copyURL."
        self.location_object_navigate(&copy_url, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-host>
    pub(crate) fn set_host_with_origin(
        &self,
        value: &str,
        entry_settings_origin: &str,
    ) -> Result<(), LocationError> {
        // Step 1: "If this's relevant Document is null, then return."
        if self.relevant_document_is_null() {
            return Ok(());
        }

        // Step 2: "If this's relevant Document's origin is not same origin-domain with the
        // entry settings object's origin, then throw a SecurityError DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 3: "Let copyURL be a copy of this's url."
        let mut copy_url = self.url.clone();

        // Step 4: "If copyURL has an opaque path, then return."
        if copy_url.cannot_be_a_base() {
            return Ok(());
        }

        // Step 5: "Basic URL parse the given value ... with host state as state override."
        self.basic_url_parse_host_state(&mut copy_url, value);

        // Step 6: "Location-object navigate this to copyURL."
        self.location_object_navigate(&copy_url, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-hostname>
    pub(crate) fn set_hostname_with_origin(
        &self,
        value: &str,
        entry_settings_origin: &str,
    ) -> Result<(), LocationError> {
        // Step 1: "If this's relevant Document is null, then return."
        if self.relevant_document_is_null() {
            return Ok(());
        }

        // Step 2: "If this's relevant Document's origin is not same origin-domain with the
        // entry settings object's origin, then throw a SecurityError DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 3: "Let copyURL be a copy of this's url."
        let mut copy_url = self.url.clone();

        // Step 4: "If copyURL has an opaque path, then return."
        if copy_url.cannot_be_a_base() {
            return Ok(());
        }

        // Step 5: "Basic URL parse the given value ... with hostname state as state override."
        self.basic_url_parse_hostname_state(&mut copy_url, value);

        // Step 6: "Location-object navigate this to copyURL."
        self.location_object_navigate(&copy_url, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-port>
    pub(crate) fn set_port_with_origin(
        &self,
        value: &str,
        entry_settings_origin: &str,
    ) -> Result<(), LocationError> {
        // Step 1: "If this's relevant Document is null, then return."
        if self.relevant_document_is_null() {
            return Ok(());
        }

        // Step 2: "If this's relevant Document's origin is not same origin-domain with the
        // entry settings object's origin, then throw a SecurityError DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 3: "Let copyURL be a copy of this's url."
        let mut copy_url = self.url.clone();

        // Step 4: "If copyURL cannot have a username/password/port, then return."
        if copy_url.cannot_be_a_base() || copy_url.scheme() == "file" {
            return Ok(());
        }

        // Step 5: "If the given value is the empty string, then set copyURL's port to null."
        if value.is_empty() {
            let _ = copy_url.set_port(None);
        } else {
            // Step 6: "Otherwise, basic URL parse the given value ... with port state as state
            // override."
            self.basic_url_parse_port_state(&mut copy_url, value);
        }

        // Step 7: "Location-object navigate this to copyURL."
        self.location_object_navigate(&copy_url, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-pathname>
    pub(crate) fn set_pathname_with_origin(
        &self,
        value: &str,
        entry_settings_origin: &str,
    ) -> Result<(), LocationError> {
        // Step 1: "If this's relevant Document is null, then return."
        if self.relevant_document_is_null() {
            return Ok(());
        }

        // Step 2: "If this's relevant Document's origin is not same origin-domain with the
        // entry settings object's origin, then throw a SecurityError DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 3: "Let copyURL be a copy of this's url."
        let mut copy_url = self.url.clone();

        // Step 4: "If copyURL has an opaque path, then return."
        if copy_url.cannot_be_a_base() {
            return Ok(());
        }

        // Step 5: "Set copyURL's path to the empty list."
        copy_url.set_path("");

        // Step 6: "Basic URL parse the given value ... with path start state as state
        // override."
        self.basic_url_parse_path_start_state(&mut copy_url, value);

        // Step 7: "Location-object navigate this to copyURL."
        self.location_object_navigate(&copy_url, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-search>
    pub(crate) fn set_search_with_origin(
        &self,
        value: &str,
        entry_settings_origin: &str,
    ) -> Result<(), LocationError> {
        // Step 1: "If this's relevant Document is null, then return."
        if self.relevant_document_is_null() {
            return Ok(());
        }

        // Step 2: "If this's relevant Document's origin is not same origin-domain with the
        // entry settings object's origin, then throw a SecurityError DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 3: "Let copyURL be a copy of this's url."
        let mut copy_url = self.url.clone();

        // Step 4: "If the given value is the empty string, set copyURL's query to null."
        if value.is_empty() {
            copy_url.set_query(None);
        } else {
            // Step 5.1: "Let input be the given value with a single leading '?' removed, if
            // any."
            let input = value.strip_prefix('?').unwrap_or(value);

            // Step 5.2: "Set copyURL's query to the empty string."
            copy_url.set_query(Some(""));

            // Step 5.3: "Basic URL parse input ... with query state as state override."
            self.basic_url_parse_query_state(&mut copy_url, input);
        }

        // Step 6: "Location-object navigate this to copyURL."
        self.location_object_navigate(&copy_url, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-hash>
    pub(crate) fn set_hash_with_origin(
        &self,
        value: &str,
        entry_settings_origin: &str,
    ) -> Result<(), LocationError> {
        // Step 1: "If this's relevant Document is null, then return."
        if self.relevant_document_is_null() {
            return Ok(());
        }

        // Step 2: "If this's relevant Document's origin is not same origin-domain with the
        // entry settings object's origin, then throw a SecurityError DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 3: "Let copyURL be a copy of this's url."
        let mut copy_url = self.url.clone();

        // Step 4: "Let thisURLFragment be copyURL's fragment if it is non-null; otherwise the
        // empty string."
        let this_url_fragment = copy_url.fragment().unwrap_or_default().to_owned();

        // Step 5: "Let input be the given value with a single leading '#' removed, if any."
        let input = value.strip_prefix('#').unwrap_or(value);

        // Step 6: "Set copyURL's fragment to the empty string."
        copy_url.set_fragment(Some(""));

        // Step 7: "Basic URL parse input ... with fragment state as state override."
        self.basic_url_parse_fragment_state(&mut copy_url, input);

        // Step 8: "If copyURL's fragment is thisURLFragment, then return."
        if copy_url.fragment().unwrap_or_default() == this_url_fragment {
            return Ok(());
        }

        // Step 9: "Location-object navigate this to copyURL."
        self.location_object_navigate(&copy_url, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-assign>
    pub(crate) fn assign_with_origin(
        &self,
        url: &str,
        entry_settings_base_url: &Url,
        entry_settings_origin: &str,
    ) -> Result<(), LocationError> {
        // Step 1: "If this's relevant Document is null, then return."
        if self.relevant_document_is_null() {
            return Ok(());
        }

        // Step 2: "If this's relevant Document's origin is not same origin-domain with the
        // entry settings object's origin, then throw a SecurityError DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 3: "Let urlRecord be the result of encoding-parsing a URL given url, relative to
        // the entry settings object."
        let url_record = self.parse_url_relative_to_entry_settings(url, entry_settings_base_url)?;

        // Step 4: "If urlRecord is failure, then throw a SyntaxError DOMException."
        // Note: parse_url_relative_to_entry_settings maps parse failure to SyntaxError.

        // Step 5: "Location-object navigate this to urlRecord."
        self.location_object_navigate(&url_record, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-replace>
    pub(crate) fn replace_with_origin(
        &self,
        url: &str,
        entry_settings_base_url: &Url,
    ) -> Result<(), LocationError> {
        // Step 1: "If this's relevant Document is null, then return."
        if self.relevant_document_is_null() {
            return Ok(());
        }

        // Step 2: "Let urlRecord be the result of encoding-parsing a URL given url, relative to
        // the entry settings object."
        let url_record = self.parse_url_relative_to_entry_settings(url, entry_settings_base_url)?;

        // Step 3: "If urlRecord is failure, then throw a SyntaxError DOMException."
        // Note: parse_url_relative_to_entry_settings maps parse failure to SyntaxError.

        // Step 4: "Location-object navigate this to urlRecord given 'replace'."
        self.location_object_navigate(&url_record, NavigationHistoryBehavior::Replace)
    }

    /// <https://html.spec.whatwg.org/#dom-location-reload>
    pub(crate) fn reload_with_origin(
        &self,
        entry_settings_origin: &str,
    ) -> Result<(), LocationError> {
        // Step 1: "Let document be this's relevant Document."
        // Note: The model carries relevant document presence as `relevant_document_origin`.

        // Step 2: "If document is null, then return."
        if self.relevant_document_is_null() {
            return Ok(());
        }

        // Step 3: "If document's origin is not same origin-domain with the entry settings
        // object's origin, then throw a SecurityError DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 4: "Reload document's node navigable."
        self.unsupported_navigation(String::from("Location.reload()"))
    }

    /// <https://html.spec.whatwg.org/#dom-location-ancestororigins>
    pub(crate) fn ancestor_origins_with_origin(
        &self,
        entry_settings_origin: &str,
    ) -> Result<Vec<String>, LocationError> {
        // Step 1: "If this's relevant Document is null, then return this's empty DOMStringList."
        if self.relevant_document_is_null() {
            return Ok(Vec::new());
        }

        // Step 2: "If this's relevant Document's origin is not same origin-domain with the
        // entry settings object's origin, then throw a SecurityError DOMException."
        self.ensure_same_origin_domain(entry_settings_origin)?;

        // Step 3: "Assert: this's relevant Document's ancestor origins list is not null."
        // Step 4: "Otherwise, return this's relevant Document's ancestor origins list."
        // Note: The implementation does not yet expose a document ancestor-origins carrier, so
        // this model returns an empty list.
        Ok(Vec::new())
    }

    /// <https://html.spec.whatwg.org/#location-object-navigate>
    fn location_object_navigate(
        &self,
        url: &Url,
        _history_handling: NavigationHistoryBehavior,
    ) -> Result<(), LocationError> {
        // Step 1: "Let navigable be location's relevant global object's navigable."
        // Note: Location's relevant global object is the Window stored as
        // self.window. We reach the GlobalScope through the Window via
        // `downcast_ref` — boa's safe API for accessing native data from
        // a JsObject handle.
        let window = self
            .window
            .downcast_ref::<Window>()
            .ok_or_else(|| {
                LocationError::NotSupported(String::from(
                    "Location window is not a valid Window object",
                ))
            })?;
        let Some(navigable_id) = window.global_scope.source_navigable_id() else {
            return Ok(());
        };
        let Some(event_sender) = window.global_scope.event_sender() else {
            return Err(LocationError::NotSupported(String::from(
                "Location navigation not available: no IPC sender",
            )));
        };

        // Step 2: "Let sourceDocument be the incumbent global object's associated Document."
        // Note: The user agent uses the incumbent global object's document as the source
        // document for referrer policy and other navigation metadata. The content process
        // signals this implicitly by sending a NavigationRequested from the current document,
        // and the user agent derives the source document from the content process identity.

        // Step 3: "If location's relevant Document is not yet completely loaded and the
        // incumbent global object does not have transient activation, then set
        // historyHandling to 'replace'."
        // Note: TODO — Track document completely-loaded state and transient activation for the
        // incumbent global object. Currently the navigation request marks the source
        // navigable, and the user agent can apply 'replace' handling when it knows
        // the document is not yet fully loaded.

        // Step 4: "Navigate navigable to url using sourceDocument, with exceptionsEnabled
        // set to true and historyHandling set to historyHandling."
        super::navigate(
            &event_sender,
            navigable_id,
            Some(navigable_id),
            url.to_string(),
            String::new(),
            UserNavigationInvolvement::None,
            false,
        )
        .map_err(|error| {
            LocationError::NotSupported(format!(
                "failed to send location navigation request: {error}"
            ))
        })
    }

    fn unsupported_navigation(&self, operation: String) -> Result<(), LocationError> {
        Err(LocationError::NotSupported(format!(
            "{operation} is not yet implemented: content process cannot issue Location navigation requests from this path"
        )))
    }

    fn parse_url_relative_to_entry_settings(
        &self,
        value: &str,
        entry_settings_base_url: &Url,
    ) -> Result<Url, LocationError> {
        entry_settings_base_url
            .join(value)
            .map_err(|_| LocationError::Syntax)
    }

    fn relevant_document_is_null(&self) -> bool {
        self.relevant_document_origin.is_none()
    }

    fn ensure_same_origin_domain(&self, entry_settings_origin: &str) -> Result<(), LocationError> {
        let Some(relevant_document_origin) = self.relevant_document_origin.as_deref() else {
            return Ok(());
        };

        // Note: The model approximates same origin-domain by matching serialized origins because
        // document.domain relaxation is not yet represented in this implementation.
        if relevant_document_origin != entry_settings_origin {
            return Err(LocationError::Security);
        }

        Ok(())
    }

    fn basic_url_parse_host_state(&self, copy_url: &mut Url, value: &str) {
        let Some(host_port) = split_host_and_port(value) else {
            return;
        };

        if Host::parse(host_port.host).is_err() {
            return;
        }

        let _ = copy_url.set_host(Some(host_port.host));
        match host_port.port {
            Some(port) => {
                let _ = copy_url.set_port(Some(port));
            }
            None => {}
        }
    }

    fn basic_url_parse_hostname_state(&self, copy_url: &mut Url, value: &str) {
        if Host::parse(value).is_err() {
            return;
        }
        let _ = copy_url.set_host(Some(value));
    }

    fn basic_url_parse_port_state(&self, copy_url: &mut Url, value: &str) {
        let Ok(port) = value.parse::<u16>() else {
            return;
        };
        let _ = copy_url.set_port(Some(port));
    }

    fn basic_url_parse_path_start_state(&self, copy_url: &mut Url, value: &str) {
        let Ok(parsed) = copy_url.join(value) else {
            return;
        };
        copy_url.set_path(parsed.path());
    }

    fn basic_url_parse_query_state(&self, copy_url: &mut Url, input: &str) {
        copy_url.set_query(Some(input));
    }

    fn basic_url_parse_fragment_state(&self, copy_url: &mut Url, input: &str) {
        copy_url.set_fragment(Some(input));
    }
}

struct HostPortInput<'a> {
    host: &'a str,
    port: Option<u16>,
}

fn split_host_and_port(input: &str) -> Option<HostPortInput<'_>> {
    let value = input.trim();
    if value.is_empty() {
        return None;
    }

    if let Some(rest) = value.strip_prefix('[')
        && let Some(end) = rest.find(']')
    {
        let host = &value[..end + 2];
        let remainder = &value[end + 2..];
        if remainder.is_empty() {
            return Some(HostPortInput { host, port: None });
        }
        if let Some(port_str) = remainder.strip_prefix(':') {
            return Some(HostPortInput {
                host,
                port: port_str.parse::<u16>().ok(),
            });
        }
        return None;
    }

    if let Some((host, port_str)) = value.rsplit_once(':')
        && !host.contains(':')
    {
        return Some(HostPortInput {
            host,
            port: port_str.parse::<u16>().ok(),
        });
    }

    Some(HostPortInput {
        host: value,
        port: None,
    })
}
