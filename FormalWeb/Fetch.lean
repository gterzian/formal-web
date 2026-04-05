import Std.Data.TreeMap
import Std.Sync.Channel
import Mathlib.Control.Monad.Writer
import FormalWeb.FFI
import FormalWeb.Navigation

namespace FormalWeb

deriving instance Repr for ByteArray

/-- https://fetch.spec.whatwg.org/#fetch-controller -/
structure FetchController where
  /-- Model-local identifier for https://fetch.spec.whatwg.org/#fetch-controller -/
  id : Nat
  /-- https://fetch.spec.whatwg.org/#fetch-controller-state -/
  state : String := "ongoing"
deriving Repr, DecidableEq

/-- Model-local bridge from an HTML navigation wait to https://fetch.spec.whatwg.org/#concept-fetch. -/
structure PendingFetchRequest where
  /-- Model-local identifier corresponding to https://fetch.spec.whatwg.org/#fetch-controller -/
  fetchId : Nat
  /-- Model-local reference back to https://html.spec.whatwg.org/multipage/#navigation-params-id -/
  navigationId : Nat
  /-- https://fetch.spec.whatwg.org/#concept-request -/
  request : NavigationRequest
deriving Repr, DecidableEq

structure DocumentFetchRequest where
  /-- Model-local identifier corresponding to https://fetch.spec.whatwg.org/#fetch-controller -/
  fetchId : Nat
  /-- https://fetch.spec.whatwg.org/#concept-request -/
  request : NavigationRequest
deriving Repr, DecidableEq

/-- Model-local pending state for a started https://fetch.spec.whatwg.org/#concept-fetch. -/
structure PendingFetch where
  /-- https://fetch.spec.whatwg.org/#concept-request -/
  request : NavigationRequest
  /-- https://fetch.spec.whatwg.org/#fetch-params-controller -/
  controller : FetchController
deriving Repr, DecidableEq

structure FetchResponse where
  /-- https://fetch.spec.whatwg.org/#concept-response-url -/
  url : String
  /-- https://fetch.spec.whatwg.org/#concept-response-status -/
  status : Nat := 200
  /-- Minimal MIME type surface for loading-a-document dispatch. -/
  contentType : String := ""
  /-- Raw response bytes for Rust-side document fetch consumers. -/
  body : ByteArray := ByteArray.empty
deriving Repr, DecidableEq

/-- Model-local top-level state for fetch processing. -/
structure Fetch where
  /-- Model-local map of started fetches keyed by controller identifier. -/
  pendingFetches : Std.TreeMap Nat PendingFetch := Std.TreeMap.empty
deriving Repr

instance : Inhabited Fetch where
  default := {}

namespace Fetch

def pendingFetch?
    (fetch : Fetch)
    (controllerId : Nat) :
    Option PendingFetch :=
  fetch.pendingFetches.get? controllerId

end Fetch

/-- https://fetch.spec.whatwg.org/#fetch-scheme -/
def isFetchScheme (url : String) : Bool :=
  url.startsWith "http://" || url.startsWith "https://" || url.startsWith "file://"

/-- https://fetch.spec.whatwg.org/#concept-fetch -/
def conceptFetch
    (fetch : Fetch)
    (fetchId : Nat)
    (request : NavigationRequest) :
    Fetch × FetchController :=
  let controller : FetchController := {
    id := fetchId
  }
  let pendingFetch : PendingFetch := {
    request
    controller
  }
  let fetch := {
    fetch with
      pendingFetches := fetch.pendingFetches.insert controller.id pendingFetch
  }
  (fetch, controller)

def conceptNavigationFetch
    (fetch : Fetch)
    (pendingRequest : PendingFetchRequest) :
    Fetch × FetchController :=
  conceptFetch fetch pendingRequest.fetchId pendingRequest.request

def conceptDocumentFetch
    (fetch : Fetch)
    (pendingRequest : DocumentFetchRequest) :
    Fetch × FetchController :=
  conceptFetch fetch pendingRequest.fetchId pendingRequest.request

def navigationResponseOfFetchResponse
    (response : FetchResponse) :
    NavigationResponse :=
  {
    url := response.url
    status := response.status
    contentType := if response.contentType.isEmpty then "text/html" else response.contentType
    body := (String.fromUTF8? response.body).getD ""
  }

/-- Model the point where a pending fetch completes and leaves the fetch set. -/
def completeFetch
    (fetch : Fetch)
    (controllerId : Nat) :
    Fetch × Option PendingFetch :=
  let pendingFetch := fetch.pendingFetches.get? controllerId
  let pendingFetches :=
    match pendingFetch with
    | some _ => fetch.pendingFetches.erase controllerId
    | none => fetch.pendingFetches
  let fetch := {
    fetch with
      pendingFetches
  }
  (fetch, pendingFetch)

