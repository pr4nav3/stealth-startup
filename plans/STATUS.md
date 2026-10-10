# Supervisor Work Tracker

Living log of what is done and what is left. Update this file when an item
moves, rather than appending a second tracker.

Last updated: 2026-10-07 (`main` at `4971f9a`)

Product OS targets: **macOS and Windows**. Linux is a workshop only, not a V1
ship target. There is no Mac on hand, so Windows is the development and
acceptance machine. macOS must not be broken in code; it cannot be fully
re-verified until a Mac is available again.

Kokoro starter generation is out. Starter clips, if we ship them, come from the
same Chatterbox-Nano worker users already use.

New work branches from `main`. The old worktrees are removed.

## Done

- [x] Local-first Tauri 2 shell (vanilla TypeScript UI, no React)
- [x] Session UI: session, settings, history report, trends, phrase library, overlay, tray
- [x] SQLite history, rules, and settings. A bad database is reset. A second copy of the app is refused. A failed save does not continue in a temporary database
- [x] Focus scored by time actually spent. Alt-Tab, Start, and Search are ignored. Supervisor is recognized by its process
- [x] Windows capture through the Microsoft Windows crate. macOS capture code is present and untested on a Mac
- [x] Custom voice path: model download, mic record in Rust, out-of-process generation, cached WAV playback
- [x] Voice runtime install checks the zip and can roll back. Manifest builder exists
- [x] Session accounting: a switch keeps time on the previous app, a capture gap does not stretch the last app, history delete cannot remove the live session
- [x] Voice settings check model size only. Cancel keeps verified model files. Generation stops after 30 minutes
- [x] Webcam tracker in Settings, with a GPU toggle. Frames are not stored
- [x] Chrome extension removed

Not done even if the UI implies it: starter voices have no real audio.

## Next branches

Start each from `main`. Merge one back before the next branch edits the same screens.

### 1. `feat/ui-polish`

Make the UI easier to use, rearrange the screens, and add dark mode.
Style direction: `plans/SUPERVISOR_UI_GUIDELINES.md`.

### 2. `feat/voice-path`

Finish the voice path. Add a generation screen where the user can generate again until a sample sounds right. The model is small, so some samples come out garbled. Set the RAM check so a normal machine is not locked out.

### 3. `feat/history`

History UI changes. Change a couple of things the session tracks. A PDF for each month's focus, and a weekly recap that shows up in History.

### 4. `feat/webcam-session`

Make the webcam models work in the app so they can be tested there. Control how webcam strictness sits alongside the voice nudges.
Signals: `plans/webcam-tracking.md`.

## Outside those branches

### Before the branches

No code has to land first. `main` is the base.

### After the branches, or beside them as bugfix-only work

These are not the four features. Do them as small fixes, not as a fifth product branch.

- Starter voices are listed as ready and have no clips. A nudge only plays a custom voice. The fallback beep opens an audio context it never closes
- The screen rebuilds on a timer, so typing in a field can be wiped
- A window-title change restarts the distraction timer
- "Forget app names" clears saved titles only. The live session can write the title back, and the app name stays
- A recording can grow without a size cap, and a failed generation can leave a half-written profile

### Ship, after the app behavior is settled

- [ ] Dev voice path on Windows: `.venv-voice-runtime-windows` launches the worker
- [ ] Prod voice path: download the frozen zip, generate, play, delete, regenerate
- [ ] GitHub Release for the macOS ARM64 zip (SHA-256 `fcfd9bc86932f16b53449b7b6e29ab108b9ba9f23aec8d1af5333fe4b1f79ae5`) and the Windows x64 zip (SHA-256 `69bace321f9f5f8f6c6fbec3c4dc20a50f109ffa60ed9fdd5c19f76f26ec9b0a`). Do not commit the zips
- [ ] Runtime manifest with both targets, and `SUPERVISOR_VOICE_PACK_MANIFEST_URL`
- [ ] Windows installer, and a decision on code signing
- [ ] Mac checklist: `plans/mac-handoff.md`

## Test gate

Every shipped item has to work as intended on **Windows**, and must not break
**macOS** code paths. Windows is the machine that can actually prove it.

- Run `cargo test`, `npm run check`, and the relevant Windows check
- Keep macOS-only paths intact
- Re-smoke Mac when hardware exists; do not claim Mac end-to-end from Windows
