# Supervisor Work Tracker

Living log of what is done and what is left. Update this file when an item
moves, rather than appending a second tracker.

Last updated: 2026-10-25 night (`app-v2-simplified-rules` at `ab3c544`, merged
and pushed; `main` points at the same commit)

Product OS targets: **macOS and Windows**. Linux is a workshop only, not a V1
ship target. Everything through 2026-10-24 was written and run on Windows. On
2026-10-25 the app was built and run on a MacBook for the first time:
`plans/mac-handoff.md` says what was seen and what is still open, and
`plans/mac-handoff-2026-10-25.md` is that day's summary. The review of that
pass from Windows, and what is left for a release, is
`plans/review-2026-10-25.md`.

## Post-Mac review (2026-10-25, Windows, `fix/post-mac-review`)

- [x] Every Mac commit read and checked on Windows: 210 tests, 204 pass, 6
      manual ignored; `npm run check`; CI green on macOS and Windows
- [x] Starter voices ship their 20 phrase clips each (`src-tauri/voices/<id>/`),
      copied into the starter profiles once; a deleted clip stays deleted and an
      existing one is kept. Chosen from four seeded takes by length against the
      text. **Not yet listened to**
- [x] Voice page shows why a clip failed, under Generate; a failed queue no
      longer disables Generate, the voice picker and page rebuilds
- [x] Clip estimates: ~90 s for the first clip (model load), ~15 s for each
      after (3-6 s measured), instead of ~90 s for every clip
- [x] Empty Home no longer scrolls at 920x760 (quote card size containment, and
      the quote re-fits after the webfont loads and on resize); the WebKit check
      now allows Home zero scroll
- [x] Four Windows compiler warnings from the Mac pass; the silent-recording
      test runs on every OS
- [x] Plain-language pass (`copy/plain-language`): every string users see,
      checked across 40 mock screens and dialogs, uses the vocabulary in
      `AGENTS.md`. The distraction card says Back to my task, Allow once,
      Always allow; the voice page speaks of lines and the voice engine;
      backend errors a user can hit say what happened and what to do. The
      landing page now says the voice engine is about 2 GB, not 300 MB (Generate, Regenerate, GPU and
      vision models kept on request)
- [x] Distraction card with a voice (`feat/orb-card`): the card and the first
      nudge come together at the strictness time (90, 60 or 30 s; a phone after
      its 5-15 s grace), and the first nudge of each new distraction ignores the
      cooldown; later ones on the same place still wait it out. The card is the
      coral alert with "You opened YouTube." / "Your phone is out.", a voice
      panel whose orb is the play button (bars while speaking, a play triangle
      after), the task and time left, and Back to my task / Allow once / Always
      allow. The main window plays the audio and tells the card through
      `nudge-voice` / `nudge-control` events
- [x] Phone lines: a bundled Phone set (four lines) that every voice can
      generate; a phone nudge plays from it, else from the voice's tone. It is
      never a speaking tone and never counts toward a voice being ready.
      Starter voices ship its clips; new bundled lines reach existing installs
      (the seed marker lists each line it offered). Starter voices are named
      Derek and Elise on screen, so the card reads "Elise is speaking"

## Pomodoro (2026-10-26, branch `pomodoro`)

- [x] Start screen: a Timer / Pomodoro switch under the goal. Timer keeps the
      slider; Pomodoro shows 25/5, 50/10 and 90/20 chips with a rounds stepper
      and the finish time. The session's length is its rounds' work time
