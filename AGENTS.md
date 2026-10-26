# Supervisor Agent Rules

Ponytail applies on coding work: laziest solution that actually works, stdlib and native features first, no extra dependency unless this phase needs it, one runnable check for non-trivial logic. Off: "stop ponytail" / "normal mode".

Claude Code loads the ponytail skills from `.claude/skills/` (committed, so worktree sessions get them too). Cursor loads its own copy from the machine. Other tools only see this file. To update, re-copy `skills/*/SKILL.md` from the ponytail repo into `.claude/skills/`.

Supervisor-specific defaults:

- Keep the app local-first: no telemetry, analytics, remote fonts, or cloud processing.
- Prefer OS/window metadata before browser contents, screenshots, OCR, or AI.
- Keep voice generation out of the main app process; focus mode should play cached audio only.
- Add no new dependency unless the current phase needs it.
- Leave one runnable check for non-trivial logic.
- Words users see are plain, never internal names. Say line, make, make again (not clip, generate, regenerate, phrase); voice engine (not model, pack, runtime, worker); recording (not reference); Allow once / Always allow (not exception, Not today, Never a distraction); start screen (not preflight); graphics card (not GPU). Code identifiers keep their names.

Baseline branch: `app-v2-simplified-rules` (`main` is an ancestor; fast-forward it when ready). Tracker: `plans/STATUS.md`. Agent starter bugs: `plans/known-issues.md`. Mac work: `plans/mac-handoff.md`.
