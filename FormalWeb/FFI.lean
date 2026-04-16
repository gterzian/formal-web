namespace FormalWeb

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

/-- Sends a destroy-document command to the event loop's content process. -/
@[extern "contentProcessDestroyDocument"]
opaque contentProcessDestroyDocument : USize → USize → IO Unit

/-- Sends a serialized UI event batch to the event loop's content process. -/
@[extern "contentProcessDispatchEvent"]
opaque contentProcessDispatchEvent : USize → @& String → IO Unit

/-- Runs the `beforeunload` steps for a document in the event loop's content process. -/
@[extern "contentProcessRunBeforeUnload"]
opaque contentProcessRunBeforeUnload : USize → USize → USize → IO Unit

/-- Runs the I/O part of update-the-rendering for a document in the event loop's content process. -/
@[extern "contentProcessUpdateTheRendering"]
opaque contentProcessUpdateTheRendering : USize → USize → IO Unit

/-- Runs one queued timer task inside the event loop's content process. -/
@[extern "contentProcessRunWindowTimer"]
opaque contentProcessRunWindowTimer : USize → USize → USize → USize → USize → IO Unit

/-- Fails one queued document fetch inside the event loop's content process. -/
@[extern "contentProcessFailDocumentFetch"]
opaque contentProcessFailDocumentFetch : USize → USize → IO Unit

/-- Completes a queued document fetch inside the event loop's content process. -/
@[extern "contentProcessCompleteDocumentFetch"]
opaque contentProcessCompleteDocumentFetch : USize → USize → @& String → ByteArray → IO Unit

end FormalWeb
