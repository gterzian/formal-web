import FormalWeb.UserAgent
import FormalWeb.Proofs.TransitionSystem

namespace FormalWeb

/-- High-level LTS action for one user-agent message together with its state contract. -/
structure UserAgentAction where
  message : UserAgentTaskMessage
  precondition : UserAgent → Prop
  postcondition : UserAgent → UserAgent → Prop

def defaultUserAgentAction
    (message : UserAgentTaskMessage) :
    UserAgentAction := {
      message
      precondition := fun _ => True
      postcondition := fun userAgent userAgent' =>
        userAgent' = (runMonadic userAgent message).2
    }

/-- Relational LTS for user-agent task-message handling. -/
def userAgentLTS : TransitionSystem.LTS UserAgent UserAgentAction where
  init := fun userAgent => userAgent = default
  trans := fun userAgent action userAgent' =>
    action.precondition userAgent ∧ action.postcondition userAgent userAgent'

def runMonadicState
    (userAgent : UserAgent)
    (message : UserAgentTaskMessage) :
    UserAgent :=
  (runMonadic userAgent message).2

theorem runMonadic_refines_defaultAction
    (userAgent : UserAgent)
    (message : UserAgentTaskMessage) :
    userAgentLTS.trans
      userAgent
      (defaultUserAgentAction message)
      (runMonadicState userAgent message) := by
  simp [userAgentLTS, defaultUserAgentAction, runMonadicState]

theorem runMonadic_trace
    (userAgent : UserAgent)
    (message : UserAgentTaskMessage) :
    TransitionSystem.TransitionTrace
      userAgentLTS
      userAgent
      [defaultUserAgentAction message]
      (runMonadicState userAgent message) := by
  exact TransitionSystem.TransitionTrace.single (runMonadic_refines_defaultAction userAgent message)

-- ============================================================
-- § Projection helpers
-- ============================================================

private theorem replaceTraversable_pendingNavigationFinalizations
    (ua : UserAgent) (t : TopLevelTraversable) :
    (replaceTraversable ua t).pendingNavigationFinalizations = ua.pendingNavigationFinalizations := by
  simp [replaceTraversable]

private theorem queueUpdateTheRendering_pendingNavigationFinalizations
    (ua : UserAgent) (traversableId : Nat) :
    (queueUpdateTheRendering ua traversableId).1.pendingNavigationFinalizations =
    ua.pendingNavigationFinalizations := by
  simp only [queueUpdateTheRendering, Id.run]
  -- split on the four nested option matches; the "some" arm comes first each time
  split
  · -- some traversable
    split
    · -- some document
      split
      · -- some documentId
        split
        · -- some eventLoop: success branch; only eventLoops and topLevelTraversableSet may change
          simp only [UserAgent.setEventLoop]
          split_ifs
          · simp [replaceTraversable]
          · rfl
        · simp  -- none eventLoop
      · simp  -- none documentId
    · simp  -- none document
  · simp  -- none traversable

-- ============================================================
-- § takePendingNavigationFinalization always erases the entry
-- ============================================================

/-- `takePendingNavigationFinalization` atomically removes the pending finalization record
for `documentId`, regardless of whether one existed. -/
theorem takePendingNavigationFinalization_erases
    (ua : UserAgent) (documentId : Nat) :
    ((ua.takePendingNavigationFinalization documentId).1).pendingNavigationFinalizations.get? documentId = none := by
  simp [UserAgent.takePendingNavigationFinalization, UserAgent.pendingNavigationFinalization?]

-- ============================================================
-- § Simp lemma: runMonadic (.finalizeNavigation …) unfolds to finalizeNavigationM
-- ============================================================

/-- `handleUserAgentTaskMessage` dispatches `.finalizeNavigation` to `finalizeNavigationM`. -/
theorem handleUserAgentTaskMessage_finalizeNavigation
    (documentId : Nat) (url : String) :
    handleUserAgentTaskMessage (.finalizeNavigation documentId url) =
    (finalizeNavigationM documentId url : M Unit) := by rfl

/-- Reduce the result of processing `.finalizeNavigation` to the concrete monadic computation. -/
theorem runMonadic_finalizeNavigation
    (ua : UserAgent) (documentId : Nat) (url : String) :
    runMonadic ua (.finalizeNavigation documentId url) =
    (fun p => (p.1.2, p.2)) ((finalizeNavigationM documentId url).run ua) := by
  simp only [runMonadic, handleUserAgentTaskMessage]
  rcases (finalizeNavigationM documentId url).run ua with ⟨⟨⟨⟩, effects⟩, nextUserAgent⟩
  rfl

