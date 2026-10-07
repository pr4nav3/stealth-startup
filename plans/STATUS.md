# Supervisor Work Tracker

Living log of what is done and what is left. Update this file when a item
moves, rather than appending a second tracker.

Last updated: 2026-10-06 (session UI and temporary rules on `feat/ui`)

Product OS targets: **macOS and Windows**. Linux is a workshop only, not a V1
ship target. There is no Mac on hand anymore, so Windows is the development and
acceptance machine. macOS must not be broken in code; it cannot be fully
re-verified locally until a Mac is available again.

Kokoro starter generation is out. Starter clips, if we ship them, come from the
same Chatterbox-Nano worker users already use.

## Done

These are in the repo and were verified on the last Mac, except where noted.

- [x] Local-first Tauri 2 shell (vanilla TypeScript UI, no React)
- [x] Activity capture for macOS (`active-win-pos-rs`) and Windows (direct
      Win32 in `activity_win32.rs`)
- [x] Focus engine with cooldowns, idle skip, no instant nudge on app switch
- [x] Session start / pause / snooze / stop / mark relevant / mark distracting
- [x] Session, Settings, History screens plus compact overlay and tray
- [x] Custom voice path: model download, mic record in Rust, out-of-process
      generation, cached WAV playback
- [x] Frozen **macOS ARM64** voice runtime zip, SHA-256 verified
- [x] Mac zip sitting in this checkout as
      `supervisor-voice-runtime-macos-arm64.zip` (not in Git)

Not done even if the UI implies it: starter voices have no real audio.
Sessions, rules, settings, and selected voice persist in local SQLite
(`sessions.db` under app data). Voice reference files and generated clips
already live under app data.

## Open

Status key: `not started` · `blocked` · `in progress` · `done`

### 1. Windows app + Windows voice runtime

Status: **in progress** · owner machine: Windows

- [x] Run the app on Windows: session, overlay pin/unpin, snooze, first-click
      (Chrome extension deferred; native window titles are enough for V1)
- [x] Win32 capture edge cases: direct Win32 capture, UWP host resolution,
      protected-process fallback, cloaked/minimized skip, browser title
      suffix stripping, shell surfaces (Start, Search, Alt-Tab, taskbar,
      desktop) neutral without resetting drift, idle tick wraparound.
      Unit-tested; UAC, lock screen, and cloaked windows not live-probed yet
- [x] Build `supervisor-voice-runtime-windows-x64.zip` on Windows with
      `scripts/build_voice_runtime.py --target windows-x64`
      (local zip: `dist/supervisor-voice-runtime-windows-x64.zip`,
      SHA-256 `69bace321f9f5f8f6c6fbec3c4dc20a50f109ffa60ed9fdd5c19f76f26ec9b0a`,
      310736158 bytes; not in Git)
- [ ] Dev voice path: `.venv-voice-runtime-windows` launches the Python worker
- [ ] Prod voice path: app downloads the frozen zip, launches
      `supervisor-voice-worker.exe`, record → generate → play → delete →
      regenerate

This is the current critical path. Almost everything below waits on it.

### 2. GitHub Releases for voice packs

Status: **not started**

- [ ] Create a GitHub Release and upload the verified macOS ARM64 zip unchanged
      (SHA-256 must remain
      `fcfd9bc86932f16b53449b7b6e29ab108b9ba9f23aec8d1af5333fe4b1f79ae5`)
- [ ] Upload the Windows x64 zip after item 1 is proven
- [ ] Publish a runtime manifest with `macos-arm64` and `windows-x64` entries
- [ ] Point the app at that manifest (`SUPERVISOR_VOICE_PACK_MANIFEST_URL`)

Do not commit the zips to Git. They are ~295 MiB.

### 3. Starter voice pack generation

Status: **blocked on 1**

Do this **after** the Windows runtime works end-to-end, using the same prod
download/generate path a real user would use. That is intentional: starters
should not hide a broken runtime.

- [ ] Generate bundled starter clips with Chatterbox-Nano (not Kokoro)
- [ ] Ship them with the app, or hide starter playback until they exist
- [ ] Confirm a first-run session can nudge with a starter voice and no clone

### 4. Local SQLite persistence, session reports, trends

Status: **history UI on `feat/ui`** · design: `plans/session-storage.md`

Keep it local. No cloud. Session-only tracking; no always-on mode.