inductive FetchTaskMessage where
  | startFetch (pendingRequest : PendingFetchRequest)
  | startDocumentFetch (pendingRequest : DocumentFetchRequest)
  | finishFetch (controllerId : Nat) (response : FetchResponse)
deriving Repr, DecidableEq

inductive FetchNotification where
  | fetchCompleted (fetchId : Nat) (response : FetchResponse)
deriving Repr, DecidableEq

structure SpawnedFetchTask where
  controllerId : Nat
  request : NavigationRequest
deriving Repr, DecidableEq

inductive FetchEffect where
  | startFetch (pendingRequest : PendingFetchRequest) (task : SpawnedFetchTask)
  | startDocumentFetch (pendingRequest : DocumentFetchRequest) (task : SpawnedFetchTask)
  | completeFetch (controllerId : Nat) (response : FetchResponse) (notification : FetchNotification)
deriving Repr, DecidableEq

abbrev FetchM := WriterT (Array FetchEffect) (StateM Fetch)

namespace FetchM

def emit (effect : FetchEffect) : FetchM Unit :=
  tell #[effect]

def startFetch
    (pendingRequest : PendingFetchRequest)
    (task : SpawnedFetchTask) : FetchM Unit :=
  emit (.startFetch pendingRequest task)

def startDocumentFetch
    (pendingRequest : DocumentFetchRequest)
    (task : SpawnedFetchTask) : FetchM Unit :=
  emit (.startDocumentFetch pendingRequest task)

def completeFetch
    (controllerId : Nat)
    (response : FetchResponse)
    (notification : FetchNotification) : FetchM Unit :=
  emit (.completeFetch controllerId response notification)

end FetchM

def startNavigationFetchM
    (pendingRequest : PendingFetchRequest) :
    FetchM Unit := fun fetch =>
  let (nextFetch, controller) := conceptNavigationFetch fetch pendingRequest
  (((), #[FetchEffect.startFetch pendingRequest {
    controllerId := controller.id
    request := pendingRequest.request
  }]), nextFetch)

def startDocumentFetchM
    (pendingRequest : DocumentFetchRequest) :
    FetchM Unit := fun fetch =>
  let (nextFetch, controller) := conceptDocumentFetch fetch pendingRequest
  (((), #[FetchEffect.startDocumentFetch pendingRequest {
    controllerId := controller.id
    request := pendingRequest.request
  }]), nextFetch)

