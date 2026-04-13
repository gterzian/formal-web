# Embedder

- Chrome controls use explicit font family, size, and line-height on the address field instead of relying on inherited shorthand so text metrics stay stable in the embedded UI runtime.
- Single-line address inputs in Blitz should clip painted text to the padding box instead of the content box, because glyph bounds can extend above the line box and otherwise get cut off.
- The current embedder chrome is address-bar only; do not leave browser controls in the UI unless their end-to-end behavior is intentionally exposed.