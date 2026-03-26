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

end FormalWeb
