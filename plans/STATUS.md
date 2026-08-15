# Supervisor Work Tracker

Living log of what is done and what is left. Update this file when a item
moves, rather than appending a second tracker.

Last updated: 2026-08-15 (webcam tracking added)

Product OS targets: **macOS and Windows**. Linux is a workshop only, not a V1
ship target. There is no Mac on hand anymore, so Windows is the development and
acceptance machine. macOS must not be broken in code; it cannot be fully
re-verified locally until a Mac is available again.

Kokoro starter generation is out. Starter clips, if we ship them, come from the
same Chatterbox-Nano worker users already use.

## Done

These are in the repo and were verified on the last Mac, except where noted.

- [x] Local-first Tauri 2 shell (vanilla TypeScript UI, no React)
- [x] Activity capture for macOS and Windows (`active-win-pos-rs`)
- [x] Focus engine with cooldowns, idle skip, no instant nudge on app switch
- [x] Session start / pause / snooze / stop / mark relevant / mark distracting
- [x] Session, Settings, History screens plus compact overlay and tray
- [x] Chrome MV3 localhost tab bridge (background relay only; no popup UI yet)
- [x] Custom voice path: model download, mic record in Rust, out-of-process
      generation, cached WAV playback
- [x] Frozen **macOS ARM64** voice runtime zip, SHA-256 verified
- [x] Mac zip sitting in this checkout as
      `supervisor-voice-runtime-macos-arm64.zip` (not in Git)

Not done even if the UI implies it: starter voices have no real audio.
Rules, settings, history, and selected voice are in-memory only. Voice
reference files and generated clips already live under app data.

## Open

Status key: `not started` · `blocked` · `in progress` · `done`

### 1. Windows app + Windows voice runtime

Status: **not started** · owner machine: Windows

- [ ] Run the app on Windows: session, overlay pin/unpin, snooze, first-click
      behavior, Chrome enrichment
- [ ] Prove Win32 capture edge cases (empty titles, UWP host, idle, protected
      processes, fullscreen / UAC)
- [ ] Build `supervisor-voice-runtime-windows-x64.zip` on Windows with
      `scripts/build_voice_runtime.py --target windows-x64`
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

### 4. Local SQLite persistence

Status: **not started** (nothing in the repo yet; voice files already persist
on disk)

Keep it local. No cloud.

- [ ] Persist rules, settings, selected voice, and session history
- [ ] Survive app restart
- [ ] Deletion paths: delete a voice, delete clips, delete history
- [ ] One runnable check around load/save

### 5. UI overhaul

Status: **later, not started** · direction still to be confirmed

Reference vibe: BrainDock-ish, simpler, less clutter, more intuitive. Style
notes already live in `plans/supervisor-style-guide-braindock-ish.md` and
`plans/supervisor-style-guide-mashup.md`. Do not start this until the Windows
loop in item 1 is usable.

### 6. Web extension popup

Status: **not started**

Today the Chrome extension only relays the active tab in the background.

- [ ] Small popup when the toolbar icon is clicked
- [ ] Show 1–2 simple facts (for example current window / tab)
- [ ] State that all data stays local
- [ ] Direct people to the Supervisor app for everything else

### 7. GitHub Releases website

Status: **not started**

- [ ] A public page that lists GitHub Releases (voice packs, later app builds)
- [ ] No telemetry, no account system, no cloud processing pitch

### 8. Webcam presence / phone tracking

Status: **not started** · can bake off in parallel · do not wire into the app
until the detector is boringly reliable

V1 explicitly deferred camera. Style mocks already show `phone reaches` /
`phone hits` as a metric. Approach: `plans/webcam-tracking.md`.

- [ ] Offline bakeoff: local tiny models, 1–2 fps, discrete events only
- [ ] Prove three simple signals: in-seat, looking away, phone visible
- [ ] Compose “on phone / reaching” from those, not a custom action model
- [ ] App integration later: optional, session-only, off by default
- [ ] Same focus-engine cooldowns; never store or upload frames

## Test gate

Every shipped item has to work as intended on **Windows**, and must not break
**macOS** code paths. Windows is the machine that can actually prove it.

Until a Mac is available again:

- Run `cargo test`, `npm run check`, and the relevant Windows E2E on Windows
- Keep macOS-only branches (`Info.plist`, Screen Recording fallback,
  `acceptFirstMouse`) intact
- Re-smoke Mac when hardware exists; do not claim Mac E2E from Windows

## Suggested order

1 → 2 → 4 can overlap slightly once the Windows app runs → 3 → 6 → 7 → 5.

Item 8 bakeoff can run **beside** 1–4 as a separate script. App wiring waits
until the detector works, and until the Windows voice loop is usable.

Item 5 stays last until the UI direction is confirmed.
