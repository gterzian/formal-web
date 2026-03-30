namespace FormalWeb

inductive TransitionTrace
    (stepFn : σ → α → Option σ) :
    σ → List α → σ → Prop where
  | nil (state : σ) : TransitionTrace stepFn state [] state
  | cons
      {start intermediate finish : σ}
      {action : α}
      {actions : List α} :
      stepFn start action = some intermediate →
      TransitionTrace stepFn intermediate actions finish →
      TransitionTrace stepFn start (action :: actions) finish

namespace TransitionTrace

theorem single
    {stepFn : σ → α → Option σ}
    {start finish : σ}
    {action : α}
    (h : stepFn start action = some finish) :
    TransitionTrace stepFn start [action] finish :=
  .cons h (.nil finish)

theorem append
    {stepFn : σ → α → Option σ}
    {start middle finish : σ}
    {xs ys : List α}
    (hxs : TransitionTrace stepFn start xs middle)
    (hys : TransitionTrace stepFn middle ys finish) :
    TransitionTrace stepFn start (xs ++ ys) finish := by
  induction hxs with
  | nil state =>
      simpa using hys
  | @cons start intermediate finish action actions hstep htrace ih =>
      simp
      exact TransitionTrace.cons hstep (ih hys)

end TransitionTrace

end FormalWeb