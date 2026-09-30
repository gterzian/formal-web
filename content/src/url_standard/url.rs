use std::cell::RefCell;
use std::rc::Rc;

use js_engine::{Completion, ExecutionContext, gc_struct};
use url::{Url, quirks};

use crate::js::Types;

use super::url_search_params::URLSearchParams;

/// <https://url.spec.whatwg.org/#concept-basic-url-parser>
fn basic_url_parser(input: &str, base: Option<&Url>) -> Option<Url> {
    // Note: The `url` crate runs the parser's state machine; a parse error
    // is this algorithm's failure.
    Url::options().base_url(base).parse(input).ok()
}

/// <https://url.spec.whatwg.org/#api-url-parser>
pub(crate) fn api_url_parser(url: &str, base: Option<&str>) -> Option<Url> {
    // Step 1: Let parsedBase be null.
    let mut parsed_base = None;

    // Step 2: If base is non-null:
    if let Some(base) = base {
        // Step 2.1: Set parsedBase to the result of running the basic URL
        // parser on base.
        // Step 2.2: If parsedBase is failure, then return failure.
        parsed_base = Some(basic_url_parser(base, None)?);
    }

    // Step 3: Return the result of running the basic URL parser on url with
    // parsedBase.
    basic_url_parser(url, parsed_base.as_ref())
}

/// <https://url.spec.whatwg.org/#url>
#[gc_struct]
#[allow(clippy::upper_case_acronyms)]
pub(crate) struct URL {
    /// <https://url.spec.whatwg.org/#concept-url-url>
    #[ignore_trace]
    url: Rc<RefCell<Url>>,

    /// <https://url.spec.whatwg.org/#concept-url-query-object>
    query_object: URLSearchParams,
}

impl URL {
    /// <https://url.spec.whatwg.org/#dom-url-url>
    pub(crate) fn constructor(
        url: String,
        base: Option<String>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let parsedURL be the result of running the API URL parser
        // on url with base, if given.
        let parsed_url = api_url_parser(&url, base.as_deref());

        // Step 2: If parsedURL is failure, then throw a TypeError.
        let Some(parsed_url) = parsed_url else {
            return Err(
                ec.new_type_error(&format!("Failed to construct 'URL': Invalid URL: {url}"))
            );
        };

        // Step 3: Let query be parsedURL's query, if that is non-null;
        // otherwise the empty string.
        let query = parsed_url.query().unwrap_or_default().to_owned();

        // Step 4: Initialize this with parsedURL and query.
        Self::initialize(parsed_url, &query, ec)
    }

    /// <https://url.spec.whatwg.org/#url-initialize>
    fn initialize(
        url_record: Url,
        query: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Set url's URL to urlRecord.
        let url = Rc::new(RefCell::new(url_record));

        // Step 2: Set url's query object to a new URLSearchParams object.
        // Step 3: Initialize url's query object with query.
        // Step 4: Set url's query object's URL object to url.
        // Note: The query object holds the URL record it updates (shared
        // with this URL object) in place of the URL object itself.
        let query_object = URLSearchParams::new_query_object(query, Rc::clone(&url), ec)?;
        Ok(Self { url, query_object })
    }

