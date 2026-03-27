namespace FormalWeb

/-- Opaque pointer to a Rust-side document object. -/
structure RustDocumentPointer where
  raw : USize
deriving Repr, DecidableEq, Inhabited

def RustDocumentPointer.null : RustDocumentPointer :=
  { raw := 0 }

/-- Opaque pointer to a Rust-side `BaseDocument` extracted from an `HtmlDocument`. -/
structure RustBaseDocumentPointer where
  raw : USize
deriving Repr, DecidableEq, Inhabited

def RustBaseDocumentPointer.null : RustBaseDocumentPointer :=
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

/-- Sends a runtime message to the Rust host runtime. -/
@[extern "formal_web_send_runtime_message"]
opaque sendRuntimeMessage : @& String → IO Unit

/-- Runs the Rust-side `winit` demo event loop until the window closes. -/
@[extern "formal_web_run_winit_event_loop"]
opaque runWinitEventLoop : Unit → IO Unit

/-- Extracts the `BaseDocument` pointer from an `HtmlDocument` pointer. Called by event-loop tasks to prepare for rendering. -/
@[extern "formal_web_extract_base_document"]
opaque extractBaseDocument : USize → RustBaseDocumentPointer

/-- Queues a `Paint(BaseDocument)` user event onto the winit event loop proxy. -/
@[extern "formal_web_queue_paint"]
opaque queuePaint : USize → IO Unit

end FormalWeb
