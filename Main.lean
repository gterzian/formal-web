import FormalWeb
import FormalWeb.FFI
import Std.Sync.Channel


inductive RuntimeMessage where
  | freshTopLevelTraversable
  | renderingOpportunity


initialize runtimeMessageChannelRef : IO.Ref (Option (Std.CloseableChannel RuntimeMessage)) ←
  IO.mkRef (none : Option (Std.CloseableChannel RuntimeMessage))
initialize runtimeUserAgentRef : IO.Ref FormalWeb.UserAgent ←
  IO.mkRef default
initialize runtimeTraversableIdRef : IO.Ref (Option Nat) ←
  IO.mkRef none

def runtimeMessageOfString? (message : String) : Option RuntimeMessage :=
  if message = "FreshTopLevelTraversable" then
    some .freshTopLevelTraversable
  else
    none

def bootstrapFreshTopLevelTraversable
    (userAgent : FormalWeb.UserAgent) :
    FormalWeb.UserAgent × Nat :=
  let (userAgent, traversable) := FormalWeb.createNewTopLevelTraversable userAgent none ""
  let userAgent := FormalWeb.navigate userAgent traversable "https://example.com"
  let userAgent :=
    match FormalWeb.traversable? userAgent traversable.id with
    | none => userAgent
    | some updatedTraversable =>
        match updatedTraversable.toTraversableNavigable.toNavigable.ongoingNavigation with
        | some (.navigationId navigationId) =>
            FormalWeb.processNavigationFetchResponse userAgent navigationId {
              url := "https://example.com"
              body := "<!DOCTYPE html><html><head><style type=\"text/css\">html, body { height: 100%; margin: 0; } body { display: grid; place-items: center; background: #f4e8d2; }</style></head><body><svg width=\"368\" height=\"106\" viewBox=\"0 0 368 106\" version=\"1.1\" xmlns=\"http://www.w3.org/2000/svg\" style=\"display:block;fill-rule:evenodd;clip-rule:evenodd;stroke-linejoin:round;stroke-miterlimit:2;\"><g><path d=\"M131.548,97.488L131.548,8.369L144.939,8.369C150.903,8.369 155.656,8.831 159.196,9.755C162.774,10.678 165.795,12.236 168.258,14.43C170.759,16.7 172.741,19.528 174.203,22.915C175.703,26.339 176.454,29.802 176.454,33.304C176.454,39.692 174.01,45.098 169.123,49.523C173.856,51.139 177.589,53.967 180.321,58.008C183.091,62.01 184.477,66.666 184.477,71.976C184.477,78.941 182.014,84.828 177.089,89.638C174.126,92.601 170.797,94.66 167.103,95.814C163.063,96.93 158.003,97.488 151.923,97.488L131.548,97.488ZM144.997,46.637L149.21,46.637C154.213,46.637 157.878,45.531 160.206,43.318C162.534,41.106 163.698,37.845 163.698,33.535C163.698,29.341 162.505,26.156 160.119,23.982C157.734,21.808 154.27,20.721 149.73,20.721L144.997,20.721L144.997,46.637ZM144.997,84.847L153.308,84.847C159.388,84.847 163.852,83.654 166.699,81.269C169.701,78.691 171.201,75.42 171.201,71.456C171.201,67.608 169.758,64.376 166.872,61.76C164.063,59.181 159.042,57.892 151.808,57.892L144.997,57.892L144.997,84.847Z\" style=\"fill-rule:nonzero;\"/><rect x=\"202.173\" y=\"0\" width=\"12.987\" height=\"97.488\" style=\"fill-rule:nonzero;\"/><path d=\"M247.806,41.269L247.806,97.488L234.819,97.488L234.819,41.269L247.806,41.269ZM232.857,17.893C232.857,15.623 233.684,13.66 235.338,12.006C236.993,10.351 238.975,9.524 241.284,9.524C243.631,9.524 245.632,10.351 247.286,12.006C248.941,13.622 249.768,15.603 249.768,17.951C249.768,20.298 248.941,22.299 247.286,23.953C245.67,25.608 243.689,26.435 241.341,26.435C238.994,26.435 236.993,25.608 235.338,23.953C233.684,22.299 232.857,20.279 232.857,17.893Z\" style=\"fill-rule:nonzero;\"/><path d=\"M285.856,53.39L285.856,97.488L272.869,97.488L272.869,53.39L267.328,53.39L267.328,41.269L272.869,41.269L272.869,20.663L285.856,20.663L285.856,41.269L295.957,41.269L295.957,53.39L285.856,53.39Z\" style=\"fill-rule:nonzero;\"/><path d=\"M331.64,85.251L365.059,85.251L365.059,97.488L305.897,97.488L342.318,53.39L313.631,53.39L313.631,41.269L368.003,41.269L331.64,85.251Z\" style=\"fill-rule:nonzero;\"/></g><g><g><circle cx=\"53\" cy=\"53\" r=\"53\" style=\"fill:rgb(1,99,63);\"/><circle cx=\"53\" cy=\"53\" r=\"45.773\" style=\"fill:rgb(0,118,114);\"/><circle cx=\"53\" cy=\"53\" r=\"38.545\" style=\"fill:rgb(62,149,147);\"/><circle cx=\"53\" cy=\"53\" r=\"31.318\" style=\"fill:rgb(252,176,64);\"/><circle cx=\"53\" cy=\"53\" r=\"24.091\" style=\"fill:rgb(233,86,41);\"/><circle cx=\"53\" cy=\"53\" r=\"16.864\" style=\"fill:rgb(230,29,50);\"/></g><g><path d=\"M39.759,90.287C39.549,90.287 39.338,90.241 39.137,90.144C38.49,89.83 38.177,89.087 38.404,88.405L49.211,55.986L38.33,55.986C37.853,55.986 37.407,55.747 37.141,55.35C36.875,54.953 36.826,54.448 37.011,54.008L51.303,19.707C51.524,19.174 52.045,18.826 52.622,18.826L66.2,18.826C66.684,18.826 67.136,19.072 67.399,19.478C67.663,19.886 67.702,20.397 67.504,20.839L56.257,45.982L66.914,45.982C67.439,45.982 67.922,46.27 68.172,46.73C68.422,47.192 68.398,47.754 68.11,48.193L40.955,89.64C40.682,90.057 40.228,90.287 39.759,90.287Z\" style=\"fill:rgb(244,232,210);fill-rule:nonzero;\"/></g></g></svg></body></html>"
            }
        | _ => userAgent
  (userAgent, traversable.id)