def finishFetchM
    (controllerId : Nat)
    (response : FetchResponse) :
    FetchM Unit := fun fetch =>
  let (nextFetch, pendingFetch?) := completeFetch fetch controllerId
  match pendingFetch? with
  | none =>
      (((), #[]), nextFetch)
  | some _ =>
      (((), #[FetchEffect.completeFetch controllerId response (.fetchCompleted controllerId response)]), nextFetch)

def handleFetchTaskMessage
    (message : FetchTaskMessage) :
    FetchM Unit :=
  match message with
  | .startFetch pendingRequest =>
      startNavigationFetchM pendingRequest
  | .startDocumentFetch pendingRequest =>
      startDocumentFetchM pendingRequest
  | .finishFetch controllerId response =>
      finishFetchM controllerId response

def runFetchMonadic
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    Array FetchEffect × Fetch :=
  let (((), effects), nextFetch) :=
    (handleFetchTaskMessage message).run fetch
  (effects, nextFetch)

@[simp] theorem runFetchMonadic_startFetch
    (fetch : Fetch)
    (pendingRequest : PendingFetchRequest) :
    runFetchMonadic fetch (.startFetch pendingRequest) =
      (
        #[
          FetchEffect.startFetch
            pendingRequest
            {
              controllerId := (conceptNavigationFetch fetch pendingRequest).2.id
              request := pendingRequest.request
            }
        ],
        (conceptNavigationFetch fetch pendingRequest).1
      ) := by
  rfl

@[simp] theorem runFetchMonadic_startDocumentFetch
    (fetch : Fetch)
    (pendingRequest : DocumentFetchRequest) :
    runFetchMonadic fetch (.startDocumentFetch pendingRequest) =
      (
        #[
          FetchEffect.startDocumentFetch
            pendingRequest
            {
              controllerId := (conceptDocumentFetch fetch pendingRequest).2.id
              request := pendingRequest.request
            }
        ],
        (conceptDocumentFetch fetch pendingRequest).1
      ) := by
  rfl

    @[simp] theorem completeFetch_none
      (fetch : Fetch)
      (controllerId : Nat)
      (hlookup : fetch.pendingFetches[controllerId]? = none) :
      completeFetch fetch controllerId = (fetch, none) := by
      cases fetch with
      | mk pendingFetches =>
        simp [completeFetch, hlookup]

    @[simp] theorem completeFetch_some
      (fetch : Fetch)
      (controllerId : Nat)
      (pendingFetch : PendingFetch)
      (hlookup : fetch.pendingFetches[controllerId]? = some pendingFetch) :
      completeFetch fetch controllerId =
        ({ fetch with pendingFetches := fetch.pendingFetches.erase controllerId }, some pendingFetch) := by
      cases fetch with
      | mk pendingFetches =>
        simp [completeFetch, hlookup]

def fetchTaskStep
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    Fetch :=
  (runFetchMonadic fetch message).2

def fetchTaskExec
    (fetch : Fetch)
    (messages : List FetchTaskMessage) :
    Fetch :=
  messages.foldl fetchTaskStep fetch

private def recvCloseableChannel?
    (channel : Std.CloseableChannel α) :
    IO (Option α) := do
  let receiveTask ← channel.recv
  IO.wait receiveTask

private def trySendAndForget
    (channel : Std.CloseableChannel α)
    (message : α) :
    IO Unit := do
  let _ ← channel.trySend message
  pure ()

private def spawnDetached (action : IO Unit) : IO Unit := do
  let _ ← IO.asTask action
  pure ()

private def withTempOutputPath
    (action : System.FilePath → IO α) :
    IO α := do
  let output ← IO.Process.output { cmd := "mktemp" }
  if output.exitCode != 0 then
    throw <| IO.userError s!"mktemp failed: {output.stderr.trimAscii.toString}"
  let outputPath : System.FilePath := output.stdout.trimAscii.toString
  try
    action outputPath
  finally
    let _ ← IO.Process.output {
      cmd := "rm"
      args := #["-f", outputPath.toString]
    }
    pure ()

private def curlArgsForRequest
    (outputPath : System.FilePath)
    (request : NavigationRequest) :
    Array String :=
  let args := #[
    "-L",
    "--silent",
    "--show-error",
    "-o",
    outputPath.toString,
    "--write-out",
    "%{http_code}\n%{content_type}\n%{url_effective}"
  ]
  let args :=
    if request.method = "GET" then
      args
    else
      args ++ #["-X", request.method]
  let args :=
    match request.body with
    | some body => args ++ #["--data-binary", body]
    | none => args
  args ++ #[request.url]

def fetchResponseForRequest
    (request : NavigationRequest) :
    IO FetchResponse := do
  try
    withTempOutputPath fun outputPath => do
      let output ← IO.Process.output {
        cmd := "curl"
        args := curlArgsForRequest outputPath request
      }
      if output.exitCode == 0 then
        let body ← IO.FS.readBinFile outputPath
        let metadata := output.stdout.splitOn "\n"
        let (statusLine, contentTypeLine, resolvedUrlLine) :=
          match metadata with
          | statusLine :: contentTypeLine :: resolvedUrlLine :: _ =>
              (statusLine, contentTypeLine, resolvedUrlLine)
          | _ =>
              ("200", "", request.url)
        let resolvedUrlLine := resolvedUrlLine.trimAscii.toString
        let statusLine := statusLine.trimAscii.toString
        let contentTypeLine := contentTypeLine.trimAscii.toString
        pure {
          url := if resolvedUrlLine.isEmpty then request.url else resolvedUrlLine
          status := (statusLine.toNat?).getD 200
          contentType := contentTypeLine
          body
        }
      else
        pure {
          url := request.url
          status := 599
          contentType := "text/html"
          body :=
            s!"<!DOCTYPE html><html><head><title>Fetch failed</title></head><body><pre>{output.stderr}</pre></body></html>".toUTF8
        }
  catch error =>
    pure {
      url := request.url
      status := 599
      contentType := "text/html"
      body :=
        s!"<!DOCTYPE html><html><head><title>Fetch failed</title></head><body><pre>{error.toString}</pre></body></html>".toUTF8
    }

private def spawnFetchRequestTask
    (channel : Std.CloseableChannel FetchTaskMessage)
    (controllerId : Nat)
    (request : NavigationRequest) :
    IO Unit := do
  spawnDetached do
    let response ← fetchResponseForRequest request
    trySendAndForget channel (.finishFetch controllerId response)

def runFetchMessage
    (channel : Std.CloseableChannel FetchTaskMessage)
    (onNotification : FetchNotification -> IO Unit)
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    IO Fetch := do
  let (effects, nextFetch) := runFetchMonadic fetch message
  for effect in effects do
    match effect with
    | .startFetch _ task =>
        spawnFetchRequestTask channel task.controllerId task.request
    | .startDocumentFetch _ task =>
        spawnFetchRequestTask channel task.controllerId task.request
    | .completeFetch _ _ notification =>
        onNotification notification
  pure nextFetch

/-- Process fetch-task messages until the channel is closed. -/
partial def runFetch
    (channel : Std.CloseableChannel FetchTaskMessage)
    (onNotification : FetchNotification -> IO Unit)
    (fetch : Fetch := default) :
    IO Unit := do
  let nextMessage? ← recvCloseableChannel? channel
  match nextMessage? with
  | none =>
      pure ()
  | some message =>
      let nextFetch ← runFetchMessage channel onNotification fetch message
      runFetch channel onNotification nextFetch

end FormalWeb
