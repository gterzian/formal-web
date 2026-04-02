namespace TransitionSystem

structure LTS (State : Type) (Action : Type) where
  init : State → Prop
  trans : State → Action → State → Prop

variable {State Action : Type}

inductive Reachable (sys : LTS State Action) : State → Prop where
  | init : ∀ s, sys.init s → Reachable sys s
  | step : ∀ s a s', Reachable sys s → sys.trans s a s' → Reachable sys s'

def InvariantHolds (sys : LTS State Action) (inv : State → Prop) : Prop :=
  ∀ s, Reachable sys s → inv s

def Safety (sys : LTS State Action) (bad : State → Prop) : Prop :=
  ∀ s, Reachable sys s → ¬ bad s

def Liveness (sys : LTS State Action) (good : State → Prop) : Prop :=
  ∀ s, Reachable sys s → ∃ s', Reachable sys s' ∧ good s'

def Deterministic (sys : LTS State Action) : Prop :=
  ∀ s a s₁ s₂, sys.trans s a s₁ → sys.trans s a s₂ → s₁ = s₂

def Enabled (sys : LTS State Action) (s : State) (a : Action) : Prop :=
  ∃ s', sys.trans s a s'

def Terminal (sys : LTS State Action) (s : State) : Prop :=
  ∀ a, ¬ Enabled sys s a

structure Inductive (sys : LTS State Action) (inv : State → Prop) : Prop where
  base : ∀ s, sys.init s → inv s
  step : ∀ s a s', inv s → sys.trans s a s' → inv s'

inductive TransitionTrace (sys : LTS State Action) : State → List Action → State → Prop where
  | nil (state : State) : TransitionTrace sys state [] state
  | cons
      {start intermediate finish : State}
      {action : Action}
      {actions : List Action}
      (hstep : sys.trans start action intermediate)
      (htrace : TransitionTrace sys intermediate actions finish) :
      TransitionTrace sys start (action :: actions) finish

namespace TransitionTrace

theorem single
    {sys : LTS State Action}
    {start finish : State}
    {action : Action}
    (hstep : sys.trans start action finish) :
    TransitionTrace sys start [action] finish :=
  .cons hstep (.nil finish)

theorem append
    {sys : LTS State Action}
    {start intermediate finish : State}
    {actions₁ actions₂ : List Action}
    (hleft : TransitionTrace sys start actions₁ intermediate)
    (hright : TransitionTrace sys intermediate actions₂ finish) :
    TransitionTrace sys start (actions₁ ++ actions₂) finish := by
  induction hleft with
  | nil _ =>
      simpa using hright
  | @cons start intermediate next action actions hstep htrace ih =>
      simpa using TransitionTrace.cons hstep (ih hright)

end TransitionTrace

theorem inductive_implies_invariant (sys : LTS State Action) (inv : State → Prop) :
    Inductive sys inv → InvariantHolds sys inv := by
  intro h s hReach
  induction hReach with
  | init s hInit =>
      exact h.base s hInit
  | step s a s' _ hTrans ih =>
      exact h.step s a s' ih hTrans

