import Std.Data.TreeMap
import Std.Sync.Channel
import Mathlib.Control.Monad.Writer

namespace FormalWeb

/-- https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#run-steps-after-a-timeout -/
inductive TimerCompletion where
  | windowTimerTask
      (documentId : Nat)
      (timerId : Nat)
      (timerKey : Nat)
      (nestingLevel : Nat)
  | documentFetchTimeout (handlerId : Nat)
deriving Repr, DecidableEq

structure TimerNotification where
  eventLoopId : Nat
  completion : TimerCompletion
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#run-steps-after-a-timeout -/
structure RunStepsAfterTimeoutRequest where
  /-- Model-local implementation of the spec's unique internal value. -/
  timerKey : Nat
  /-- Model-local stand-in for the relevant global object used by the ordering checks. -/
  globalId : Nat
  /-- https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#run-steps-after-a-timeout -/
  orderingIdentifier : String
  /-- https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#run-steps-after-a-timeout -/
  milliseconds : Nat
  /-- Model-local owner that receives the completion steps. -/
  eventLoopId : Nat
  /-- Model-local encoding of the completion steps. -/
  completion : TimerCompletion
deriving Repr, DecidableEq

structure ScheduledTimer where
  request : RunStepsAfterTimeoutRequest
  startedAtMs : Nat
  deadlineAtMs : Nat
  sequenceNumber : Nat
deriving Repr, DecidableEq

/-- Model-local top-level state for timers. -/
structure Timer where
  activeTimers : Std.TreeMap Nat ScheduledTimer := Std.TreeMap.empty
  nextSequenceNumber : Nat := 0
  wakeGeneration : Nat := 0
  nextWakeDeadlineAtMs : Option Nat := none
deriving Repr

instance : Inhabited Timer where
  default := {}

inductive TimerTaskMessage where
  | scheduleTimeout (nowMs : Nat) (request : RunStepsAfterTimeoutRequest)
  | clearTimeout (nowMs : Nat) (timerKey : Nat)
  | wake (nowMs : Nat) (generation : Nat)
deriving Repr, DecidableEq

inductive TimerEffect where
  | scheduleWake (generation : Nat) (delayMs : Nat)
  | notify (notification : TimerNotification)
deriving Repr, DecidableEq

abbrev TimerM := WriterT (Array TimerEffect) (StateM Timer)

namespace TimerM

def emit (effect : TimerEffect) : TimerM Unit :=
  tell #[effect]

def scheduleWake (generation : Nat) (delayMs : Nat) : TimerM Unit :=
  emit (.scheduleWake generation delayMs)

def notify (notification : TimerNotification) : TimerM Unit :=
  emit (.notify notification)

end TimerM

private def nextWakeDeadlineAtMs?
    (timer : Timer) :
    Option Nat :=
  timer.activeTimers.foldl
    (fun earliest _ scheduledTimer =>
      match earliest with
      | none =>
          some scheduledTimer.deadlineAtMs
      | some deadlineAtMs =>
          some (Nat.min deadlineAtMs scheduledTimer.deadlineAtMs))
    none

private def timerOrderingLt
    (lhs rhs : ScheduledTimer) :
    Bool :=
  if lhs.deadlineAtMs < rhs.deadlineAtMs then
    true
  else if rhs.deadlineAtMs < lhs.deadlineAtMs then
    false
  else
    lhs.sequenceNumber < rhs.sequenceNumber

private def insertScheduledTimer
    (scheduledTimer : ScheduledTimer)
    (sortedTimers : List ScheduledTimer) :
    List ScheduledTimer :=
  match sortedTimers with
  | [] =>
      [scheduledTimer]
  | headTimer :: tailTimers =>
      if timerOrderingLt scheduledTimer headTimer then
        scheduledTimer :: sortedTimers
      else
        headTimer :: insertScheduledTimer scheduledTimer tailTimers

