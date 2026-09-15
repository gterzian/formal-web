//! The HTML spec's "Common microsyntaxes"
//! (<https://html.spec.whatwg.org/#common-microsyntaxes>): the micro-parsers
//! for the data types HTML content attributes accept.  Each spec subsection
//! gets its own module, and each parser is a free function named for the spec
//! algorithm it implements.

pub(crate) mod non_negative_integers;
pub(crate) mod signed_integers;
