//! URL Standard (<https://url.spec.whatwg.org/>): the URL and URLSearchParams
//! interfaces, over the `url` crate's parser and serializer.

pub(crate) mod application_x_www_form_urlencoded;
pub(crate) mod url;
pub(crate) mod url_search_params;

pub(crate) use self::url::URL;
pub(crate) use url_search_params::URLSearchParams;