private def sortScheduledTimers
    (timers : List ScheduledTimer) :
    List ScheduledTimer :=
  timers.foldl
    (fun sortedTimers scheduledTimer => insertScheduledTimer scheduledTimer sortedTimers)
    []

private def dueTimers
    (timer : Timer)
    (nowMs : Nat) :
    List ScheduledTimer :=
  sortScheduledTimers <|
    timer.activeTimers.foldl
      (fun readyTimers _ scheduledTimer =>
        if scheduledTimer.deadlineAtMs ≤ nowMs then
          scheduledTimer :: readyTimers
        else
          readyTimers)
      []

/-- https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#run-steps-after-a-timeout -/
private def refreshWakeM
    (nowMs : Nat) :
    TimerM Unit := do
  let timer ← get
  let nextWakeDeadlineAtMs? := nextWakeDeadlineAtMs? timer
  if timer.nextWakeDeadlineAtMs = nextWakeDeadlineAtMs? then
    pure ()
  else
    let nextGeneration := timer.wakeGeneration + 1
    set {
      timer with
        wakeGeneration := nextGeneration
        nextWakeDeadlineAtMs := nextWakeDeadlineAtMs?
    }
    match nextWakeDeadlineAtMs? with
    | none =>
        pure ()
    | some nextWakeDeadlineAtMs =>
        TimerM.scheduleWake nextGeneration (nextWakeDeadlineAtMs - nowMs)

/-- https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#run-steps-after-a-timeout -/
def runStepsAfterTimeoutM
    (nowMs : Nat)
    (request : RunStepsAfterTimeoutRequest) :
    TimerM Unit := do
  let timer ← get

  -- Note: This helper models the prefix of `run steps after a timeout` through the point where the in-parallel wait is armed. The caller has already reserved the model-local `timerKey`, so this helper starts with Step 2.

  -- Step 2: "Let startTime be the current high resolution time given global."
  let startTime := nowMs

  -- Step 3: "Set global's map of active timers[timerKey] to startTime plus milliseconds."
  -- Note: `expiryTime` is the value written into the model-local active-timer map for this timer key.
  let expiryTime := startTime + request.milliseconds

  let scheduledTimer : ScheduledTimer := {
    request
    startedAtMs := startTime
    deadlineAtMs := expiryTime
    sequenceNumber := timer.nextSequenceNumber
  }
  set {
    timer with
      activeTimers := timer.activeTimers.insert request.timerKey scheduledTimer
      nextSequenceNumber := timer.nextSequenceNumber + 1
  }

  -- Step 4: "Run the following steps in parallel:"
  -- Note: `refreshWakeM` arms or reschedules the detached wake task that later resumes the in-parallel continuation in `processWakeM`.
  refreshWakeM nowMs

/-- https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#run-steps-after-a-timeout -/
def clearActiveTimerM
    (nowMs : Nat)
    (timerKey : Nat) :
    TimerM Unit := do
  modify fun timer =>
    {
      timer with
        activeTimers := timer.activeTimers.erase timerKey
    }

  refreshWakeM nowMs

