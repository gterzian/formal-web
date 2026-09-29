# content/src/cssom_view

CSSOM View (<https://drafts.csswg.org/cssom-view/>). `matchMedia` parses
and evaluates its query with stylo against the blitz document's style
device, so a query is supported here exactly when stylo's media query
parser accepts it.

## Remaining work

- "Evaluate media queries and report changes" does not run in update the
  rendering, so a `MediaQueryList` never fires `change`; `matches` is
  evaluated live on each read instead.
- `MediaQueryListEvent` does not exist.
