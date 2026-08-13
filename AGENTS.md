# Supervisor Agent Rules

Ponytail rules apply here. The local checkout is at `vendor/ponytail`; read `vendor/ponytail/AGENTS.md` before making non-trivial changes.

Supervisor-specific defaults:

- Keep the app local-first: no telemetry, analytics, remote fonts, or cloud processing.
- Prefer OS/window metadata before browser contents, screenshots, OCR, or AI.
- Keep voice generation out of the main app process; focus mode should play cached audio only.
- Add no new dependency unless the current phase needs it.
- Leave one runnable check for non-trivial logic.

