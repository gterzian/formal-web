# CSS WPT Test Status

*Last updated: 2026-06-06*

This document summarises which CSS Web Platform Tests (WPT) can currently pass under
formal-web and which missing web APIs prevent the rest from running.

## Test Methodology

CSS test subdirectories under `vendor/wpt/css/` were selected for manual test runs based
on size (smaller directories first) and type (preferring directories with testharness.js
tests over reftest-heavy ones).  The results below cover:

| Batch | Directories tested | Testharness tests |
|-------|-------------------|-------------------|
| 1     | css-content, css-color-hdr, css-device-adapt, css-exclusions, css-forced-color-adjust, css-forms, css-image-animation, css-paint-api, css-parser-api, css-size-adjust, css-will-change | ~56 |
| 2     | css-color-adjust, css-easing, css-env, css-highlight-api, css-layout-api, css-overscroll-behavior, css-page, css-rhythm, css-ruby, css-scrollbars | ~152 |
| 3     | css-display, css-syntax, cssom | ~255 |
| Spot   | css-values, css-backgrounds, css-color, css-fonts, css-lists, css-text, css-ui, css-position, css-animations, css-cascade, css-variables | ~25 |

A test file is considered "passing" when all its sub-tests report PASS and no ERROR,
TIMEOUT, or FAIL occurs at the file level.

## Overall Pass Rate

| Metric | Count |
|--------|-------|
| Testable CSS testharness files across `vendor/wpt/css/` | ~5,900 |
| Test files that pass today | ~38  |
| **Pass rate** | **~0.6 %** |

Of the ~38 passing files, most are `*-invalid.html` parsing tests that verify that
garbage CSS values are correctly rejected.  A handful are basic structural tests
(tokenizer whitespace, SVG display handling, CSSOM mutation) that do not depend on
specific CSS property support.

## Tests That Pass

### Invalid-value parsing (`*-invalid.html`)

These tests verify that the CSS parser correctly rejects invalid property values.
They pass because they never call `CSS.supports()` or check computed style — they
only set invalid values on `e.style` and verify the property was not set.

| Test file |
|-----------|
| `css/css-content/parsing/content-invalid.html` |
| `css/css-color/parsing/color-invalid.html` |
| `css/css-color/parsing/color-computed.html` *(no, this failed)* |
| `css/css-color-adjust/parsing/color-scheme-invalid.html` |
| `css/css-easing/timing-functions-syntax-invalid.html` |
| `css/css-forced-color-adjust/parsing/forced-color-adjust-invalid.html` |
| `css/css-overscroll-behavior/parsing/overscroll-behavior-invalid.html` |
| `css/css-page/parsing/page-invalid.html` |
| `css/css-page/parsing/page-orientation-invalid.tentative.html` |
| `css/css-page/parsing/size-invalid.html` |
| `css/css-rhythm/parsing/block-step-align-invalid.html` |
| `css/css-rhythm/parsing/block-step-insert-invalid.html` |
| `css/css-rhythm/parsing/block-step-invalid.html` |
| `css/css-rhythm/parsing/block-step-round-invalid.html` |
| `css/css-rhythm/parsing/block-step-size-invalid.html` |
| `css/css-ruby/parsing/ruby-align-invalid.html` |
| `css/css-ruby/parsing/ruby-merge-invalid.html` |
| `css/css-ruby/parsing/ruby-overhang-invalid.html` |
| `css/css-ruby/parsing/ruby-position-invalid.html` |
| `css/css-size-adjust/parsing/text-size-adjust-invalid.html` |
| `css/css-will-change/parsing/will-change-invalid.html` |
| `css/css-display/parsing/display-invalid.html` |
| `css/css-display/reading-flow/tentative/reading-flow-invalid.html` |
| `css/css-display/reading-flow/tentative/reading-order-invalid.html` |

### Basic structural / non-property tests