- [x] `rusqlite` bundled, migrations, `setting` + `rule` persistence
- [x] Span writer (merge on change, flush on close) + `session` / `span` /
      `event` tables
- [x] Orphan recovery on launch: `ended_at IS NULL` closes as `abandoned`
- [x] Rollups + `recompute_session_metrics`, history and report commands
      (`get_history` unchanged shape; new `get_session_report(id)`,
      `get_trends()`)
- [x] Session report screen: headline, timeline bar, category bars, top drifts
- [x] Trends above History: attention-span sparkline, focus % per day
- [x] Deletion paths: backend commands done for one session
      (`delete_session`), all history (`delete_all_history`), and window
      titles (`scrub_span_context`); History UI wired. Voice / clip deletion not
      done
- [x] One runnable check around rollup + reload + orphan recovery
- [ ] `phone` / `away` / `back` events and `away_seconds` / `phone_seconds`:
      schema ready, not emitted until webcam wiring

### 5. UI overhaul

Status: **next** · owner has a specific design in mind for the session
screen and preflight

The authoritative style direction is `plans/SUPERVISOR_UI_GUIDELINES.md`.
The older BrainDock-ish and mashup guides are historical references. Do not
rewrite the voice phrase lines; the owner is rewriting them. Leave the Settings
webcam preview section of `src/main.ts` alone while `feat/webcam-models` is open.

The report and trend screens in item 4 are the overhaul for the History tab
and the post-session view. Settle the styling before building them so they are
not built twice.

### 6. GitHub Releases website

Status: **not started**

- [ ] A public page that lists GitHub Releases (voice packs, later app builds)
- [ ] No telemetry, no account system, no cloud processing pitch

### 7. Webcam presence / phone tracking

Status: **in progress** · Settings live preview wired on Windows

Scope is now two signals only: **phone in frame** (the app will nudge
immediately) and **present / away** (away is logged once absent long enough).
Gaze and head pose are paused. Model work lives on `feat/webcam-models`
(not merged): YuNet + YOLO11n, presence = face OR person, hysteresis. Phone
recall is still weak (~0.63); next step is trying YOLO26 before any
fine-tuning. Signals and toggles: `plans/webcam-tracking.md`.

- [x] Offline bakeoff: YuNet + YOLOX-s, 4 fps, JSON labels
- [x] Prove: face in/out; when in frame, at PC / away from PC / at phone;
      phone in frame (script smoke on this Windows PC)
- [x] Settings: live frames, phone use on/off, strict vs not strict,
      alarm vs voice clip
- [ ] Session-time camera nudges (optional, off until a check is on)
- [ ] Wire webcam states into `session.rs` — today `webcam.rs` only feeds the
      Settings preview and `session.rs` has no webcam reference at all
- [ ] Package the detector as a frozen sidecar; it cannot ship as `.venv-webcam`
- [x] Never store or upload frames; a second of persistence before firing

Dev path uses `.venv-webcam` and `webcam_bakeoff/run_tracker.py --preview`.
Head pose is YuNet yaw/pitch ratios (solvePnP Euler wrapped to -170 on a
normal face and was reverted). Phone detect is YOLOX-s with a lower-center
square at 8 fps. Not packaged as a frozen sidecar yet.

### 8. Shippable Windows build

Status: **not started**

- [ ] Installer that a stranger can run
- [ ] Code signing, or a written answer for the SmartScreen warning an
      unsigned download will show

## Test gate

Every shipped item has to work as intended on **Windows**, and must not break
**macOS** code paths. Windows is the machine that can actually prove it.

Until a Mac is available again:

- Run `cargo test`, `npm run check`, and the relevant Windows E2E on Windows
- Keep macOS-only branches (`Info.plist`, Screen Recording fallback,
  `acceptFirstMouse`) intact
- Re-smoke Mac when hardware exists; do not claim Mac E2E from Windows

## Suggested order

1 → 2 → 3 are the ship blockers: the Windows voice production path has never
run end to end, and starter voices still have no audio, so a first-run user
who has not cloned a voice gets a silent nudge.

4 can overlap with 1–3 — it touches no voice code. Settle item 5's styling
before item 4's report screens. Then 6 → 8.

Item 7 bakeoff can run **beside** 1–4 as a separate script. App wiring waits
until the detector works, and until the Windows voice loop is usable.

Item 5 can start once session and preflight direction is settled.
