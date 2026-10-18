# Supervisor Agent Rules

Ponytail applies on coding work: laziest solution that actually works, stdlib and native features first, no extra dependency unless this phase needs it, one runnable check for non-trivial logic. Off: "stop ponytail" / "normal mode".

This repo does not vendor ponytail. Cursor loads the full skill from the machine. Codex, Claude Code, and other tools only see this file unless ponytail is copied into that tool.

Supervisor-specific defaults:

- Keep the app local-first: no telemetry, analytics, remote fonts, or cloud processing.
- Prefer OS/window metadata before browser contents, screenshots, OCR, or AI.
- Keep voice generation out of the main app process; focus mode should play cached audio only.
- Add no new dependency unless the current phase needs it.
- Leave one runnable check for non-trivial logic.

Baseline branch: `feat/window-supervision`. Tracker: `plans/STATUS.md`. Agent starter bugs: `plans/known-issues.md`.