    /// <https://url.spec.whatwg.org/#dom-url-parse>
    pub(crate) fn parse(
        url: String,
        base: Option<String>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<Self>, Types> {
        // Step 1: Let parsedURL be the result of running the API URL parser
        // on url with base, if given.
        let parsed_url = api_url_parser(&url, base.as_deref());

        // Step 2: If parsedURL is failure, then return null.
        let Some(parsed_url) = parsed_url else {
            return Ok(None);
        };

        // Step 3: Let query be parsedURL's query, if that is non-null;
        // otherwise the empty string.
        let query = parsed_url.query().unwrap_or_default().to_owned();

        // Step 4: Let url be a new URL object.
        // Step 5: Initialize url with parsedURL and query.
        let url = Self::initialize(parsed_url, &query, ec)?;

        // Step 6: Return url.
        Ok(Some(url))
    }

    /// <https://url.spec.whatwg.org/#dom-url-canparse>
    pub(crate) fn can_parse(url: String, base: Option<String>) -> bool {
        // Step 1: Let parsedURL be the result of running the API URL parser
        // on url with base, if given.
        let parsed_url = api_url_parser(&url, base.as_deref());

        // Step 2: If parsedURL is failure, then return false.
        // Step 3: Return true.
        parsed_url.is_some()
    }

    /// <https://url.spec.whatwg.org/#dom-url-href>
    pub(crate) fn href(&self) -> String {
        // The href getter steps and the toJSON() method steps are to return
        // the serialization of this's URL.
        quirks::href(&self.url.borrow()).to_owned()
    }

    /// <https://url.spec.whatwg.org/#dom-url-tojson>
    pub(crate) fn to_json(&self) -> String {
        // The href getter steps and the toJSON() method steps are to return
        // the serialization of this's URL.
        self.href()
    }

    /// <https://url.spec.whatwg.org/#dom-url-href>
    pub(crate) fn set_href(
        &self,
        value: String,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Let parsedURL be the result of running the basic URL parser
        // on the given value.
        let parsed_url = basic_url_parser(&value, None);

        // Step 2: If parsedURL is failure, then throw a TypeError.
        let Some(parsed_url) = parsed_url else {
            return Err(ec.new_type_error(&format!("Failed to set 'href': Invalid URL: {value}")));
        };

        // Step 3: Set this's URL to parsedURL.
        *self.url.borrow_mut() = parsed_url;

        // Step 4: Empty this's query object's list.
        self.query_object.empty_list();

        // Step 5: Let query be this's URL's query.
        let query = self.url.borrow().query().map(str::to_owned);

        // Step 6: If query is non-null, then set this's query object's list
        // to the result of parsing query.
        if let Some(query) = query {
            self.query_object.set_list_from_parsing(&query);
        }
        Ok(())
    }

    /// <https://url.spec.whatwg.org/#dom-url-origin>
    pub(crate) fn origin(&self) -> String {
        // The origin getter steps are to return the serialization of this's
        // URL's origin.
        quirks::origin(&self.url.borrow())
    }

    /// <https://url.spec.whatwg.org/#dom-url-protocol>
    pub(crate) fn protocol(&self) -> String {
        // The protocol getter steps are to return this's URL's scheme,
        // followed by U+003A (:).
        quirks::protocol(&self.url.borrow()).to_owned()
    }

    /// <https://url.spec.whatwg.org/#dom-url-protocol>
    pub(crate) fn set_protocol(&self, value: String) {
        // The protocol setter steps are to basic URL parse the given value,
        // followed by U+003A (:), with this's URL as url and scheme start
        // state as state override.
        if quirks::set_protocol(&mut self.url.borrow_mut(), &value).is_err() {
            // The parser returned failure with the URL unchanged.
        }
    }

    /// <https://url.spec.whatwg.org/#dom-url-username>
    pub(crate) fn username(&self) -> String {
        // The username getter steps are to return this's URL's username.
        quirks::username(&self.url.borrow()).to_owned()
    }

    /// <https://url.spec.whatwg.org/#dom-url-username>
    pub(crate) fn set_username(&self, value: String) {
        // Step 1: If this's URL cannot have a username/password/port, then
        // return.
        // Step 2: Set the username given this's URL and the given value.
        if quirks::set_username(&mut self.url.borrow_mut(), &value).is_err() {
            // The URL cannot have a username: it is unchanged.
        }
    }

    /// <https://url.spec.whatwg.org/#dom-url-password>
    pub(crate) fn password(&self) -> String {
        // The password getter steps are to return this's URL's password.
        quirks::password(&self.url.borrow()).to_owned()
    }

    /// <https://url.spec.whatwg.org/#dom-url-password>
    pub(crate) fn set_password(&self, value: String) {
        // Step 1: If this's URL cannot have a username/password/port, then
        // return.
        // Step 2: Set the password given this's URL and the given value.
        if quirks::set_password(&mut self.url.borrow_mut(), &value).is_err() {
            // The URL cannot have a password: it is unchanged.
        }
    }

    /// <https://url.spec.whatwg.org/#dom-url-host>
    pub(crate) fn host(&self) -> String {
        // Step 1: Let url be this's URL.
        // Step 2: If url's host is null, then return the empty string.
        // Step 3: If url's port is null, return url's host, serialized.
        // Step 4: Return url's host, serialized, followed by U+003A (:) and
        // url's port, serialized.
        quirks::host(&self.url.borrow()).to_owned()
    }

    /// <https://url.spec.whatwg.org/#dom-url-host>
    pub(crate) fn set_host(&self, value: String) {
        // Step 1: If this's URL has an opaque path, then return.
        // Step 2: Basic URL parse the given value with this's URL as url and
        // host state as state override.
        if quirks::set_host(&mut self.url.borrow_mut(), &value).is_err() {
            // The parser returned failure with the URL unchanged.
        }
    }

    /// <https://url.spec.whatwg.org/#dom-url-hostname>
    pub(crate) fn hostname(&self) -> String {
        // Step 1: If this's URL's host is null, then return the empty string.
        // Step 2: Return this's URL's host, serialized.
        quirks::hostname(&self.url.borrow()).to_owned()
    }

    /// <https://url.spec.whatwg.org/#dom-url-hostname>
    pub(crate) fn set_hostname(&self, value: String) {
        // Step 1: If this's URL has an opaque path, then return.
        // Step 2: Basic URL parse the given value with this's URL as url and
        // hostname state as state override.
        if quirks::set_hostname(&mut self.url.borrow_mut(), &value).is_err() {
            // The parser returned failure with the URL unchanged.
        }
    }

    /// <https://url.spec.whatwg.org/#dom-url-port>
    pub(crate) fn port(&self) -> String {
        // Step 1: If this's URL's port is null, then return the empty string.
        // Step 2: Return this's URL's port, serialized.
        quirks::port(&self.url.borrow()).to_owned()
    }

    /// <https://url.spec.whatwg.org/#dom-url-port>
    pub(crate) fn set_port(&self, value: String) {
        // Step 1: If this's URL cannot have a username/password/port, then
        // return.
        // Step 2: If the given value is the empty string, then set this's
        // URL's port to null.
        // Step 3: Otherwise, basic URL parse the given value with this's URL
        // as url and port state as state override.
        if quirks::set_port(&mut self.url.borrow_mut(), &value).is_err() {
            // The parser returned failure with the URL unchanged.
        }
    }

    /// <https://url.spec.whatwg.org/#dom-url-pathname>
    pub(crate) fn pathname(&self) -> String {
        // The pathname getter steps are to return the result of URL path
        // serializing this's URL.
        quirks::pathname(&self.url.borrow()).to_owned()
    }

    /// <https://url.spec.whatwg.org/#dom-url-pathname>
    pub(crate) fn set_pathname(&self, value: String) {
        // Step 1: If this's URL has an opaque path, then return.
        // Step 2: Empty this's URL's path.
        // Step 3: Basic URL parse the given value with this's URL as url and
        // path start state as state override.
        quirks::set_pathname(&mut self.url.borrow_mut(), &value);
    }

    /// <https://url.spec.whatwg.org/#dom-url-search>
    pub(crate) fn search(&self) -> String {
        // Step 1: If this's URL's query is either null or the empty string,
        // then return the empty string.
        // Step 2: Return U+003F (?), followed by this's URL's query.
        quirks::search(&self.url.borrow()).to_owned()
    }

    /// <https://url.spec.whatwg.org/#dom-url-search>
    pub(crate) fn set_search(&self, value: String) {
        // Step 1: Let url be this's URL.
        // Step 2: If the given value is the empty string:
        if value.is_empty() {
            // Step 2.1: Set url's query to null.
            // Step 2.3: Potentially strip trailing spaces from an opaque
            // path with this.
            quirks::set_search(&mut self.url.borrow_mut(), "");

            // Step 2.2: Empty this's query object's list.
            self.query_object.empty_list();

            // Step 2.4: Return.
            return;
        }

        // Step 3: Let input be the given value with a single leading U+003F
        // (?) removed, if any.
        let input = value.strip_prefix('?').unwrap_or(&value);

        // Step 4: Set url's query to the empty string.
        // Step 5: Basic URL parse input with url as url and query state as
        // state override.
        quirks::set_search(&mut self.url.borrow_mut(), input);

        // Step 6: Set this's query object's list to the result of parsing
        // input.
        self.query_object.set_list_from_parsing(input);
    }

    /// <https://url.spec.whatwg.org/#dom-url-searchparams>
    pub(crate) fn search_params(&self) -> URLSearchParams {
        // The searchParams getter steps are to return this's query object.
        self.query_object.clone()
    }

    /// <https://url.spec.whatwg.org/#dom-url-hash>
    pub(crate) fn hash(&self) -> String {
        // Step 1: If this's URL's fragment is either null or the empty
        // string, then return the empty string.
        // Step 2: Return U+0023 (#), followed by this's URL's fragment.
        quirks::hash(&self.url.borrow()).to_owned()
    }

    /// <https://url.spec.whatwg.org/#dom-url-hash>
    pub(crate) fn set_hash(&self, value: String) {
        // Step 1: If the given value is the empty string:
        // Step 1.1: Set this's URL's fragment to null.
        // Step 1.2: Potentially strip trailing spaces from an opaque path
        // with this.
        // Step 1.3: Return.
        // Step 2: Let input be the given value with a single leading U+0023
        // (#) removed, if any.
        // Step 3: Set this's URL's fragment to the empty string.
        // Step 4: Basic URL parse input with this's URL as url and fragment
        // state as state override.
        quirks::set_hash(&mut self.url.borrow_mut(), &value);
    }
}
