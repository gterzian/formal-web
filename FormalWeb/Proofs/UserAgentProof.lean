import FormalWeb.UserAgent
import FormalWeb.Proofs.TransitionSystem

namespace FormalWeb

/-- The user-agent LTS labels each step by the task message that `runMonadic` handles. -/
abbrev UserAgentAction := UserAgentTaskMessage

/-- Relational LTS for user-agent task-message handling. -/
def userAgentLTS : TransitionSystem.LTS UserAgent UserAgentAction where
  init := fun userAgent => userAgent = default
  trans := fun userAgent action userAgent' =>
    userAgent' = (runMonadic userAgent action).2

def interpretUserAgentAction
    (userAgent : UserAgent)
    (action : UserAgentAction) :
    List UserAgentEffect :=
  (runMonadic userAgent action).1.toList

theorem runMonadic_trace
    (userAgent : UserAgent)
    (action : UserAgentAction) :
    TransitionSystem.TransitionTrace
      userAgentLTS
      userAgent
      [action]
      (runMonadic userAgent action).2 := by
  exact TransitionSystem.TransitionTrace.single rfl

theorem interpretUserAgentAction_eq_runMonadic
    (userAgent : UserAgent)
    (action : UserAgentAction) :
    interpretUserAgentAction userAgent action = (runMonadic userAgent action).1.toList := by
  rfl

theorem runNextEventLoopTask_trace
    (userAgent : UserAgent)
    (eventLoopId : Nat) :
    TransitionSystem.TransitionTrace
      userAgentLTS
      userAgent
      [.runNextEventLoopTask eventLoopId]
      (runMonadic userAgent (.runNextEventLoopTask eventLoopId)).2 := by
  simpa using runMonadic_trace userAgent (.runNextEventLoopTask eventLoopId)

end FormalWeb