- [x] A break is a pause the session takes by itself at the end of a round:
      nothing is judged or counted, the timer holds, the card turns yellow and
      the distractions list reads "Don't count during a break". When the break
      runs out the session waits; "Start round N" (Pause's place; "Next
      round" in the status window) starts the next one. The last round ends
      the session
- [x] Round dots on the live card and the status window: finished rounds yellow
      on a round, green on a break, the current one ink
- [x] Round over card (yellow): "Round N done.", the session's goal, a tomato
      per round (the new one drops in), the round's focused minutes, the voice,
      Start break. Break over card (green): "Break over.", the voice, Start
      round N. The backend emits `session-moment`; the main window plays a line
      from the new Round over / Break over sets (system sets like Phone, never
      a speaking tone; starter voices ship them)
- [ ] Not built yet: Pomodoro in the report (rounds count), a closing line when
      the last round ends, the voice page's Nudges / Moments grouping, the
      practice run

## macOS pass (2026-10-25)

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
- [x] Dev signing: `scripts/make_mac_dev_cert.sh` once per Mac (own keychain,
      no password dialogs), then `npm run bundle:mac-dev`; permission grants
      survive rebuilds
- [x] Webcam worker on the Mac: `.venv-webcam`, YuNet downloaded, YOLO26s
      re-exported (both hashes accepted), 12 unit tests, CPU and CoreML
      self-tests, frozen with PyInstaller (346 MB), every library signed by
      `scripts/sign_webcam_runtime.sh`; the release bundle
      (`tauri.bundle.conf.json`, app + DMG) carries it and the worker runs
      from inside the app
- [x] Voice worker on the Mac: `.venv-voice-runtime-macos` (Python 3.11,
      torch 2.6), one clip generated from `derek.wav` on CPU in 37 s wall
- [x] CI: `.github/workflows/ci.yml` runs `npm run check`, `cargo test` and a
      Mac debug bundle on macOS and Windows for every push
- [x] WebKit layout pass: every screen screenshotted in a real WKWebView at two
      sizes and both themes; three Mac-only layout bugs fixed (Home overflow,
      Settings cards, report tiles). Window sizes corrected for the title bar
      that macOS counts inside a window's inner height (status window was
      losing its last row)
- [x] Live flows driven inside the real app (`SUPERVISOR_AUTOMATION`, debug
      builds) and checked against the database: preflight → start → pause →
      resume → stop → report, crash recovery, History (weeks, report, delete),
      Settings (strictness, timer, rules), Voice page, Webcam prompt and
      preview, a full YouTube distraction with the interrupt card, "Not today",
      2 nudges recorded
- [x] Second live pass: webcam monitoring inside a session, five voice clips
      generated from the Voice page (after fixing the venv-symlink bug in
      `voice_runtime.rs`), kill -9 leaves no worker, Finder folders count,
      Chrome's video wake lock is the assertion media presence reads
- [x] `scripts/mac-ui-check/check.sh`: every mock state laid out in WebKit,
      fails on overflow, a modal or overlay card taller than its window, or
      a scrolling Home; runs in CI on the macOS job
- [x] Later the same day (`plans/mac-handoff-2026-10-25.md`): Firefox, Brave
      and Edge by address; tray actions; shell surfaces; media presence and Away;
      Dock app names; browser profile suffixes; mic permission and silence check;
      sleep-aware session clock; hide on close; status window on all Spaces; App
      Nap off; one-line installer and signed release zip; schema v13
- [ ] Still open on the Mac: the unticked items in `plans/mac-handoff.md`
      (notably a voice clone through the installed app, Continuity Camera, the
      corrupt-database card, Arc) and an Intel build

## Branches

**`app-v2-simplified-rules` is the baseline**, and `origin/main` points at the
same commit. Everything below is merged into it: the post-Mac review
(`fix/post-mac-review`), the plain-language pass (`copy/plain-language`), the
speaking distraction card (`feat/orb-card`) and a tried-then-reverted day card
change (`feat/day-card-last-session`; the strip stays the whole day). Those
branches can be deleted.

On 2026-10-25 the remote history was rewritten once to replace a Mac hostname
author address with the gmail one; no tree or message changed. Every older
local branch (`feat/*`, `test-features`) belongs to the pre-rewrite history and
is fully contained in the baseline by content: never merge one again. The old
local tips are kept as `backup/pre-email-rewrite` and
`backup/pre-email-rewrite-main`; all of these can be deleted.

Sessions started from the desktop app get their own worktree under
`.claude/worktrees/` (git-ignored) and merge back with a plain merge.

## Done on `app-v2-simplified-rules` (2026-10-21 to 2026-10-24)

First run in the built app on the Mac on 2026-10-25 (`plans/mac-handoff.md`);
on Windows only `cargo test`, `npm run check`, the browser mock and CI so far.
The last live run on Windows was 2026-10-18.

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

Needs a person (details in `plans/review-2026-10-25.md`):

