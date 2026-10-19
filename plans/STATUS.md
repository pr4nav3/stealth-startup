# Supervisor Work Tracker

Living log of what is done and what is left. Update this file when an item
moves, rather than appending a second tracker.

Last updated: 2026-10-19 (`main` at `dcc2336`; tree clean)

Product OS targets: **macOS and Windows**. Linux is a workshop only, not a V1
ship target. There is no Mac on hand, so Windows is the development and
acceptance machine. macOS must not be broken in code; it cannot be verified
until a Mac is available.

**`main` is the baseline.** Everything below is committed there. New work
branches from `main`; sessions started from the desktop app get their own
worktree under `.claude/worktrees/` (git-ignored) and merge back with a plain
merge. These local branches are fully inside `main` and must not be merged
again: `feat/window-supervision`, `feat/durable-app-identity`, `feat/sqlite`,
`feat/ui`, `feat/voice-runtime-hardening`, `feat/webcam-models`,
`feat/win32-capture`, `test-features`.

## On `main`

### Window supervision and history

- [x] Browser time is one row per website, read from the address bar (Windows,
      UI Automation). Browser pages, local pages and Other websites are their
      own rows. Titles are a narrow fallback and are never stored. Browsers
      themselves are always Neutral; only websites are classified
- [x] Durable app identity (schema v11): packaged-app id, exe + publisher, or
      bundle id, mapped to one key per app; display names kept separate
- [x] Every second of a session is recorded. Spans tile the session from Start
      to Stop: Supervisor's own windows record as focused, pauses as `paused`,
      sleep or a frozen poll loop as `unknown` ("Device asleep / unknown").
      A property test fuzzes random event sequences and fails on any hole
- [x] Mute keeps recording and only silences nudges; Pause is the untracked break
- [x] Focused-only score: Relevant time only. Neutral and unclassified time are
      shown apart. Classifying an app from a report (classify popup, one app at
      a time, keyboard 1/2/3) saves a rule and rescores past unclassified time
- [x] Session report: timeline, Apps & sites grouped by recorded decision,
      "now X" when today's rule differs. Weekly recap with daily bars,
      attention span and Recovery (time back on task after a drift). Trends
      page (focused hours, attention span, recovery, 8 weeks; unlocks after two
      full weeks). Monthly recap and category guesses removed
- [x] Day headers in History show that day's focused time and session count
- [x] Live-session polling repaints the clock and state in place; it never
      rebuilds under an open editor or a focused field
- [x] Getting started card on the home page: rules, clone a voice, webcam
      (optional), first 15-minute session; ticks itself off from real state,
      folds to one line on Hide, retires when the required steps are done

### Voice

- [x] Four phrase sets, five lines each: Gentle, Mom, Gen Z, Stern (`phraseBank`
      in `src/main.ts`). Phrase editor: add, edit, move between sets, delete
      (bundled phrases are hidden via `hidden_phrases`), new sets inline
- [x] Clips are generated one at a time, listened to, then kept or retried.
      Saving a recording is instant; clips come afterwards
- [x] Persistent `--serve` worker keeps the model loaded, releases after idle,
      is killed on app exit, survives a per-request exception, and reports the
      real reason when it dies at startup
- [x] RAM gate from a measured run: 4 GiB free, 7 GiB total
- [x] Mic recording capped at 60 s; stop timeout and device-ready wait fixed
- [x] Runtime zip reinstalls when the manifest version or checksum changes;
      "Check for updates" checks off the UI thread, shows a Checking phase,
      ends on "Up to date" when nothing is needed, and can be cancelled at any
      point including mid-hash
- [x] Voice nudges play the moment they are due, even with the window
      minimized (backend event, not the webview poll)

### Webcam

- [x] Runs during sessions when "Turn on webcam monitoring" is on (off by
      default). Phone in view is its own span, counts as distracted, nudges
      after the strictness grace through the same cooldown as window nudges,
      shows the popup; Muted records without nudging. Nobody in view for 60 s
      is Away
- [x] Worker watches at 2 fps and bursts to 5 fps on the first phone-like frame;
      skips face detection when a person is already found; hashes nothing on
      the UI thread; restarts if it hangs or never starts; falls back to CPU
      for the session when GPU mode fails; dies with the app (job object)

### Platform and reliability (audit of 2026-10-19)

- [x] Slow commands run off the UI thread; no file hashing or directory walks
      on the main thread; one transaction per settings save; SQLite WAL with
      `synchronous = NORMAL`
- [x] Worker busy flag and download phase reset on panic; poisoned session
      lock recovers; cancel reaches every hash, zip extraction and download
- [x] Dialogs always have an exit; a finished session does not open History
      over an open dialog; ratings cannot double-fire

## Next

1. **Live test pass on Windows** (nothing since 2026-10-18 has run outside the
   test suite): Check for updates and Cancel, record a voice and generate a
   clip, a session with the webcam on, quit and confirm no worker survives.
2. **Generate the starter phrase clips** for `derek` and `elise` with the
   worker's one-shot CLI (20 phrases each). Until then starter voices nudge
   silently.
3. **Audit leftovers** (session in progress): runtime size cached at install,
   manifest fetch timeouts, overlapping polls, camera index setting, low items.
4. **macOS**: website detection (`browser_macos.rs` is a stub), permissions,
   sleep, media presence, bundle-id capture. Needs a Mac. Checklist:
   `plans/mac-handoff.md`.
5. **Ship**: see below.

## Open product decisions

- Browser-wide rules with per-site exceptions, versus today's "browsers are
  always Neutral, only sites are classified".
- Observation coverage on the report (capture failures still count as Away).
- Stored provenance per span (which rule decided it), so old reports can
  explain themselves after rules change.

## Ship, after the app behavior is settled

- [ ] Prod voice path: download the frozen zip, generate, play, delete, regenerate
- [ ] GitHub Release for the macOS ARM64 zip (SHA-256 `fcfd9bc86932f16b53449b7b6e29ab108b9ba9f23aec8d1af5333fe4b1f79ae5`) and the Windows x64 zip (SHA-256 `69bace321f9f5f8f6c6fbec3c4dc20a50f109ffa60ed9fdd5c19f76f26ec9b0a`). Do not commit the zips
- [ ] Runtime manifest with both targets, and `SUPERVISOR_VOICE_PACK_MANIFEST_URL`
- [ ] Frozen webcam worker built on each OS (`scripts/build_webcam_runtime.py`);
      `webcam_bakeoff/models/yolo26s.onnx` is not in git (see that README)
- [ ] Windows installer, and a decision on code signing
- [ ] macOS signing and notarization

## Test gate

Every shipped item has to work as intended on **Windows**, and must not break
**macOS** code paths. Windows is the machine that can actually prove it.

- `cargo test` in `src-tauri` (189 tests), `npm run check`
- Webcam worker: `webcam_bakeoff/test_labels.py`, `test_detectors.py`, and
  `run_tracker.py --self-test` on CPU and GPU
- Keep macOS-only paths intact; re-smoke Mac when hardware exists