def startupTraversableReadyHtml?
    (userAgent : FormalWeb.UserAgent)
    (traversableId : Nat) :
    Option String :=
  match FormalWeb.traversable? userAgent traversableId with
  | none => none
  | some traversable =>
      if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
        none
      else
        match traversable.toTraversableNavigable.activeDocument with
        | none => none
        | some document => some (FormalWeb.UserAgent.documentHtml userAgent document)

def handleRuntimeMessage (message : RuntimeMessage) : IO Unit := do
  match message with
  | .freshTopLevelTraversable =>
    let userAgent ← runtimeUserAgentRef.get
    let (userAgent, traversableId) := bootstrapFreshTopLevelTraversable userAgent
    runtimeUserAgentRef.set userAgent
    runtimeTraversableIdRef.set (some traversableId)
    match startupTraversableReadyHtml? userAgent traversableId with
    | none =>
      pure ()
    | some _html =>
      FormalWeb.sendRuntimeMessage "NewTopLevelTraversable"
  | .renderingOpportunity =>
    let traversableId? ← runtimeTraversableIdRef.get
    match traversableId? with
    | none =>
        pure ()
    | some traversableId =>
        let userAgent ← runtimeUserAgentRef.get
        FormalWeb.noteRenderingOpportunity userAgent traversableId

partial def runtimeMessageLoop (channel : Std.CloseableChannel RuntimeMessage) : IO Unit := do
  let receiveTask ← channel.recv
  match (← IO.wait receiveTask) with
  | none =>
      pure ()
  | some message =>
      handleRuntimeMessage message
      runtimeMessageLoop channel

def enqueueRuntimeMessage (message : RuntimeMessage) : IO Unit := do
  let channel? ← runtimeMessageChannelRef.get
  match channel? with
  | none =>
      pure ()
  | some channel =>
      let sent ← channel.trySend message
      let _ := sent
      pure ()


@[export formal_web_user_agent_note_rendering_opportunity]
def userAgentNoteRenderingOpportunity (message : String) : IO Unit := do
  let _ := message
  let _ <- IO.asTask <| enqueueRuntimeMessage .renderingOpportunity
  pure ()

@[export formal_web_handle_runtime_message]
def handleRuntimeMessageFromRust (message : String) : IO Unit := do
  match runtimeMessageOfString? message with
  | none =>
      pure ()
  | some runtimeMessage =>
      let _ <- IO.asTask <| enqueueRuntimeMessage runtimeMessage
      pure ()

def main : IO Unit := do
  let runtimeMessageChannel ← Std.CloseableChannel.new
  runtimeUserAgentRef.set default
  runtimeTraversableIdRef.set none
  runtimeMessageChannelRef.set (some runtimeMessageChannel)
  let worker ← IO.asTask (runtimeMessageLoop runtimeMessageChannel)
  FormalWeb.runWinitEventLoop ()
  runtimeMessageChannelRef.set (none : Option (Std.CloseableChannel RuntimeMessage))
  runtimeMessageChannel.close
  let _ ← IO.wait worker
  pure ()
