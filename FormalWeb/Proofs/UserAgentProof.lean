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

end FormalWeb
