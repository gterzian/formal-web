# pi-share-hf

This extension is based on https://github.com/badlogic/pi-share-hf. It collects and archives pi sessions locally without the upload/review flow.

The original repo did not come with a license at the time it was downloaded, so the best I can do is add this attribution note.

## Behavior

- **Auto-collection on shutdown:** When pi shuts down, the current session is automatically archived to `.pi/collected-sessions/`.
- **Each session gets its own file:** Every collection (manual or automatic) produces a unique file named `<session-stem>_<timestamp>_<random>.jsonl`, even when collected multiple times against the same pi session file.
- **Manual collection:** Use `/collect-session` (interactive) or the `collect_session` tool (LLM-accessible) to archive at any time.
- **Upload:** The `upload_session` tool is a stub and not yet implemented.