-- ============================================================
-- § Postcondition: finalizeNavigation always consumes the pending finalization
-- ============================================================

/-- **Postcondition**: after handling `.finalizeNavigation documentId url`, the pending
navigation finalization record for `documentId` is unconditionally consumed.

The handler calls `takePendingNavigationFinalization documentId` before any conditional
branch, so the entry is erased regardless of whether the finalization ultimately succeeds. -/
theorem finalizeNavigation_pending_consumed
    (ua : UserAgent) (documentId : Nat) (url : String) :
    ((runMonadic ua (.finalizeNavigation documentId url)).2).pendingNavigationFinalizations.get? documentId = none := by
  -- TODO: Complete the proof by unfolding the WriterT/StateM computation and case-splitting
  -- on the option branches in finalizeNavigationM. Each branch sets state derived from
  -- (ua.takePendingNavigationFinalization documentId).1, which has the entry erased.
  sorry

-- ============================================================
-- § Invariant: no dangling navigation IDs
-- ============================================================

/-- For every traversable whose `ongoingNavigation` holds a navigation ID, there is
either a pending navigation fetch (fetch still in progress) or a pending navigation
finalization (fetch done, awaiting content commit), but not both. -/
def noDanglingNavigationIds (ua : UserAgent) : Prop :=
  ∀ (traversableId : Nat) (traversable : TopLevelTraversable) (navigationId : Nat),
    traversable? ua traversableId = some traversable →
    traversable.toTraversableNavigable.toNavigable.ongoingNavigation =
      some (.navigationId navigationId) →
    (ua.pendingNavigationFetches.get? navigationId).isSome ∨
    (ua.pendingNavigationFinalizationIdsByNavigationId.get? navigationId).isSome

/-- **Base case**: the default user agent satisfies `noDanglingNavigationIds` vacuously,
since it has no traversables. -/
theorem noDanglingNavigationIds_default :
    noDanglingNavigationIds default := by
  intro traversableId traversable navigationId h_trav _
  -- The default traversable set has no members, so the lookup returns none.
  simp only [traversable?, TopLevelTraversableSet.find?] at h_trav
  simp [default, Inhabited.default] at h_trav

/-- **Invariant step for `finalizeNavigation`**: `noDanglingNavigationIds` is preserved
by the `.finalizeNavigation` handler. -/
theorem noDanglingNavigationIds_finalizeNavigation
    (ua : UserAgent) (documentId : Nat) (url : String) :
    noDanglingNavigationIds ua →
    noDanglingNavigationIds (runMonadic ua (.finalizeNavigation documentId url)).2 := by
  intro h_inv
  -- TODO: Complete by case-splitting on the finalizeNavigation branches.
  -- The key cases: in the success branch, the affected traversable's ongoingNavigation is
  -- cleared, so no ongoing navigationId remains for that traversable. For all other
  -- traversables and navigation IDs, the maps are unchanged relative to ua.
  sorry

/-- **Invariant induction**: `noDanglingNavigationIds` is an inductive invariant of
`userAgentLTS`. The base case is proved; the step case is established for
`finalizeNavigation` and deferred for the remaining message variants. -/
theorem noDanglingNavigationIds_inductive :
    TransitionSystem.Inductive userAgentLTS (noDanglingNavigationIds) := by
  constructor
  · intro ua h_init
    simp [userAgentLTS] at h_init
    subst h_init
    exact noDanglingNavigationIds_default
  · intro ua action ua' h_inv h_trans
    simp [userAgentLTS] at h_trans
    obtain ⟨_, h_post⟩ := h_trans
    -- TODO: Dispatch on the action message and apply the per-message invariant steps.
    sorry

/-- **Precondition** (derived from `noDanglingNavigationIds`): if the invariant holds and
a traversable has ongoing navigation `navigationId`, then the pending navigation fetch and
pending navigation finalization for that ID are mutually exclusive — at most one is present. -/
theorem noDanglingNavigationIds_exclusive
    (ua : UserAgent)
    (h_inv : noDanglingNavigationIds ua)
    (traversableId : Nat)
    (traversable : TopLevelTraversable)
    (navigationId : Nat)
    (h_trav : traversable? ua traversableId = some traversable)
    (h_ongoing : traversable.toTraversableNavigable.toNavigable.ongoingNavigation =
        some (.navigationId navigationId))
    (h_both : (ua.pendingNavigationFetches.get? navigationId).isSome ∧
              (ua.pendingNavigationFinalizationIdsByNavigationId.get? navigationId).isSome) :
    False := by
  -- TODO: Prove mutual exclusion from the handler structure; requires showing
  -- that fetchCompleted removes the pending fetch before inserting the finalization.
  sorry

end FormalWeb
