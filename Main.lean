import FormalWeb

open FormalWeb

def printDocumentHtml
    (label : String)
    (userAgent : UserAgent)
    (document : Document) :
    IO Unit := do
  IO.println s!"{label}:"
  IO.println s!"  URL: {document.url}"
  IO.println s!"  HTML: {userAgent.documentHtml document}"

def firstPendingNavigationId?
    (userAgent : UserAgent) :
    Option Nat :=
  match userAgent.pendingNavigationFetches with
  | pendingNavigationFetch :: _ => some pendingNavigationFetch.navigationId
  | [] => none

def main : IO Unit := do
  let (userAgent, topLevelTraversable) := createNewTopLevelTraversable {} none "demo"
  let some initialDocument := topLevelTraversable.toTraversableNavigable.activeDocument
    | throw <| IO.userError "expected an initial about:blank document"
  printDocumentHtml "Initial blank document" userAgent initialDocument

  let userAgent := navigate userAgent topLevelTraversable "https://example.test/loaded"
  let some navigationId := firstPendingNavigationId? userAgent
    | throw <| IO.userError "expected a stub pending navigation fetch"
  let response : NavigationResponse := {
    url := "https://example.test/loaded"
    contentType := "text/html"
    body := "<html><head><title>Loaded</title></head><body><p>Loaded!</p></body></html>"
  }
  let userAgent := processNavigationFetchResponse userAgent navigationId response
  let some navigatedTraversable := FormalWeb.traversable? userAgent topLevelTraversable.id
    | throw <| IO.userError "expected the traversable to remain present after navigation"
  let some navigatedDocument := navigatedTraversable.toTraversableNavigable.activeDocument
    | throw <| IO.userError "expected navigation to install an active document"

  IO.println ""
  printDocumentHtml "Navigated document" userAgent navigatedDocument