| Test file | What it tests |
|-----------|---------------|
| `css/css-syntax/whitespace.html` | CSS tokenizer whitespace handling |
| `css/css-syntax/unclosed-constructs.html` | Tokenizer unclosed-string/unclosed-url recovery |
| `css/css-display/display-with-float.html` | Float + display interaction |
| `css/css-display/display-contents-svg-anchor-child.html` | SVG `<a>` child with `display: contents` |
| `css/css-display/display-contents-svg-switch-child.html` | SVG `<switch>` child with `display: contents` |
| `css/css-ruby/ruby-position.html` | Ruby position rendering |
| `css/css-ruby/ruby-overhang-no-overlap.html` | Ruby overhang layout |
| `css/css-highlight-api/historical.window.js` | Historical API removal check (no feature needed) |
| `css/cssom/css-style-attribute-modifications.html` | Style attribute mutation tracking |
| `css/cssom/cssom-cssstyledeclaration-set.html` | `CSSStyleDeclaration.setProperty()` basic round-trip |
| `css/cssom/getComputedStyle-width-scroll.tentative.html` | `getComputedStyle()` for width with scrollbar |
| `css/css-values/absolute-length-units-001.html` | Absolute length unit parsing (`px`, `cm`, `mm`, etc.) |
| `css/css-scrollbars/scrollbar-color-001.html` | Basic `scrollbar-color` property acceptance |
| `css/css-scrollbars/scrollbar-width-007.html` | Basic `scrollbar-width` property acceptance |
| `css/css-scrollbars/scrollbar-width-010.html` | Basic `scrollbar-width` property acceptance |
| `css/css-scrollbars/scrollbar-width-012.html` | Basic `scrollbar-width` property acceptance |
| `css/css-scrollbars/scrollbar-width-014.html` | Basic `scrollbar-width` property acceptance |

## Missing Web APIs Causing Failures

The failures fall into a few recurring categories.  Listed in approximate order of
impact (most test-blocking first):

### 1. CSS property registration / `CSS.supports()` always returns `false`

**Impact:** Blocks the vast majority of CSS property tests.

The test helper `property_supported_in_computed_style()` (used by every
`*-computed.html` test) calls `CSS.supports(propertyName, initialValue)`.  Since
`CSS.supports()` is not wired to the actual property database, **every property
test** — including very basic ones like `position`, `color`, `font-family`,
`background-color`, `background-attachment`, `animation-name`,
`animation-timing-function`, `text-transform`, `letter-spacing`, `list-style-type`,
etc. — reports the property as unsupported and fails.

**What's needed:**
- Implement `CSS.supports(property, value)` by consulting the CSS property
  definition table.
- Alternatively, change the test infrastructure to not gate on `CSS.supports()`,
  though that would deviate from WPT expectations.

### 2. `CSS` namespace object missing / incomplete

**Impact:** Blocks CSSOM and CSS-typed-OM tests.

The `CSS` global namespace object (`CSS.supports()`, `CSS.escape()`,
`CSS.paintWorklet`, `CSS.layoutWorklet`) is referenced by many tests.  `CSS.escape()`
is implemented; `CSS.supports()` and the worklet registries are not.

**What's needed:**
- `CSS.supports(conditionText)` / `CSS.supports(property, value)`

### 3. `getComputedStyle()` property-value round-trip incomplete

**Impact:** Every `*-computed.html` and `*-valid.html` test fails.

When a property value is set via `e.style.property = "value"`, reading it back via
`e.style.property` or `getComputedStyle(e).property` returns empty string instead of
the canonicalised value.  This causes all `*-valid.html` tests (which set a valid
value and assert it was accepted) to fail.

**What's needed:**
- Wire property assignment to return the accepted/canonicalised value.
- Implement `getComputedStyle()` property value resolution for all CSS properties.

### 4. Reftest support not implemented

**Impact:** Blocks ~8,900 reftest files across `vendor/wpt/css/`.

The WPT runner skips files containing `rel="match"` or `rel="mismatch"`.  Many CSS
suites (css-backgrounds, css-flexbox, css-grid, css-text, css-transforms, etc.) are
predominantly reftests.

**What's needed:**
- Pixel-level screenshot comparison in the WPT runner (beyond current
  screenshot-on-shutdown capability).

### 5. `fetch()` not implemented

**Impact:** Blocks all `idlharness.html` and `idlharness.window.js` tests.

IDL harness tests load WebIDL fragments via `fetch()`.  Without `fetch`, the
IDL blocks never load and the test framework reports setup failure.

**What's needed:**
- Implement the `fetch()` API (at minimum the basic `fetch(url)` → `Response` path).

### 6. `Blob` constructor not implemented

**Impact:** Blocks structured clone tests and any test creating binary blobs.

`new Blob([...])` throws `ReferenceError: Blob is not defined`.

**What's needed:**
- Implement the `Blob` constructor (`Blob(parts, options)`).

### 7. `Range` interface not implemented

**Impact:** Blocks all Highlight API (`css-highlight-api`) tests.

`new Range()` throws `ReferenceError: Range is not defined`.

**What's needed:**
- Implement the `Range` constructor and basic `AbstractRange` / `StaticRange` /
  `Range` interfaces.

### 8. `document.visualViewport` not implemented

