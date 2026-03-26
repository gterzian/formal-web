namespace FormalWeb

/-- Opaque pointer to a Rust-side document object. -/
structure RustDocumentPointer where
  raw : USize
deriving Repr, DecidableEq, Inhabited

def RustDocumentPointer.null : RustDocumentPointer :=
  { raw := 0 }

/-- Allocates a host-side `HtmlDocument` for a fixed `html/head/body` skeleton. -/
@[extern "formal_web_create_empty_html_document"]
opaque createEmptyHtmlDocument : Unit → RustDocumentPointer

/-- Allocates a host-side `HtmlDocument` for a fixed loaded-page HTML document. -/
@[extern "formal_web_create_loaded_html_document"]
opaque createLoadedHtmlDocument : Unit → RustDocumentPointer

/-- Renders a host-side `HtmlDocument` as an HTML string for debugging. -/
@[extern "formal_web_render_html_document"]
opaque renderHtmlDocument : USize → String

end FormalWeb
