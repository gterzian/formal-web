# content/src/cssom

CSSOM (<https://drafts.csswg.org/cssom/>) platform objects and algorithms.
A declaration block's declarations are stylo's `PropertyDeclarationBlock`:
parsing, serialization, shorthand expansion, `!important` and the supported
property set come from stylo's property database, so a property is
supported here exactly when `PropertyId::parse_enabled_for_all_content`
accepts it.

- The camel-cased, dashed and webkit-cased IDL attributes of
  `CSSStyleDeclaration` are generated from tables in
  `js/bindings/cssom/css_style_declaration.rs`; the tables list stylo's
  properties enabled for all content.  A property stylo adds is not exposed
  as an attribute until its row is added there (`setProperty` and
  `getPropertyValue` accept it regardless).
- An element-backed block reads its declarations from the element's `style`
  attribute on each access and writes them back through "update style
  attribute for"; the attribute is the only storage, so Rust-side attribute
  writes are visible without a sync step.

## Remaining work

- `getComputedStyle` blocks are snapshots of the resolved values
  `html/html_element.rs` can source; custom properties are not included
  (`css/cssom/cssstyledeclaration-custom-properties.html`).
- No `CSSStyleSheet`/`CSSRule` objects, so stylesheet-backed declaration
  blocks do not exist (`css/cssom/cssstyledeclaration-mutability.html`).
- Attribute mutation records are not queued for style attribute updates
  (`css/cssom/css-style-attr-decl-block.html`).
