use boa_engine::JsData;
use boa_gc::{Finalize, Trace};
use url::Url;

/// <https://html.spec.whatwg.org/#location>
#[derive(Trace, Finalize, JsData)]
pub struct Location {
    /// Model-local backing URL used for Location attribute serialization and URL parsing.
    ///
    /// Note: The spec defines Location.url in terms of the relevant Document URL. This runtime
    /// currently snapshots that URL when creating the Location object.
    #[unsafe_ignore_trace]
    url: Url,
}

/// <https://html.spec.whatwg.org/#navigationhistorybehavior>
enum NavigationHistoryBehavior {
    Auto,
    Replace,
}

impl Location {
    pub(crate) fn new(url: Url) -> Self {
        Self { url }
    }

    /// <https://html.spec.whatwg.org/#dom-location-href>
    pub(crate) fn href(&self) -> String {
        // Step 2: "Return this's url, serialized."
        self.url.as_str().to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-location-origin>
    pub(crate) fn origin(&self) -> String {
        // Step 2: "Return the serialization of this's url's origin."
        self.url.origin().unicode_serialization()
    }

    /// <https://html.spec.whatwg.org/#dom-location-protocol>
    pub(crate) fn protocol(&self) -> String {
        // Step 2: "Return this's url's scheme, followed by ':'."
        format!("{}:", self.url.scheme())
    }

    /// <https://html.spec.whatwg.org/#dom-location-host>
    pub(crate) fn host(&self) -> String {
        // Step 2: "Return this's url's host and port (if different from the default port for the
        // scheme)."
        let hostname = self.url.host_str().unwrap_or_default();
        match self.url.port() {
            Some(port) if !hostname.is_empty() => format!("{hostname}:{port}"),
            _ => hostname.to_owned(),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-location-hostname>
    pub(crate) fn hostname(&self) -> String {
        // Step 2: "Return this's url's host."
        self.url.host_str().unwrap_or_default().to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-location-port>
    pub(crate) fn port(&self) -> String {
        // Step 2: "Return this's url's port."
        self.url
            .port()
            .map(|value| value.to_string())
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-location-pathname>
    pub(crate) fn pathname(&self) -> String {
        // Step 2: "Return this's url's path."
        self.url.path().to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-location-search>
    pub(crate) fn search(&self) -> String {
        // Step 2: "Return this's url's query (includes leading '?' if non-empty)."
        self.url
            .query()
            .map(|value| format!("?{value}"))
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-location-hash>
    pub(crate) fn hash(&self) -> String {
        // Step 2: "Return this's url's fragment (includes leading '#' if non-empty)."
        self.url
            .fragment()
            .map(|value| format!("#{value}"))
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-location-href>
    pub(crate) fn set_href(&self, value: &str) -> Result<(), String> {
        // Step 2: "Let url be the result of encoding-parsing a URL given the given value,
        // relative to the entry settings object."
        let url = self
            .url
            .join(value)
            .map_err(|_| String::from("failed to parse Location href"))?;

        // Step 4: "Location-object navigate this to url."
        self.location_object_navigate(&url, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-protocol>
    pub(crate) fn set_protocol(&self, value: &str) -> Result<(), String> {
        // Step 3: "Let copyURL be a copy of this's url."
        let mut copy_url = self.url.clone();

        // Step 4: "Let possibleFailure be the result of basic URL parsing ... with scheme start
        // state as state override."
        let scheme = value.trim_end_matches(':');
        if scheme.is_empty() || copy_url.set_scheme(scheme).is_err() {
            // Step 5: "If possibleFailure is failure, then throw a SyntaxError DOMException."
            return Err(String::from("failed to parse Location protocol"));
        }

        // Step 6: "If copyURL's scheme is not an HTTP(S) scheme, then terminate these steps."
        if !matches!(copy_url.scheme(), "http" | "https") {
            return Ok(());
        }

        // Step 7: "Location-object navigate this to copyURL."
        self.location_object_navigate(&copy_url, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-host>
    pub(crate) fn set_host(&self, _value: &str) -> Result<(), String> {
        // Step 4: "Location-object navigate this to copyURL."
        self.unsupported_navigation(String::from("Location.host setter"))
    }

    /// <https://html.spec.whatwg.org/#dom-location-hostname>
    pub(crate) fn set_hostname(&self, _value: &str) -> Result<(), String> {
        // Step 4: "Location-object navigate this to copyURL."
        self.unsupported_navigation(String::from("Location.hostname setter"))
    }

    /// <https://html.spec.whatwg.org/#dom-location-port>
    pub(crate) fn set_port(&self, _value: &str) -> Result<(), String> {
        // Step 4: "Location-object navigate this to copyURL."
        self.unsupported_navigation(String::from("Location.port setter"))
    }

    /// <https://html.spec.whatwg.org/#dom-location-pathname>
    pub(crate) fn set_pathname(&self, _value: &str) -> Result<(), String> {
        // Step 4: "Location-object navigate this to copyURL."
        self.unsupported_navigation(String::from("Location.pathname setter"))
    }

    /// <https://html.spec.whatwg.org/#dom-location-search>
    pub(crate) fn set_search(&self, _value: &str) -> Result<(), String> {
        // Step 4: "Location-object navigate this to copyURL."
        self.unsupported_navigation(String::from("Location.search setter"))
    }

    /// <https://html.spec.whatwg.org/#dom-location-hash>
    pub(crate) fn set_hash(&self, _value: &str) -> Result<(), String> {
        // Step 7: "Location-object navigate this to copyURL."
        self.unsupported_navigation(String::from("Location.hash setter"))
    }

    /// <https://html.spec.whatwg.org/#dom-location-assign>
    pub(crate) fn assign(&self, url: &str) -> Result<(), String> {
        // Step 1: "Let parsedURL be the result of encoding-parsing a URL given url, relative to
        // the entry settings object."
        let parsed_url = self
            .url
            .join(url)
            .map_err(|_| String::from("failed to parse Location.assign() URL"))?;

        // Step 3: "Location-object navigate this to parsedURL."
        self.location_object_navigate(&parsed_url, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-replace>
    pub(crate) fn replace(&self, url: &str) -> Result<(), String> {
        // Step 1: "Let parsedURL be the result of encoding-parsing a URL given url, relative to
        // the entry settings object."
        let parsed_url = self
            .url
            .join(url)
            .map_err(|_| String::from("failed to parse Location.replace() URL"))?;

        // Step 3: "Location-object navigate this to parsedURL with historyHandling set to
        // 'replace'."
        self.location_object_navigate(&parsed_url, NavigationHistoryBehavior::Replace)
    }

    /// <https://html.spec.whatwg.org/#dom-location-reload>
    pub(crate) fn reload(&self) -> Result<(), String> {
        // Step 1: "Location-object navigate this to this's url."
        self.location_object_navigate(&self.url, NavigationHistoryBehavior::Auto)
    }

    /// <https://html.spec.whatwg.org/#dom-location-ancestororigins>
    pub(crate) fn ancestor_origins(&self) -> Vec<String> {
        // Step 1: "Return a DOMStringList object listing the origins of the ancestor navigables'
        // active documents."
        // Note: The current runtime does not yet expose ancestor navigable chains here.
        Vec::new()
    }

    /// <https://html.spec.whatwg.org/#location-object-navigate>
    fn location_object_navigate(
        &self,
        _url: &Url,
        history_handling: NavigationHistoryBehavior,
    ) -> Result<(), String> {
        // Step 4: "Navigate navigable to url using sourceDocument ... with historyHandling set to
        // historyHandling."
        let history_handling = match history_handling {
            NavigationHistoryBehavior::Auto => "auto",
            NavigationHistoryBehavior::Replace => "replace",
        };
        self.unsupported_navigation(format!(
            "Location navigation with history handling '{history_handling}'"
        ))
    }

    fn unsupported_navigation(&self, operation: String) -> Result<(), String> {
        Err(format!(
            "{operation} is not yet implemented: content runtime cannot issue Location navigation requests from this path"
        ))
    }
}
