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

/-- Opaque pointer to an embedder-side content-process bridge for one event loop. -/
structure RustContentProcessPointer where
  raw : USize
deriving Repr, DecidableEq, Inhabited

def RustContentProcessPointer.null : RustContentProcessPointer :=
  { raw := 0 }

/-- Opaque pointer to a boxed Rust-side Blitz `NetHandler`. -/
structure RustNetHandlerPointer where
  raw : USize
deriving Repr, DecidableEq, Inhabited

def RustNetHandlerPointer.null : RustNetHandlerPointer :=
  { raw := 0 }

/-- Allocates an embedder-side `HtmlDocument` for a fixed `html/head/body` skeleton. -/
@[extern "createEmptyHtmlDocument"]
opaque createEmptyHtmlDocument : Unit → RustDocumentPointer

/-- Allocates an embedder-side `HtmlDocument` from fetched HTML and a base URL. -/
@[extern "createLoadedHtmlDocument"]
opaque createLoadedHtmlDocument : @& String → @& String → RustDocumentPointer

/-- Renders an embedder-side `HtmlDocument` as an HTML string for debugging. -/
@[extern "renderHtmlDocument"]
opaque renderHtmlDocument : USize → String

/-- Sends a runtime message to the Rust embedder runtime. -/
@[extern "sendEmbedderMessage"]
opaque sendEmbedderMessage : @& String → IO Unit

/-- Runs the Rust-side embedder event loop until the window closes. -/
@[extern "runEmbedderEventLoop"]
opaque runEmbedderEventLoop : Unit → IO Unit

/-- Starts the content process for one event loop and returns the opaque embedder-side bridge handle. -/
@[extern "contentProcessStart"]
opaque contentProcessStart : USize → IO RustContentProcessPointer

/-- Stops the content process for one event loop and tears down the embedder-side bridge. -/
@[extern "contentProcessStop"]
opaque contentProcessStop : USize → IO Unit

/-- Sends a create-empty-document command to the event loop's content process. -/
@[extern "contentProcessCreateEmptyDocument"]
opaque contentProcessCreateEmptyDocument : USize → USize → IO Unit

/-- Sends a create-loaded-document command to the event loop's content process. -/
@[extern "contentProcessCreateLoadedDocument"]
opaque contentProcessCreateLoadedDocument : USize → USize → @& String → @& String → IO Unit

/-- Sends a serialized UI event to the event loop's content process. -/
@[extern "contentProcessDispatchEvent"]
opaque contentProcessDispatchEvent : USize → USize → @& String → IO Unit

/-- Runs the I/O part of update-the-rendering for a document in the event loop's content process. -/
@[extern "contentProcessUpdateTheRendering"]
opaque contentProcessUpdateTheRendering : USize → USize → IO Unit

/-- Completes a queued document fetch inside the event loop's content process. -/
@[extern "contentProcessCompleteDocumentFetch"]
opaque contentProcessCompleteDocumentFetch : USize → USize → @& String → ByteArray → IO Unit

/-- Extracts the `BaseDocument` pointer from an `HtmlDocument` pointer. Called by event-loop tasks to prepare for rendering. -/
@[extern "extractBaseDocument"]
opaque extractBaseDocument : USize → RustBaseDocumentPointer

/-- Queues a `Paint(BaseDocument)` user event onto the winit event loop proxy. -/
@[extern "queuePaint"]
opaque queuePaint : USize → IO Unit

/-- Applies a serialized embedder-side UI event to the given `BaseDocument`. -/
@[extern "applyUiEvent"]
opaque applyUiEvent : USize → @& String → IO Unit

/-- Completes a queued document fetch by calling the boxed Rust-side Blitz `NetHandler`. -/
@[extern "completeDocumentFetch"]
opaque completeDocumentFetch : USize → @& String → ByteArray → IO Unit

end FormalWeb