**Impact:** Blocks `css-device-adapt` tests.

`window.visualViewport` returns `undefined`, causing "cannot convert 'null' or
'undefined' to object" errors on viewport property access.

**What's needed:**
- Implement the `VisualViewport` API on `document`.

### 9. `window.devicePixelRatio` not implemented

**Impact:** Blocks `css-device-adapt` tests.

`window.devicePixelRatio` returns `undefined` instead of `1`.

**What's needed:**
- Return `1` from `window.devicePixelRatio` (hardcoded is fine for headless).

### 10. Missing CSS pseudo-elements in selector parser

**Impact:** Blocks `css-forms` pseudo-element tests.

`::checkmark`, `::picker-icon`, and `::picker(select)` pseudo-elements are not
recognised by the CSS selector parser (parse error).  The `:animated-image`
pseudo-class is also missing.

**What's needed:**
- Add support for `::checkmark`, `::picker-icon`, `::picker(select)`, and
  `:animated-image` in the CSS selector parser.

### 11. `env()` CSS function not implemented

**Impact:** Blocks all `css-env` tests.

The `env(safe-area-inset-top)` CSS function is not recognised during property
parsing, so setting `e.style.width = "env(...)"` returns empty string.

**What's needed:**
- Implement the `env()` CSS function with at minimum the `safe-area-inset-*`
  environment variables.

### 12. CSS animation interpolation not implemented

**Impact:** Causes timeouts in animation and interpolation tests.

Tests that trigger CSS animations and wait for interpolation frames time out
because animation frame scheduling is not wired.

**What's needed:**
- Animation frame scheduling and property interpolation.

### 13. Form element UA stylesheet missing

**Impact:** Blocks `css-forms/appearance-base-basic.html`.

Input elements (`<input>`, `<select>`, `<textarea>`, `<button>`, etc.) do not
receive default UA styles — in particular `box-sizing: border-box` is missing,
so tests cannot read it from `getComputedStyle()`.

**What's needed:**
- Add a UA stylesheet that sets basic form element defaults.

### 14. CSSOM interfaces missing/incomplete

**Impact:** Blocks constructable stylesheet tests, CSS rule type tests.

`CSSStyleSheet()`, `CSSStyleRule`, `CSSKeyframesRule`, `CSSMediaRule`,
`CSSFontFaceRule`, `CSSNamespaceRule`, `CSSGroupingRule`, `CSSConditionRule`,
`CSSCounterStyleRule` and other CSSOM interfaces are either missing or have
incomplete implementations.  `MediaList` is also incomplete.

**What's needed:**
- Implement CSSOM interfaces per https://drafts.csswg.org/cssom/.

### 15. `OperatorNode` / `calc()` serialisation incomplete

**Impact:** Blocks `css-values/calc-serialization.html`.

`calc()` values are parsed but their serialised form returns `undefined` instead
of a canonicalised string like `"calc(10% + 10px + 1vmin)"`.

**What's needed:**
- Implement canonical serialisation of `calc()` expressions.

## Summary by directory