1. **Listen to the 40 starter clips** and regenerate any that sound wrong.
2. **Publish the two GitHub Releases** (app `v0.1.0`, `voice-runtime`
   pre-release); `plans/distribution.md` has the steps.
3. **Decide Windows distribution** (direct download with SmartScreen, or the
   Microsoft Store), then build its installer.
4. **Live test pass on Windows** with the running copy closed: a V2 session
   with "Not today" and "Never a distraction", History with real weeks, a voice
   tone end to end, a webcam session, quit and confirm no worker survives, and
   closing the window (it now hides).
5. **Back up the Mac signing certificate** into a password manager.
6. **Decide whether the repository should stay public.**

Agent work, no sign-off needed:

7. Delete the merged branches: `fix/post-mac-review`, `copy/plain-language`,
   `feat/orb-card`, `feat/day-card-last-session`, the old `feat/*`,
   `test-features` and `backup/*`.
8. Freeze the Windows webcam worker (`.venv-webcam` needs PyInstaller from
   `webcam_bakeoff/build-requirements.txt`) and build the NSIS installer with
   `tauri.bundle.conf.json`, so the Windows decision only has to pick a channel.
9. Remaining Mac checklist items that a Mac agent can drive alone.

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
- [ ] Upload the voice runtime to the fixed `voice-runtime` GitHub Release (pre-release): macOS arm64 zip rebuilt and tested 2026-10-25 (SHA-256 `210878b6061a85fba0ef1b2b222a365b3eff5b8a469ae1772653b17955a8bc6b`, in `build/voice-runtime/`) plus its manifest. The older macOS zip (`fcfd9bc8…`) cannot start on a Mac (zipped without symlinks); the Windows x64 zip was rebuilt with the current worker on 2026-10-25 (SHA-256 `de5ae5fc51268e61ec8081ea149723ba809ab664e06849cc953ec8a108cca179`, 311 MB, in `build/voice-runtime/` on the Windows machine), installed through the app's own install code and made a clip cold in 39 s; the old `69bace32…` zip is superseded. The manifest can now list both targets (no `--partial`) once both zips have release URLs. Do not commit the zips. See `plans/distribution.md`
- [x] Installed copies find the runtime without build-time setup: the manifest address is compiled in (`DEFAULT_MANIFEST_URL`); a macOS-only manifest is built with `--partial`. Add the Windows target when its zip is rebuilt
- [ ] Frozen webcam worker built on each OS (`scripts/build_webcam_runtime.py`);
      `webcam_bakeoff/models/yolo26s.onnx` is not in git (see that README).
      Apple Silicon: done 2026-10-25, signed with the dev identity. Windows
      and Intel Mac: not yet
- [ ] Windows installer, and a decision on code signing
- [ ] macOS Developer ID signing and notarization (the signing steps exist
      and work with the dev identity: `scripts/sign_webcam_runtime.sh`, then
      `APPLE_SIGNING_IDENTITY` on `tauri build`; swap in the Developer ID)

## Test gate

Every shipped item has to work as intended on **Windows** and on **macOS**.

- `cargo test` in `src-tauri`: Windows 211 tests, 205 pass, 6 ignored
  (2026-10-25, `ab3c544`); macOS 207 pass at `aa9a92a`
  (Mac handoff). CI (`.github/workflows/ci.yml`) runs both on every push
- `npm run check` (tsc + vite build)
- Browser mock: `npm run dev:ui`, open `http://127.0.0.1:1420/mock.html`
  (`?empty=1` for a new user, `?big=1` for heavy weeks); render Home, Session
  with the preflight, live card, dialog and popup, History, Voice and Settings
  in both themes
- Webcam worker: `webcam_bakeoff/test_labels.py`, `test_detectors.py`, and
  `run_tracker.py --self-test` on CPU and GPU
- macOS: the CI macOS job compiles, tests, bundles and runs
  `scripts/mac-ui-check` (WebKit layout of every mock state). Cross-checking
  from Windows (`cargo check --target aarch64-apple-darwin`) no longer works
  since `objc2-exception-helper` needs Apple's compiler. On the Mac, the debug
  bundle plus `SUPERVISOR_PROBE` (`plans/mac-handoff.md`) shows what the
  capture resolves to