/-- https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#run-steps-after-a-timeout -/
def processWakeM
    (nowMs : Nat)
    (generation : Nat) :
    TimerM Unit := do
  let timer ← get

  -- Note: This helper resumes the in-parallel continuation of `run steps after a timeout` after the detached wake fires. The wall-clock sleep has already elapsed, so this helper continues with the remaining wait conditions and completion steps.

  if generation ≠ timer.wakeGeneration then
    pure ()
  else
    let readyTimers := dueTimers timer nowMs

    -- Step 4.1: "If global is a Window object, wait until global's associated Document has been fully active for a further milliseconds milliseconds (not necessarily consecutively)."
    -- Note: The detached wake task already accounts for the wall-clock portion of the wait. The remaining fully-active `Document` condition is still modelled as a TODO.
    -- TODO: Track fully-active `Document` time instead of relying on wall-clock elapsed time alone.

    -- Step 4.2: "Wait until any invocations of this algorithm that had the same global and orderingIdentifier, that started before this one, and whose milliseconds is less than or equal to this one's, have completed."
    -- Note: The worker keeps a single ordered wake path and drains due timers by deadline and insertion sequence so identical timer groups notify in FIFO order.

    -- Step 4.3: "Optionally, wait a further implementation-defined length of time."
    -- TODO: Add implementation-defined padding if the embedder needs timer coalescing.

    -- Step 4.4: "Perform completionSteps."
    for scheduledTimer in readyTimers do
      TimerM.notify {
        eventLoopId := scheduledTimer.request.eventLoopId
        completion := scheduledTimer.request.completion
      }

    -- Step 4.5: "Remove global's map of active timers[timerKey]."
    let activeTimers :=
      readyTimers.foldl
        (fun remainingTimers scheduledTimer =>
          remainingTimers.erase scheduledTimer.request.timerKey)
        timer.activeTimers
    set {
      timer with
        activeTimers
    }

    refreshWakeM nowMs

def handleTimerTaskMessage
    (message : TimerTaskMessage) :
    TimerM Unit :=
  match message with
  | .scheduleTimeout nowMs request =>
      runStepsAfterTimeoutM nowMs request
  | .clearTimeout nowMs timerKey =>
      clearActiveTimerM nowMs timerKey
  | .wake nowMs generation =>
      processWakeM nowMs generation

def runTimerMessagesMonadic
    (timer : Timer)
    (messages : List TimerTaskMessage) :
    Array TimerEffect × Timer :=
  let (((), effects), nextTimer) :=
    (messages.foldlM (fun _ message => handleTimerTaskMessage message) ()).run timer
  (effects, nextTimer)

inductive TimerRuntimeMessage where
  | task (message : TimerTaskMessage)
  | scheduleTimeout
      (nowMs : Nat)
      (request : RunStepsAfterTimeoutRequest)
      (onComplete : TimerCompletion -> IO Unit)

structure TimerRuntimeState where
  timer : Timer := default
  completions : Std.TreeMap Nat (TimerCompletion -> IO Unit) := Std.TreeMap.empty

instance : Inhabited TimerRuntimeState where
  default := {}

private def recvCloseableChannel?
    (channel : Std.CloseableChannel α) :
    IO (Option α) := do
  let receiveTask ← channel.recv
  IO.wait receiveTask

private partial def drainCloseableChannel
    (channel : Std.CloseableChannel α)
    (reversedMessages : List α := []) :
    IO (List α) := do
  match ← channel.tryRecv with
  | some message =>
      drainCloseableChannel channel (message :: reversedMessages)
  | none =>
      pure reversedMessages.reverse

private def recvDrainedMessages?
    (channel : Std.CloseableChannel α) :
    IO (Option (List α)) := do
  let some firstMessage ← recvCloseableChannel? channel | pure none
  let drainedMessages ← drainCloseableChannel channel
  pure (some (firstMessage :: drainedMessages))

private def maxSleepMilliseconds : Nat :=
  4294967295

private partial def sleepMilliseconds
    (delayMs : Nat) :
    IO Unit := do
  if delayMs = 0 then
    pure ()
  else
    let chunk := Nat.min delayMs maxSleepMilliseconds
    IO.sleep (UInt32.ofNat chunk)
    sleepMilliseconds (delayMs - chunk)

private def spawnWakeTask
    (sendWake : Nat -> Nat -> IO Unit)
    (generation : Nat)
    (delayMs : Nat) :
    IO Unit := do
  let _ ← IO.asTask <| do
    sleepMilliseconds delayMs
    let nowMs ← IO.monoMsNow
    sendWake nowMs generation
  pure ()

private def timerCompletionKey
    (completion : TimerCompletion) :
    Nat :=
  match completion with
  | .windowTimerTask _ _ timerKey _ =>
      timerKey
  | .documentFetchTimeout handlerId =>
      handlerId

