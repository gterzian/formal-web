use url::{Url, quirks};

/// <https://html.spec.whatwg.org/#hyperlinkelementutils>
pub(crate) trait HyperlinkElementUtils {
    /// <https://html.spec.whatwg.org/#concept-hyperlink-url-set>
    fn set_the_url(&self, document_creation_url: &Url) -> Option<Url>;

    /// <https://html.spec.whatwg.org/#update-href>
    fn update_href(&self, url: &Url);

    /// <https://html.spec.whatwg.org/#reinitialise-url>
    fn reinitialize_url(&self, document_creation_url: &Url) -> Option<Url> {
        // Step 1: "If the element's url is non-null, its scheme is \"blob\", and it has an opaque path, then terminate these steps."
        // Note: The current runtime does not persist the associated hyperlink URL between calls, so there is no cached blob URL instance to preserve here.

        // Step 2: "Set the url."
        self.set_the_url(document_creation_url)
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-origin>
    fn origin(&self, document_creation_url: &Url) -> String {
        // Step 1: "Reinitialize url."
        let Some(url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "If this's url is null, return the empty string."
            return String::new();
        };

        // Step 3: "Return the serialization of this's url's origin."
        quirks::origin(&url)
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-protocol>
    fn protocol(&self, document_creation_url: &Url) -> String {
        // Step 1: "Reinitialize url."
        let Some(url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "If this's url is null, return ':'."
            return String::from(":");
        };

        // Step 3: "Return this's url's scheme, followed by ':'."
        quirks::protocol(&url).to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-protocol>
    fn set_protocol(&self, document_creation_url: &Url, value: &str) {
        // Step 1: "Reinitialize url."
        let Some(mut url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "If this's url is null, then return."
            return;
        };

        // Step 3: "Basic URL parse the given value, followed by ':', with this's url as url and scheme start state as state override."
        if quirks::set_protocol(&mut url, value).is_err() {
            return;
        }

        // Step 4: "Update href."
        self.update_href(&url);
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-username>
    fn username(&self, document_creation_url: &Url) -> String {
        // Step 1: "Reinitialize url."
        let Some(url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "If this's url is null, return the empty string."
            return String::new();
        };

        // Step 3: "Return this's url's username."
        quirks::username(&url).to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-username>
    fn set_username(&self, document_creation_url: &Url, value: &str) {
        // Step 1: "Reinitialize url."
        let Some(mut url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url is null or url cannot have a username/password/port, then return."
            return;
        };

        // Step 4: "Set the username, given url and the given value."
        if quirks::set_username(&mut url, value).is_err() {
            return;
        }

        // Step 5: "Update href."
        self.update_href(&url);
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-password>
    fn password(&self, document_creation_url: &Url) -> String {
        // Step 1: "Reinitialize url."
        let Some(url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url is null, then return the empty string."
            return String::new();
        };

        // Step 4: "Return url's password."
        quirks::password(&url).to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-password>
    fn set_password(&self, document_creation_url: &Url, value: &str) {
        // Step 1: "Reinitialize url."
        let Some(mut url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url is null or url cannot have a username/password/port, then return."
            return;
        };

        // Step 4: "Set the password, given url and the given value."
        if quirks::set_password(&mut url, value).is_err() {
            return;
        }

        // Step 5: "Update href."
        self.update_href(&url);
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-host>
    fn host(&self, document_creation_url: &Url) -> String {
        // Step 1: "Reinitialize url."
        let Some(url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url or url's host is null, return the empty string."
            return String::new();
        };

        if url.host().is_none() {
            // Step 3: "If url or url's host is null, return the empty string."
            return String::new();
        }

        if url.port().is_none() {
            // Step 4: "If url's port is null, return url's host, serialized."
            return quirks::hostname(&url).to_owned();
        }

        // Step 5: "Return url's host, serialized, followed by ':' and url's port, serialized."
        quirks::host(&url).to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-host>
    fn set_host(&self, document_creation_url: &Url, value: &str) {
        // Step 1: "Reinitialize url."
        let Some(mut url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url is null or url has an opaque path, then return."
            return;
        };

        // Step 4: "Basic URL parse the given value, with url as url and host state as state override."
        if quirks::set_host(&mut url, value).is_err() {
            return;
        }

        // Step 5: "Update href."
        self.update_href(&url);
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-hostname>
    fn hostname(&self, document_creation_url: &Url) -> String {
        // Step 1: "Reinitialize url."
        let Some(url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url or url's host is null, return the empty string."
            return String::new();
        };

        if url.host().is_none() {
            // Step 3: "If url or url's host is null, return the empty string."
            return String::new();
        }

        // Step 4: "Return url's host, serialized."
        quirks::hostname(&url).to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-hostname>
    fn set_hostname(&self, document_creation_url: &Url, value: &str) {
        // Step 1: "Reinitialize url."
        let Some(mut url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url is null or url has an opaque path, then return."
            return;
        };

        // Step 4: "Basic URL parse the given value, with url as url and hostname state as state override."
        if quirks::set_hostname(&mut url, value).is_err() {
            return;
        }

        // Step 5: "Update href."
        self.update_href(&url);
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-port>
    fn port(&self, document_creation_url: &Url) -> String {
        // Step 1: "Reinitialize url."
        let Some(url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url or url's port is null, return the empty string."
            return String::new();
        };

        if url.port().is_none() {
            // Step 3: "If url or url's port is null, return the empty string."
            return String::new();
        }

        // Step 4: "Return url's port, serialized."
        quirks::port(&url).to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-port>
    fn set_port(&self, document_creation_url: &Url, value: &str) {
        // Step 1: "Reinitialize url."
        let Some(mut url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url is null or url cannot have a username/password/port, then return."
            return;
        };

        // Step 4: "If the given value is the empty string, then set url's port to null."
        // Step 5: "Otherwise, basic URL parse the given value, with url as url and port state as state override."
        if quirks::set_port(&mut url, value).is_err() {
            return;
        }

        // Step 6: "Update href."
        self.update_href(&url);
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-pathname>
    fn pathname(&self, document_creation_url: &Url) -> String {
        // Step 1: "Reinitialize url."
        let Some(url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url is null, then return the empty string."
            return String::new();
        };

        // Step 4: "Return the result of URL path serializing url."
        quirks::pathname(&url).to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-pathname>
    fn set_pathname(&self, document_creation_url: &Url, value: &str) {
        // Step 1: "Reinitialize url."
        let Some(mut url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url is null or url has an opaque path, then return."
            return;
        };

        // Step 4: "Set url's path to the empty list."
        // Step 5: "Basic URL parse the given value, with url as url and path start state as state override."
        quirks::set_pathname(&mut url, value);

        // Step 6: "Update href."
        self.update_href(&url);
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-search>
    fn search(&self, document_creation_url: &Url) -> String {
        // Step 1: "Reinitialize url."
        let Some(url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url is null, or url's query is either null or the empty string, return the empty string."
            return String::new();
        };

        let search = quirks::search(&url);
        if search.is_empty() {
            // Step 3: "If url is null, or url's query is either null or the empty string, return the empty string."
            return String::new();
        }

        // Step 4: "Return '?', followed by url's query."
        search.to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-search>
    fn set_search(&self, document_creation_url: &Url, value: &str) {
        // Step 1: "Reinitialize url."
        let Some(mut url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url is null, terminate these steps."
            return;
        };

        // Step 4: "If the given value is the empty string, set url's query to null."
        // Step 5: "Otherwise" "Let input be the given value with a single leading '?' removed, if any." "Set url's query to the empty string." "Basic URL parse input, with url as url and query state as state override."
        quirks::set_search(&mut url, value);

        // Step 6: "Update href."
        self.update_href(&url);
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-hash>
    fn hash(&self, document_creation_url: &Url) -> String {
        // Step 1: "Reinitialize url."
        let Some(url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url is null, or url's fragment is either null or the empty string, return the empty string."
            return String::new();
        };

        let hash = quirks::hash(&url);
        if hash.is_empty() {
            // Step 3: "If url is null, or url's fragment is either null or the empty string, return the empty string."
            return String::new();
        }

        // Step 4: "Return '#', followed by url's fragment."
        hash.to_owned()
    }

    /// <https://html.spec.whatwg.org/#dom-hyperlink-hash>
    fn set_hash(&self, document_creation_url: &Url, value: &str) {
        // Step 1: "Reinitialize url."
        let Some(mut url) = self.reinitialize_url(document_creation_url) else {
            // Step 2: "Let url be this's url."
            // Step 3: "If url is null, then return."
            return;
        };

        // Step 4: "If the given value is the empty string, set url's fragment to null."
        // Step 5: "Otherwise" "Let input be the given value with a single leading '#' removed, if any." "Set url's fragment to the empty string." "Basic URL parse input, with url as url and fragment state as state override."
        quirks::set_hash(&mut url, value);

        // Step 6: "Update href."
        self.update_href(&url);
    }
}
