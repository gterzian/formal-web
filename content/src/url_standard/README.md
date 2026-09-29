# content/src/url_standard

The `url` crate is the basic URL parser and serializer, and its `quirks`
module the setters' state-override parses; the URL and URLSearchParams
algorithms sit on top and carry the spec steps.

## Remaining work

- `url` 2.5.8 strips the trailing spaces of an opaque path when its query
  becomes null; the current URL Standard keeps them and serializes the last
  one as `%20`. Two `urlsearchparams-delete.any.js` subtests fail on it
  (`tests/wpt/meta/url/`).
- `LegacyWindowAlias=webkitURL` is not exposed.
