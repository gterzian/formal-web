This folder should contain the proof that accompany the modules in the level above.

For refinement proofs over WriterT/state helpers, prefer short projection lemmas and direct `simp` proofs over manually expanded `((match ...).run state)` terms. When nested `match` binders remain necessary, add explicit binder types to reduce elaboration cost.