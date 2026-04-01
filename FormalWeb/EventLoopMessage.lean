import FormalWeb.Document
import FormalWeb.FFI
import FormalWeb.Fetch

namespace FormalWeb

inductive EventLoopTaskMessage where
  | createEmptyDocument (documentId : RustDocumentHandle)
  | createLoadedDocument (documentId : RustDocumentHandle) (url : String) (body : String)
  | queueUpdateTheRendering (traversableId : Nat) (documentId : RustDocumentHandle)
  | queueDispatchEvent (documentId : RustDocumentHandle) (event : String)
  | queuePaint (documentId : RustDocumentHandle)
  | queueDocumentFetchCompletion
      (handler : RustNetHandlerPointer)
      (resolvedUrl : String)
      (body : ByteArray)
deriving Repr, DecidableEq

structure EventLoopTaskCompletion where
  traversableId : Nat
  eventLoopId : Nat
  documentId : RustDocumentHandle
deriving DecidableEq

end FormalWeb
