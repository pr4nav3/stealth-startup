# Supervisor Work Tracker

Living log of what is done and what is left. Update this file when an item
moves, rather than appending a second tracker.

Last updated: 2026-10-25 (`app-v2-simplified-rules`; first macOS pass, uncommitted
in the working tree)

Product OS targets: **macOS and Windows**. Linux is a workshop only, not a V1
ship target. Everything through 2026-10-24 was written and run on Windows. On
2026-10-25 the app was built and run on a MacBook for the first time:
`plans/mac-handoff.md` says what was seen and what is still open.

## macOS pass (2026-10-25, working tree)

- [x] Builds on macOS: `cargo test` 198 pass, 2 manual ignored; `npm run check`;
      debug `Supervisor.app` bundle runs the V2 UI
- [x] Browser address read (`browser_macos.rs`): web area `AXURL` through
      Accessibility; verified on Safari and Chrome, 3-10 ms a read. Window
      titles also come from Accessibility, so Screen Recording is not needed
- [x] `CapturePermissions.accessibility`; Home's Getting started card has an
      "Allow Accessibility" step with Open settings (`get_capture_access`,
      `open_accessibility_settings`). The old Screen Recording banner is gone:
      it pushed Home past the window
- [x] WebKit layout: `.app-shell` has a definite `height: 100vh`; Home fits
      920x760 exactly in both engines
- [x] Workers cannot outlive the app (webcam worker exits when orphaned);
      `child_pids` on macOS; camera names from AVFoundation; bidi marks
      stripped from app names
- [x] `SUPERVISOR_PROBE` runs the capture probe inside the debug bundle
- [x] Dev signing: `scripts/make_mac_dev_cert.sh` once per Mac, then
      `npm run bundle:mac-dev`; permission grants survive rebuilds
- [ ] Not yet: Python venvs, webcam and voice workers on the Mac, frozen
      workers, Developer ID signing and notarization, shell surfaces, media
      presence, other browsers

## Branches

**`app-v2-simplified-rules` is the baseline.** It was cut from `main` at
`c7eb971` and carries the V2 rules model, the History and Voice rebuilds and
the Settings polish below. `main` (`fd6b418`) is an ancestor of it: a plain
fast-forward of `main` onto this branch is the intended merge, nothing to
resolve. `origin/main` is still at `3e14095` (2026-10-19) and has none of this;
push before another machine starts.

Every other local branch is fully inside this one and must not be merged
again: `feat/home-streak`, `feat/window-supervision`,
`feat/durable-app-identity`, `feat/sqlite`, `feat/ui`,
`feat/voice-runtime-hardening`, `feat/webcam-models`, `feat/win32-capture`,
`test-features`. They can be deleted.

Sessions started from the desktop app get their own worktree under
`.claude/worktrees/` (git-ignored) and merge back with a plain merge.

## Done on `app-v2-simplified-rules` (2026-10-21 to 2026-10-24)

Not yet run in the built app: everything below was proved by `cargo test`,
`npm run check` and the browser mock (`mock.html`, see Test gate). The last
live run on Windows was 2026-10-18.

### Rules: distractions only (V2, 2026-10-22)

- [x] A rule *is* a distraction; everything else is work. Relevant, Neutral and
      Unclassified are gone from the data, the engine, the reports and every
      screen. One list, edited through the same rule manager in Settings, the
      preflight and the live dialog. Starter rules: five common sites
- [x] Per-session allowances instead of a per-session rule copy: a preflight
      chip or the nudge popup's "Not today" allows a site for this session
      only; "Never a distraction" drops the rule; "This is a distraction" in
      the overlay's More settings adds one. Commands `flag_current_distracting`,
      `allow_current_for_session`, `allow_current_always`
- [x] Rules apply from the next sample and never rewrite recorded time (schema
      v12 migrates neutral spans to focused, drops non-distraction rules,
      recomputes every session once). No classify flow after a session; the
      report is read-only. The old rescore path and the "Apply permanently"
      button are gone
- [x] Score is time not on a distraction divided by duration. Report rows are
      grouped Distractions / Work / Away and carry time only (the row toggle
      and its backend flags were removed on 2026-10-22)
- [x] Full inventory, traced to code: `plans/v2-distractions-only.md`.
      `plans/single-rule-set.md` was the earlier plan and is superseded

### Home

- [x] Streak calendar, daily note, and a day card (replaces the session card)
      that copies its summary on click; weekly bars removed. The calendar pops
      in when scrolled into view