theorem invariant_induction (sys : LTS State Action) (inv : State → Prop)
    (hInit : ∀ s, sys.init s → inv s)
    (hStep : ∀ s a s', inv s → sys.trans s a s' → inv s') :
    InvariantHolds sys inv :=
  inductive_implies_invariant sys inv ⟨hInit, hStep⟩

theorem conjunction_preserved (sys : LTS State Action) (inv₁ inv₂ : State → Prop) :
    InvariantHolds sys inv₁ →
    InvariantHolds sys inv₂ →
    InvariantHolds sys (fun s => inv₁ s ∧ inv₂ s) :=
  fun h₁ h₂ s hReach => ⟨h₁ s hReach, h₂ s hReach⟩

theorem terminal_no_transitions (sys : LTS State Action) (s : State) :
    Terminal sys s → ∀ a s', ¬ sys.trans s a s' := by
  intro hTerm a s' hTrans
  exact hTerm a ⟨s', hTrans⟩

section Simulation

variable {State₁ State₂ : Type}

structure Simulation (sys₁ : LTS State₁ Action) (sys₂ : LTS State₂ Action)
    (R : State₁ → State₂ → Prop) : Prop where
  init : ∀ s₁, sys₁.init s₁ → ∃ s₂, sys₂.init s₂ ∧ R s₁ s₂
  step : ∀ s₁ s₂ a s₁', R s₁ s₂ → sys₁.trans s₁ a s₁' →
    ∃ s₂', sys₂.trans s₂ a s₂' ∧ R s₁' s₂'

def Bisimulation (sys₁ : LTS State₁ Action) (sys₂ : LTS State₂ Action)
    (R : State₁ → State₂ → Prop) : Prop :=
  Simulation sys₁ sys₂ R ∧ Simulation sys₂ sys₁ (fun s₂ s₁ => R s₁ s₂)

end Simulation

section Product

variable {State₁ State₂ : Type}

def product (sys₁ : LTS State₁ Action) (sys₂ : LTS State₂ Action) :
    LTS (State₁ × State₂) Action where
  init := fun (s₁, s₂) => sys₁.init s₁ ∧ sys₂.init s₂
  trans := fun (s₁, s₂) a (s₁', s₂') => sys₁.trans s₁ a s₁' ∧ sys₂.trans s₂ a s₂'

theorem product_deterministic (sys₁ : LTS State₁ Action) (sys₂ : LTS State₂ Action)
    (h₁ : Deterministic sys₁) (h₂ : Deterministic sys₂) :
    Deterministic (product sys₁ sys₂) := by
  intro ⟨s₁, s₂⟩ a ⟨s₁', s₂'⟩ ⟨s₁'', s₂''⟩ ⟨ht₁, ht₂⟩ ⟨ht₁', ht₂'⟩
  exact Prod.mk.injEq .. |>.mpr ⟨h₁ _ _ _ _ ht₁ ht₁', h₂ _ _ _ _ ht₂ ht₂'⟩

end Product

section Enumeration

structure FiniteEnumeration (α : Type) where
  all : List α
  complete : ∀ a : α, a ∈ all

def FiniteEnumeration.size {α : Type} (enum : FiniteEnumeration α) : Nat :=
  enum.all.length

theorem FiniteEnumeration.mem_all {α : Type} (enum : FiniteEnumeration α) (a : α) :
    a ∈ enum.all :=
  enum.complete a

end Enumeration

section SafetyCombinators

theorem safety_from_invariant (sys : LTS State Action) (inv bad : State → Prop)
    (hInv : InvariantHolds sys inv)
    (hDisjoint : ∀ s, inv s → ¬ bad s) :
    Safety sys bad :=
  fun s hReach => hDisjoint s (hInv s hReach)

theorem safety_by_induction (sys : LTS State Action) (bad : State → Prop)
    (hInit : ∀ s, sys.init s → ¬ bad s)
    (hStep : ∀ s a s', ¬ bad s → sys.trans s a s' → ¬ bad s') :
    Safety sys bad := by
  intro s hReach
  induction hReach with
  | init s h =>
      exact hInit s h
  | step s a s' _ hTrans ih =>
      exact hStep s a s' ih hTrans

theorem disjunction_preserved (sys : LTS State Action) (inv₁ inv₂ : State → Prop)
    (h₁ : ∀ s, sys.init s → inv₁ s ∨ inv₂ s)
    (h₂ : ∀ s a s', (inv₁ s ∨ inv₂ s) → sys.trans s a s' → inv₁ s' ∨ inv₂ s') :
    InvariantHolds sys (fun s => inv₁ s ∨ inv₂ s) :=
  invariant_induction sys (fun s => inv₁ s ∨ inv₂ s) h₁ h₂

theorem invariant_weakening (sys : LTS State Action) (inv₁ inv₂ : State → Prop)
    (hImpl : ∀ s, inv₁ s → inv₂ s)
    (hInv : InvariantHolds sys inv₁) :
    InvariantHolds sys inv₂ :=
  fun s hReach => hImpl s (hInv s hReach)

end SafetyCombinators

end TransitionSystem