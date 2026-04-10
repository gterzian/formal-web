This folder should contain the proof that accompany the modules in the level above.

For refinement proofs over WriterT/state helpers, prefer short projection lemmas and direct `simp` proofs over manually expanded `((match ...).run state)` terms. When nested `match` binders remain necessary, add explicit binder types to reduce elaboration cost.

For `FormalWeb.M`, prefer tiny `WriterT.run` normalization lemmas such as bind/get/set/tell/pure equations proved by `rfl`, then rewrite handler executions with those lemmas before splitting on model cases.

For task-message refinement proofs, align LTS actions with message branches rather than individual emitted effects, and interpret each action back to the emitted effect list; one action may account for multiple effects or none.