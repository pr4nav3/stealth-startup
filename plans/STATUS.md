# Supervisor Work Tracker

Living log of what is done and what is left. Update this file when an item
moves, rather than appending a second tracker.

Last updated: 2026-10-17 (`main` at `68561c8`; working tree dirty)

Product OS targets: **macOS and Windows**. Linux is a workshop only, not a V1
ship target. There is no Mac on hand, so Windows is the development and
acceptance machine. macOS must not be broken in code; it cannot be fully
re-verified until a Mac is available again.

Kokoro starter generation is out. Starter clips, if we ship them, come from the
same Chatterbox-Nano worker users already use.

Treat **`feat/window-supervision` as the baseline.** Window targeting,
attribution, and history storage changes happen on this branch. Committed
`main` is behind the working tree.

New work still branches from `main` once this lands. The old worktrees are
gone. These local branch names are fully inside `main` and must not be merged
again: `feat/sqlite`, `feat/ui`, `feat/voice-runtime-hardening`,
`feat/webcam-models`, `feat/win32-capture`, `test-features`.
`feat/window-supervision` is the same SHA as `main`; the new code is
uncommitted and mixed with leftover voice-worker edits.

## On `main`

- [x] Local-first Tauri 2 shell (vanilla TypeScript UI, no React)
- [x] Session UI overhaul (home, preflight, session, settings, history, overlay, tray)
- [x] SQLite history, rules, and settings. A bad database is reset. A second copy of the app is refused. A failed save does not continue in a temporary database
- [x] Focus scored by time actually spent. Alt-Tab, Start, and Search are ignored. Supervisor is recognized by its process
- [x] Windows capture through the Microsoft Windows crate. macOS capture code is present and untested on a Mac
- [x] Custom voice path: model download, mic record in Rust, out-of-process generation, cached WAV playback
- [x] Voice runtime install checks the zip and can roll back. Manifest builder exists
- [x] Session accounting: a switch keeps time on the previous app, a capture gap does not stretch the last app, history delete cannot remove the live session
- [x] Voice settings check model size only. Cancel keeps verified model files. Generation stops after 30 minutes
- [x] Voice RAM gate and a regenerate / try-again control on clips
- [x] Starter *reference* WAVs (`derek`, `elise`) ship in the app. Phrase clips for those voices are not generated
- [x] Webcam tracker in Settings, with a GPU toggle. Frames are not stored
- [x] Webcam in sessions: phone spans, camera Away, phone nudges on the shared cooldown and the popup (`plans/webcam-tracking.md`)
- [x] Chrome extension removed
- [x] History week/month storage and recap UI. PDF export is unimplemented; its empty control was removed in the working tree

`808dab0` (UI overhaul) landed with a warning that app/website rule matching
may be broken. That is still the committed model. The rewrite is in the
working tree, not on `main`.

## Working tree (not committed)

Two features are mixed in one dirty tree. Do not commit them as one blob.

### Window supervision (split and commit first)

Implemented, Windows-probed, tests reported green. Browser time is one row per
website key, read from the address bar through UI Automation. Browser pages,
local pages, and Other websites are their own rows. Titles are a narrow
fallback and are never stored. Rules are app or website. Site names live in
`site_name`. macOS reads no address yet (`browser_macos.rs` stub).

Additional uncommitted naming pass: rule suggestion chips removed in favor of
entry guidance; recap has an Edit names mode for app and site rows. App names
are display overrides in schema v5. Windows skips an unidentifiable process
instead of storing its window class, filters the observed System32 ShellHost,
and asks the OS for packaged-app display names before using version metadata.
Verified on Windows: packaged-app lookup returned `WhatsApp` for the running
`WhatsApp.Root` process; `cargo test` passed 116 tests (3 manual tests ignored),
and `npm run check` passed. The recap naming UI still needs an in-app look.

Audit follow-up (2026-10-17): open-span checkpoints, rule persistence and identity
fixes, corrected timing/report gaps, and schema-v6 repair of cached stretch
metrics. Detailed findings, remaining release gaps, and the retention proposal:
`plans/window-history-audit.md`.

Check after commit: `cargo test`, `npm run check`, then
`cargo test manual_foreground_probe -- --ignored --nocapture` and
`cargo test manual_read_open_browser_windows -- --ignored --nocapture`.

### Persistent voice worker (park, then its own branch)

`voice.rs` / `chatterbox_nano_worker.py --serve` keep the model loaded, plus
related UI. This is leftover dirty work from before window supervision. Review
it after the supervision commit, on a separate branch.

## Next

Do not open `feat/history` or `feat/ui-polish` as new epics. Most of that
already landed on `main`.

1. Split window supervision out of the voice/worker files, commit it, smoke a
   live Chrome session and the rule editor, then it is `main`.
2. Move the persistent voice worker onto its own branch. Finish the voice path
   there: generation until a sample sounds right, RAM gate that a normal
   machine can pass, real starter phrase clips or stop listing starters as ready.
3. Small bugfixes below, not a fifth product branch.
4. ~~`feat/webcam-session`~~ done: see "Sessions" in `plans/webcam-tracking.md`.
5. Leftover UI: dark mode and ease-of-use. Style: `plans/SUPERVISOR_UI_GUIDELINES.md`.

## Bugfixes (beside the branches)

- Starter voices look ready and have no phrase clips. A nudge only plays a custom voice. The fallback beep opens an audio context it never closes
- A recording can grow without a size cap
- `install_runtime` returns if any compatible runtime exists, so a bugfix zip never installs
- Recent time after the last successful 15-second span checkpoint can be lost on a crash

Uncommitted voice code stages generation then replaces/merges, which covers the
old half-written-profile hole. That fix is not on `main` until the voice branch
lands.

## Ship, after the app behavior is settled

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