| Directory | Testable | Passed | Pass rate | Main blocker |
|-----------|----------|--------|-----------|--------------|
| css-content | 8 | 1 | 12% | `content` property not in computed style |
| css-color-hdr | 4 | 0 | 0% | Color property not in computed style |
| css-device-adapt | 7 | 0 | 0% | `visualViewport`/`devicePixelRatio` missing |
| css-exclusions | 8 | 0 | 0% | `CSS.supports()` always false |
| css-forced-color-adjust | 4 | 1 | 25% | Property not in computed style (1 invalid test passes) |
| css-forms | 7 | 0 | 0% | Pseudo-elements / UA styles missing |
| css-color-adjust | 13 | 1 | 8% | Property not in computed style |
| css-easing | 12 | 1 | 8% | Property not in computed style |
| css-env | 10 | 0 | 0% | `env()` function not implemented |
| css-highlight-api | 16 | 1 | 6% | `Range` not implemented |
| css-layout-api | 7 | 0 | 0% | `CSS.layoutWorklet` missing |
| css-overscroll-behavior | 10 | 1 | 10% | Property not in computed style |
| css-page | 23 | 3 | 13% | Property not in computed style |
| css-rhythm | 17 | 5 | 29% | Property not in computed style |
| css-ruby | 19 | 6 | 32% | Most tests pass on invalid/valid parsing |
| css-scrollbars | 25 | 5 | 20% | Some property parsing passes |
| css-size-adjust | 5 | 1 | 20% | Property not in computed style |
| css-will-change | 5 | 1 | 20% | Property not in computed style |
| css-display | 33 | 6 | 18% | Property not in computed style |
| css-syntax | ~40 | 2 | 5% | Tokenizer tests pass; an+b parsing fails |
| cssom | ~182 | 3 | 2% | CSSOM interfaces incomplete |
| css-values | ~235 | 1 | <1% | `calc()` / property registration missing |
| css-variables | ~57 | 0 | 0% | Custom property parsing incomplete |
| css-backgrounds | ~121 | 0 | 0% | Property not in computed style |
| css-color | ~60 | 1 | 2% | Property not in computed style |
| css-fonts | ~158 | 0 | 0% | Property not in computed style |
| css-lists | ~35 | 0 | 0% | Property not in computed style |
| css-text | ~351 | 0 | 0% | Property not in computed style |
| css-ui | ~118 | 0 | 0% | Property not in computed style |
| css-position | ~105 | 0 | 0% | Property not in computed style |
| css-animations | ~115 | 0 | 0% | Property not in computed style |
| css-cascade | ~80 | 0 | 0% | Mostly reftests |
| css-flexbox | ~345 | 0 | 0% | Mostly reftests |
| css-grid | ~605 | 0 | 0% | Mostly reftests |
| css-images | ~36 | 0 | 0% | Mostly reftests |
| css-masking | ~60 | 0 | 0% | Mostly reftests |
| css-multicol | ~94 | 0 | 0% | Mostly reftests |
| css-overflow | ~204 | 0 | 0% | Mostly reftests |
| css-shapes | ~135 | 0 | 0% | Mostly reftests |
| css-sizing | ~155 | 0 | 0% | Mostly reftests |
| css-tables | ~135 | 0 | 0% | Mostly reftests |
| css-text-decor | ~45 | 0 | 0% | Mostly reftests |
| css-transforms | ~104 | 0 | 0% | Mostly reftests |
| css-transitions | ~119 | 0 | 0% | Mostly reftests |
| css-typed-om | ~354 | 0 | 0% | CSSOM interfaces missing |
| css-writing-modes | ~82 | 0 | 0% | Mostly reftests |
| CSS2 | ~65 | 0 | 0% | Mostly reftests |
| compositing | ~17 | 0 | 0% | Mostly reftests |
| filter-effects | ~36 | 0 | 0% | Mostly reftests |
| geometry | ~26 | ? | ? | Not tested |
| mediaqueries | ~31 | 0 | 0% | Mostly reftests |
| motion | ~45 | 0 | 0% | Mostly reftests |
| selectors | ~262 | 0 | 0% | Mostly reftests or CSS.supports() |
| cssom-view | ~202 | 0 | 0% | Not tested |
| css-properties-values-api | ~107 | 0 | 0% | Not tested |
| css-pseudo | ~53 | 0 | 0% | Not tested |
| css-scroll-anchoring | ~71 | 0 | 0% | Not tested |
| css-scroll-snap | ~151 | 0 | 0% | Not tested |
| css-shadow | ~90 | 0 | 0% | Not tested |
| css-break | ~48 | 0 | 0% | Not tested (reftest-heavy) |
| css-contain | ~95 | 0 | 0% | Not tested (reftest-heavy) |
| css-conditional | ~206 | 0 | 0% | Mostly reftests |
| css-gaps | ~75 | 0 | 0% | Not tested |
| css-inline | ~58 | 0 | 0% | Not tested |
| css-logical | ~86 | 0 | 0% | Not tested |
| css-mixins | ~47 | 0 | 0% | Not tested |
| css-nesting | ~22 | 0 | 0% | Not tested (reftest-heavy) |
| css-view-transitions | ~136 | 0 | 0% | Not tested (reftest-heavy) |
| fill-stroke | ~10 | 0 | 0% | Not tested |

## Quickest wins (highest impact per implementation effort)

| Missing API | Tests unblocked | Estimated effort |
|-------------|----------------|-----------------|
| `CSS.supports(property, value)` (wire to property table) | All `*-computed.html` + `*-valid.html` (~2,500 tests) | Small |
| `getComputedStyle()` value round-trip for basic properties | All `*-valid.html` tests | Medium |
| `window.devicePixelRatio` → return `1` | 7 device-adapt tests | Trivial |
| `Range` constructor | 16 highlight-api tests | Small |
| `Blob` constructor | structured-clone + idlharness tests | Small |
| `fetch()` (basic `fetch(url)` → `Response`) | All idlharness tests | Medium |
| `env()` CSS function | 10 css-env tests | Small |
