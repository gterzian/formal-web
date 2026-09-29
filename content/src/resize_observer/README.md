# content/src/resize_observer

Resize Observer (<https://drafts.csswg.org/resize-observer/>). The
processing model runs inside update the rendering in
`content/src/main.rs` after the document's style and layout resolve; box
sizes come from the element's blitz layout box, in the horizontal writing
mode (inline is width, block is height).

## Remaining work

- The resize loop error notification is logged instead of reported as an
  `ErrorEvent`, which does not exist yet.
- SVG elements without a CSS layout box are sized like any other element.
- No upstream test is selected: `resize-observer/observe.html` needs
  `Image`, `notify.html` needs notifications after `removeChild` and
  `appendChild` of an observed target, `calculate-depth-for-node.html`
  needs shadow DOM, and `idlharness.window.js` needs the whole Geometry
  Interfaces IDL.