private def registerCompletion
    (state : TimerRuntimeState)
    (timerKey : Nat)
    (onComplete : TimerCompletion -> IO Unit) :
    TimerRuntimeState :=
  {
    state with
      completions := state.completions.insert timerKey onComplete
  }

private def clearCompletion
    (state : TimerRuntimeState)
    (timerKey : Nat) :
    TimerRuntimeState :=
  {
    state with
      completions := state.completions.erase timerKey
  }

private def takeCompletion?
    (state : TimerRuntimeState)
    (timerKey : Nat) :
    TimerRuntimeState × Option (TimerCompletion -> IO Unit) :=
  let onComplete? := state.completions.get? timerKey
  ({ state with completions := state.completions.erase timerKey }, onComplete?)

def runTimerMessages
    (notify : TimerNotification → IO Unit)
    (channel : Std.CloseableChannel TimerTaskMessage)
    (timer : Timer)
    (messages : List TimerTaskMessage) :
    IO Timer := do
  let (effects, nextTimer) := runTimerMessagesMonadic timer messages
  for effect in effects do
    match effect with
    | .scheduleWake generation delayMs =>
        spawnWakeTask
          (fun nowMs wakeGeneration => do
            let _ ← channel.trySend (.wake nowMs wakeGeneration)
            pure ())
          generation
          delayMs
    | .notify notification =>
        notify notification
  pure nextTimer

private def runTimerRuntimeTaskMessage
    (channel : Std.CloseableChannel TimerRuntimeMessage)
    (state : TimerRuntimeState)
    (message : TimerTaskMessage) :
    IO TimerRuntimeState := do
  let state :=
    match message with
    | .clearTimeout _ timerKey =>
        clearCompletion state timerKey
    | _ =>
        state
  let (effects, nextTimer) := runTimerMessagesMonadic state.timer [message]
  let mut nextState := { state with timer := nextTimer }
  for effect in effects do
    match effect with
    | .scheduleWake generation delayMs =>
        spawnWakeTask
          (fun nowMs wakeGeneration => do
            let _ ← channel.trySend (.task (.wake nowMs wakeGeneration))
            pure ())
          generation
          delayMs
    | .notify notification =>
        let timerKey := timerCompletionKey notification.completion
        let (updatedState, onComplete?) := takeCompletion? nextState timerKey
        nextState := updatedState
        match onComplete? with
        | some onComplete =>
            onComplete notification.completion
        | none =>
            pure ()
  pure nextState

def runTimerRuntimeMessage
    (channel : Std.CloseableChannel TimerRuntimeMessage)
    (state : TimerRuntimeState)
    (message : TimerRuntimeMessage) :
    IO TimerRuntimeState := do
  match message with
  | .task taskMessage =>
      runTimerRuntimeTaskMessage channel state taskMessage
  | .scheduleTimeout nowMs request onComplete =>
      let state := registerCompletion state request.timerKey onComplete
      runTimerRuntimeTaskMessage channel state (.scheduleTimeout nowMs request)

partial def runTimerRuntime
    (channel : Std.CloseableChannel TimerRuntimeMessage)
    (state : TimerRuntimeState := default) :
    IO Unit := do
  let nextMessage? ← recvCloseableChannel? channel
  match nextMessage? with
  | none =>
      pure ()
  | some message =>
      let nextState ← runTimerRuntimeMessage channel state message
      runTimerRuntime channel nextState

partial def runTimerLoop
    (notify : TimerNotification → IO Unit)
    (channel : Std.CloseableChannel TimerTaskMessage)
    (timer : Timer) :
    IO Unit := do
  let some messages ← recvDrainedMessages? channel | pure ()
  let nextTimer ← runTimerMessages notify channel timer messages
  runTimerLoop notify channel nextTimer

def runTimer
    (channel : Std.CloseableChannel TimerTaskMessage)
    (notify : TimerNotification → IO Unit) :
    IO Unit := do
  runTimerLoop notify channel default

end FormalWeb