- [x] Start opens the preflight in its own layer so Home stays put; the
      preflight readout is plain text; the rule editor keeps the name on the
      address row
- [x] Header chip shows today's focused time while idle and opens the Session tab
- [x] Getting started card: clone a voice, webcam (optional), first session.
      "Add rules" became a line in the intro (five distractions are preset)

### History (v4 boards, 2026-10-22 to 2026-10-24)

- [x] Week toolbar (previous / next with a This week / Last week tag, search
      across every session by goal via `search_sessions`), yellow week hero
      (focused hours, longest stretch, recovery, daily bars), Trends door
      (change against last week through the same weekday, four-week bars),
      Sessions / Apps & sites as two buttons over one panel
- [x] Month picker behind the week label (`get_focus_days`), days shaded by
      focused time, viewed week outlined; any past day picks its week
- [x] Apps & sites: Work beside Distractions with totals, Away as a footer;
      bars share a smart scale and fill the track on quiet weeks
- [x] Each session row draws its real timeline in miniature, gaps included
      (`get_history_week` carries the segments)
- [x] Charts rise once, together; empty weeks and sparse trends explain
      themselves; a finished session refreshes the week in place
- [x] Trends: attention span (not "typical stretch"), recovery, focused hours;
      bars from theme tokens; axis thins at six weeks

### Voice (v2 boards, 2026-10-23)

- [x] One voice speaks one tone (`set_voice_tone`, stored in profile.json);
      nudges draw only from that tone, and a tone is selectable once every
      line of it has a clip. Clips are made in place, one after another, with
      Stop after this line and per-line Redo
- [x] Status tiles for the voice model and memory turn red when they block and
      open popups; the how-to leads the page; the RAM lock is a one-line strip
      with Details. The model check never shows as a download, and matching
      files are provably skipped
- [x] A line opens one popup that edits, regenerates or deletes it; a Tones
      popup renames, deletes and adds tones; no edit mode. Popups repaint in
      place; a regenerate reopens its line when the clip lands
- [x] Four bundled tones, five lines each: Gentle, Mom, Gen Z, Stern

### Settings and copy

- [x] Named cameras in the webcam select (Windows Media Foundation; macOS
      AVFoundation since 2026-10-25; Linux falls back to "Camera N"), smooth
      preview at 15 pictures/s with
      detection still at 2 fps (`--preview-fps`), strictness seconds in the
      options, danger buttons everywhere, themed scrollbars, Defaults controls
      as inner cards
- [x] One vocabulary across app and landing page: distraction, nudge, longest
      stretch, attention span, recovery; each stat and strictness explained
      where it appears
- [x] Browser mock harness (`mock.html`, `npm run dev:ui`, launch config
      `ui-mock`) renders every screen without Tauri; `?empty=1` and `?big=1`
      fixtures

## Done on `main` before the branch (through 2026-10-21)

### Window supervision and history

- [x] Browser time is one row per website, read from the address bar (Windows,
      UI Automation). Browser pages, local pages and Other websites are their
      own rows. Titles are a narrow fallback and are never stored. A browser
      is never a distraction on its own; only websites are
- [x] Durable app identity (schema v11): packaged-app id, exe + publisher, or
      bundle id, mapped to one key per app; display names kept separate
- [x] Every second of a session is recorded. Spans tile the session from Start
      to Stop: Supervisor's own windows record as focused, pauses as `paused`,
      sleep or a frozen poll loop as `unknown` ("Device asleep / unknown").
      A property test fuzzes random event sequences and fails on any hole
- [x] Mute keeps recording and only silences nudges; Pause is the untracked break
- [x] Session report: timeline, Apps & sites, weekly recap, Trends (unlocks
      after two full weeks). Monthly recap and category guesses removed
- [x] Live-session polling repaints the clock and state in place; it never
      rebuilds under an open editor or a focused field

### Voice

- [x] Phrase editor: add, edit, move between sets, delete (bundled phrases are
      hidden via `hidden_phrases`), new sets inline
- [x] Persistent `--serve` worker keeps the model loaded, releases after idle,
      is killed on app exit (Windows job object), survives a per-request
      exception, and reports the real reason when it dies at startup
- [x] RAM gate from a measured run: 4 GiB free, 7 GiB total
- [x] Mic recording capped at 60 s; stop timeout and device-ready wait fixed
- [x] Runtime zip reinstalls when the manifest version or checksum changes;
      size recorded at install; manifest fetch gives up and cancels fast;
      "Check for updates" runs off the UI thread and can be cancelled mid-hash
