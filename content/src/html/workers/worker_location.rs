use js_engine::gc_struct;
use url::Url;

/// <https://html.spec.whatwg.org/#workerlocation>
#[gc_struct]
pub(crate) struct WorkerLocation {
    /// <https://html.spec.whatwg.org/#concept-url>
    #[ignore_trace]
    url: Url,
}

impl WorkerLocation {
    pub(crate) fn new(url: Url) -> Self {
        Self { url }
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-href>
    pub(crate) fn href(&self) -> String {
        // The href getter steps are to return this's WorkerGlobalScope
        // object's url, serialized.
        self.url.to_string()
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-origin>
    pub(crate) fn origin(&self) -> String {
        // The origin getter steps are to return the serialization of this's
        // WorkerGlobalScope object's url's origin.
        self.url.origin().unicode_serialization()
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-protocol>
    pub(crate) fn protocol(&self) -> String {
        // The protocol getter steps are to return this's WorkerGlobalScope
        // object's url's scheme, followed by ":".
        format!("{}:", self.url.scheme())
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-host>
    pub(crate) fn host(&self) -> String {
        // Step 1: Let url be this's WorkerGlobalScope object's url.
        // Step 2: If url's host is null, return the empty string.
        let Some(host) = self.url.host_str() else {
            return String::new();
        };
        // Step 3: If url's port is null, return url's host, serialized.
        // Step 4: Return url's host, serialized, followed by ":" and url's
        //         port, serialized.
        match self.url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-hostname>
    pub(crate) fn hostname(&self) -> String {
        // Step 1: Let host be this's WorkerGlobalScope object's url's host.
        // Step 2: If host is null, return the empty string.
        // Step 3: Return host, serialized.
        self.url.host_str().unwrap_or("").to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-port>
    pub(crate) fn port(&self) -> String {
        // Step 1: Let port be this's WorkerGlobalScope object's url's port.
        // Step 2: If port is null, return the empty string.
        // Step 3: Return port, serialized.
        match self.url.port() {
            Some(port) => port.to_string(),
            None => String::new(),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-pathname>
    pub(crate) fn pathname(&self) -> String {
        // The pathname getter steps are to return the result of URL path
        // serializing this's WorkerGlobalScope object's url.
        self.url.path().to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-search>
    pub(crate) fn search(&self) -> String {
        // Step 1: Let query be this's WorkerGlobalScope object's url's query.
        // Step 2: If query is either null or the empty string, return the
        //         empty string.
        // Step 3: Return "?", followed by query.
        match self.url.query() {
            Some(query) if !query.is_empty() => format!("?{query}"),
            _ => String::new(),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-workerlocation-hash>
    pub(crate) fn hash(&self) -> String {
        // Step 1: Let fragment be this's WorkerGlobalScope object's url's
        //         fragment.
        // Step 2: If fragment is either null or the empty string, return the
        //         empty string.
        // Step 3: Return "#", followed by fragment.
        match self.url.fragment() {
            Some(fragment) if !fragment.is_empty() => format!("#{fragment}"),
            _ => String::new(),
        }
    }
}
