# Documentation Chain

Read repository documentation from general to specific:
1. `AGENTS.md` (top-level)
3. `<subdir>/{<nested-sub-dir/}README.md`

The above should form a chain of readmes, based on the directories where you are working for the task at hand. Essentially, when you are reading or writing to a file, you must take into account all readmes in the path to that file.

You can also update this documentation chain based on lessons learned from user feedback: update the lowest-level file that owns the rule or pattern, and avoid repeating the same guidance in multiple places.

# Local Extensions

## pi-share-hf — Session Collection

The `.pi/extensions/pi-share-hf/` extension provides:

- **`collect_session` tool** — Used by the agent at end of task to archive the current pi session to `.pi/collected-sessions/`. Does not upload or share session data.
- **`/collect-session` command** — Interactive equivalent of the above.
- **`upload_session` tool** — Stub only; not yet implemented. Will eventually upload collected sessions to a remote destination (e.g. Hugging Face dataset).

# Documentation Style

- Describe current architecture and behavior; keep task history out of repository docs.
- Keep README guidance general and durable; one-off implementation details belong in source or tests, not in repository docs.
- Use neutral, factual language.
- Check `web_standards/` before fetching standards text from the network.
- Treat `vendor/` and vendored WPT resources as read-only unless the task explicitly requires vendor changes.

# Cross-Cutting Rules

- Model cross-component browser identities such as documents, browsing contexts, navigables, traversables, event loops, and related handles as UUID newtypes in `ipc_messages`; reserve raw integers for process-local sequencing or transport details.
- Build and launch the dedicated `formal-web-content` and `formal-web-net` sidecars from the `content` and `net` packages for the selected profile together with the main `formal-web` binary.
- Keep large paint payloads on shared-memory transport and keep typed IPC messages focused on metadata and handles.
- Track navigation completion with explicit content-to-embedder commit signaling instead of inferring it from paint delivery.
- Keep verification artifacts in temporary directories, and use the local TLA+ Toolbox jar at `/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar`.
- Put plans and temporary task notes under `scratchpad/`.

At the end of each task, you MUST
- Collect the current session by invoking the `collect_session` tool (or running `/collect-session`) to archive the session trace to `.pi/collected-sessions/`.
- Finish tasks with the default WPT run, and `./verification/verify-navigation.sh`.
- Treat unexpected results in the above as something that needs to be fixed as part of the current task.
- Suggest a commit message for the completed task to the user, if the task involved changes tracked by git.