- [x] Voice nudges play the moment they are due, even with the window
      minimized (backend event, not the webview poll)

### Webcam

- [x] Runs during sessions when "Turn on webcam monitoring" is on (off by
      default). Phone in view is its own span, counts as distracted, nudges
      after the strictness grace through the same cooldown as window nudges;
      Muted records without nudging. Nobody in view for 60 s is Away
- [x] Worker watches at 2 fps and bursts to 5 fps on the first phone-like frame;
      skips face detection when a person is already found; restarts if it
      hangs or never starts; falls back to CPU for the session when GPU mode
      fails; dies with the app (Windows job object); camera index is a setting

### Platform and reliability (audit of 2026-10-19)

- [x] Slow commands run off the UI thread; no file hashing or directory walks
      on the main thread; one transaction per settings save; SQLite WAL with
      `synchronous = NORMAL`
- [x] Worker busy flag and download phase reset on panic; poisoned session
      lock recovers; cancel reaches every hash, zip extraction and download
- [x] Dialogs always have an exit; a finished session does not open History
      over an open dialog; ratings cannot double-fire; no overlapping polls;
      week navigation cannot go stale

## Next

1. **macOS pass, continued**: section 0 of `plans/mac-handoff.md` is built and
   the address read is verified. Left: the two Python venvs and
   `webcam_bakeoff/models/yolo26s.onnx` (not in git, copy from the Windows
   machine), a webcam session and a voice clip on the Mac, the remaining
   checklist sections, then frozen workers and signing (a dev certificate
   first, so rebuilds keep the Accessibility grant).
2. **Live test pass on Windows** of everything on this branch (nothing since
   2026-10-18 has run outside the test suite and the mock): a V2 session with
   "Not today" and "Never a distraction", the schema v12 migration on a real
   database, History v4 with real weeks, a voice tone generated end to end,
   Check for updates and Cancel, a session with the webcam on, quit and confirm
   no worker survives.
3. **Generate the starter phrase clips** for `derek` and `elise` with the
   worker's one-shot CLI. Until then starter voices nudge silently.
4. **Fast-forward `main`** to this branch and push; delete the merged feature
   branches.
5. **Ship**: see below.

## Open product decisions

- Browser-wide rules with per-site exceptions, versus today's "a browser is
  never a distraction on its own, only sites are".
- Observation coverage on the report (capture failures still count as Away).
- Stored provenance per span (which rule decided it), so old reports can
  explain themselves after rules change.
- A preflight allow-list typed ahead of time (today an allowance is a tap on an
  existing chip or a reaction to the first nudge).

## Ship, after the app behavior is settled

- [ ] Prod voice path: download the frozen zip, generate, play, delete, regenerate
- [ ] GitHub Release for the macOS ARM64 zip (SHA-256 `fcfd9bc86932f16b53449b7b6e29ab108b9ba9f23aec8d1af5333fe4b1f79ae5`) and the Windows x64 zip (SHA-256 `69bace321f9f5f8f6c6fbec3c4dc20a50f109ffa60ed9fdd5c19f76f26ec9b0a`). Do not commit the zips
- [ ] Runtime manifest with both targets, and `SUPERVISOR_VOICE_PACK_MANIFEST_URL`
- [ ] Frozen webcam worker built on each OS (`scripts/build_webcam_runtime.py`);
      `webcam_bakeoff/models/yolo26s.onnx` is not in git (see that README)
- [ ] Windows installer, and a decision on code signing
- [ ] macOS signing and notarization

## Test gate

Every shipped item has to work as intended on **Windows** and on **macOS**.
Until the Mac pass, Windows is the only machine that has proved anything.

- `cargo test` in `src-tauri`: Windows 201 tests, 197 pass, 4 ignored
  (2026-10-24); macOS 200 tests, 198 pass, 2 ignored (2026-10-25)
- `npm run check` (tsc + vite build)
- Browser mock: `npm run dev:ui`, open `http://127.0.0.1:1420/mock.html`
  (`?empty=1` for a new user, `?big=1` for heavy weeks); render Home, Session
  with the preflight, live card, dialog and popup, History, Voice and Settings
  in both themes
- Webcam worker: `webcam_bakeoff/test_labels.py`, `test_detectors.py`, and
  `run_tracker.py --self-test` on CPU and GPU
- macOS: `cargo check --target aarch64-apple-darwin` from Windows keeps the
  Mac paths compiling; on the Mac, the debug bundle plus `SUPERVISOR_PROBE`
  (`plans/mac-handoff.md`) shows what the capture resolves to
