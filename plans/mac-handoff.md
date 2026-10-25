# Mac handoff: things to build and test on a real Mac

Last updated: 2026-10-25 (first Mac pass, MacBook Pro M-series, macOS 14.5). Baseline:
branch `app-v2-simplified-rules`; `origin/main` was fast-forwarded to it the same day. Tracker:
`plans/STATUS.md`. Holes to expect: `plans/known-issues.md`.

**After the pass (2026-10-25, Windows):** `plans/review-2026-10-25.md` reviewed every Mac
commit and fixed, on `fix/post-mac-review`: bundled starter clips (`src-tauri/voices/<id>/`),
the silent Voice page on a failed clip, clip time estimates, and the empty Home scroll
(`scripts/mac-ui-check/assert.py` now allows Home no scroll at all, so rerun the WebKit check on
the Mac). A Mac agent should pull that branch, rerun `cargo test` and `check.sh`, and hear one
starter clip play in a session.

**First Mac pass (2026-10-25).** The app compiles, `cargo test` passes (198, 2 manual
ignored), `npm run check` passes, and a debug `Supervisor.app` runs with the V2 UI. Section 0
is built: the Accessibility address read works for Safari and Chrome (probe output below),
workers cannot outlive the app, camera names come from AVFoundation, child pids are listed.
Ticked items below were seen on this Mac; the rest is still unverified.

## Permissions and signing on a Mac (read first)

- **Accessibility** is what Supervisor needs: window titles *and* browser addresses come
  through it (`browser_macos.rs`). Screen Recording is no longer required; titles fall back
  to Accessibility when the window list gives none. The Getting started card on Home has an
  "Allow Accessibility" step with an Open settings button; the app asks once at startup.
- **Sign debug builds, or every rebuild drops the grant.** Unsigned, Tauri's debug bundle is
  ad-hoc signed with a per-build identifier (`codesign -dvv` shows
  `Identifier=supervisor-<hash>`), and macOS keys the grant to that: after a rebuild System
  Settings still shows Supervisor on, but `AXIsProcessTrusted` is false and every browser is
  Other websites; toggling the switch re-enables the stale entry and does not help. So, once
  per Mac, `sh scripts/make_mac_dev_cert.sh` (a self-signed "Supervisor Dev" certificate in
  its own `supervisor-dev` keychain with a known password, so codesign never asks for the
  login password even across the frozen worker's hundreds of files), then always build with
  `npm run bundle:mac-dev`. The designated requirement becomes
  `identifier "app.supervisor.desktop" and certificate leaf = H"…"`, which survives
  rebuilds. Switching certificates (ad-hoc to signed, or a new dev cert) needs
  `tccutil reset Accessibility app.supervisor.desktop` and one fresh approval. A release
  needs a Developer ID certificate from the Apple Developer Program (none on this Mac yet).
- **Release bundle.** `scripts/build_webcam_runtime.py`, then
  `sh scripts/sign_webcam_runtime.sh "<identity>"` (signs every library in the frozen worker
  under the hardened runtime with the camera entitlement, and fixes PyInstaller's
  Python.framework layout so codesign accepts it), then
  `APPLE_SIGNING_IDENTITY="<identity>" npx tauri build --config src-tauri/tauri.bundle.conf.json`.
  Seen on this Mac with the dev identity: Supervisor.app (368 MB, with `webcam/` and
  `voices/` resources) and a DMG; `nsis` in the target list is skipped silently on a Mac,
  so the shared target list is fine. `codesign --verify --deep --strict` passes on the app;
  `spctl` rejects it, as it will until a Developer ID signs and notarizes it.
- **Run the bundle, not the bare binary.** `npm run tauri dev` runs `target/debug/supervisor`
  from a terminal, so permission prompts name the terminal and the grant goes to it.
  `npm run bundle:mac-dev` then `open src-tauri/target/debug/bundle/macos/Supervisor.app`.
- **Probe.** `open -W src-tauri/target/debug/bundle/macos/Supervisor.app --env SUPERVISOR_PROBE=/tmp/probe.txt`
  runs the capture for 20 s inside the bundle (debug builds only) and writes one line per
  sample: app, path, title, browser read, and what the resolver made of it. For a browser it
  cannot read it also dumps the first levels of the accessibility tree. Seen on this Mac:

      accessibility=true screen_recording=false
      Safari  title=Some("Accessibility - Wikipedia") browser=address "https://en.wikipedia.org/wiki/Accessibility" -> Website Wikipedia [Address]
      Google Chrome  (first three ticks) browser=address None -> Website YouTube [Title]
      Google Chrome  browser=address "https://www.youtube.com/" -> Website YouTube [Address]

  Chrome builds its accessibility tree only after a client asks (`AXManualAccessibility`),
  so its first second or two reads by title. Safari reads on the first tick.

Rules for the Mac pass: do not change the data model, the rule semantics (a rule is a
distraction, everything else is work, history is never rewritten) or the screens; those were
settled on Windows and are proved by the tests and the mock. Keep Windows compiling: anything
Mac-only goes behind `#[cfg(target_os = "macos")]`, ideally in `src-tauri/src/macos.rs` or
`browser_macos.rs`. Sections 5 (window capture with `active-win-pos-rs`) and 6 (shell surfaces)
describe code that exists; do not re-implement title persistence from them: titles are a
fallback and are never stored.

## Day one on the Mac

Before any of the sections below. Everything here is a one-time setup; none of it is
Supervisor code.

- [x] **Pull the branch.** `git fetch && git checkout app-v2-simplified-rules`. `origin/main`
      is older than the branch; do not start from it. (A fresh clone lands on `main`, which
      shows the old UI; switch first.)
- [x] **Toolchain.** Xcode Command Line Tools, Rust stable (Tauri 2 needs it; `rustup` adds
      the host target), Node 20+, Python 3.11 (Chatterbox is documented against it). Then
      `npm install`, `npm run check`, and `cargo test` from `src-tauri`. On Windows the suite
      is 201 tests (197 pass, 4 ignored); on the Mac it is 200 (198 pass, 2 ignored), since the
      Win32 probe tests are `#[cfg(windows)]`. `system_resource_queries_return_real_values`
      passed on the first real run of the `macos.rs` memory and disk calls (rustc 1.99,
      Node 24, Python 3.12 on hand; the venvs below are not made yet).
- [x] **Webcam worker venv.** `python3 -m venv .venv-webcam`, then with
      `.venv-webcam/bin/python`: `pip install -r webcam_bakeoff/build-requirements.txt` and
      `webcam_bakeoff/download_models.py`. Debug builds run the worker from this venv
      (`webcam.rs`, `worker_command`). Done with Python 3.12: opencv 4.14.0, onnxruntime
      1.29.0 with `CoreMLExecutionProvider`, numpy 2.4.6. A python.org Python needs
      `/Applications/Python 3.12/Install Certificates.command` once or downloads fail.
- [x] **`webcam_bakeoff/models/yolo26s.onnx` is not in git.** Re-exported on the Mac with
      `export_models.py` (ultralytics 8.4.188 / onnx 1.23.2); the bytes differ from the
      Windows export, so `download_models.py` now accepts both hashes. 12 worker unit tests
      pass with it; `run_tracker.py --self-test` passes on CPU (1.4 s first frame) and on
      CoreML (6.4 s first frame, model compilation).
- [x] **Voice worker venv.** Debug builds look for `.venv-voice-runtime-macos/bin/python` at
      the repo root (`voice_runtime.rs`, `debug_worker_python`). Made with Homebrew's
      Python 3.11 (`brew install python@3.11`, then
      `/opt/homebrew/opt/python@3.11/bin/python3.11 -m venv .venv-voice-runtime-macos`) and
      `pip install -r voice_worker/requirements.txt -r voice_worker/build-requirements.txt`:
      torch 2.6.0 with Metal (`mps`) available. The `resemble-perth` watermarker that failed
      on macOS in the bakeoff is already patched to a no-op inside the worker.
- [ ] **Voice runtime zip.** A frozen `macos-arm64` runtime already exists in a GitHub
      release (SHA-256 in `plans/STATUS.md`; example manifest in
      `voice_worker/release_manifest.macos-arm64.example.json`). There is no Intel build.
      Rebuilding is `scripts/build_voice_runtime.py` from the voice venv, on the Mac.
- [x] **Run the app.** The debug bundle (see "Permissions and signing"), not `npm run dev`,
      so permissions attach to Supervisor. `npm run dev:ui` plus
      `http://127.0.0.1:1420/mock.html` renders every screen with fixtures and no backend.
      Home fits the 920x760 window exactly after the WebKit layout pass below.
- [x] **WebKit layout pass (2026-10-25).** The Mac webview is WebKit, Windows is Chromium,
      and WebKit sizes a grid or a `flex: 1` item inside a column flexbox against the
      window instead of its parent. Three places broke: Home overflowed by 110px
      (`.home-grid` now `height: 0; min-height: 0` under `flex: 1 1 auto`), the Settings
      Voice/Webcam cards threw their text and buttons to the bottom of a tall window
      (`.voice-summary` is a flex column, not a grid, and `flex: 1 1 auto`), and the report's
      "40:00 min" tile ellipsized because SF Mono is wider than Consolas (`fitShareStats`
      now shrinks report tiles too). Every mock screen was then screenshotted in a real
      WKWebView at 920x760 and 1850x1530, light and dark (46 shots: Home, empty Home,
      preflight, live session, Settings, Voice, Webcam, History, empty History, report,
      Trends, overlay): no horizontal overflow, nothing clipped, nothing displaced. Harness:
      a `WKWebView` snapshot tool over `mock.html` with CSS animations disabled (an
      offscreen window never runs them, so fade-ins stay invisible). Rule of thumb for new
      CSS: no `flex: 1` or `display: grid` with auto rows as a flex-column child without
      checking it in Safari at a tall window.
- [x] **Engine matrix (2026-10-25).** The same measurement script ran 34 states of the mock
      in WebKit and in Chromium at 920x760 (every tab and subpage, the onboarding, preflight,
      session-rules, delete-all, report-delete, tones, new-voice and info modals, the voice
      page offline / low RAM / no model, the history week picker and Apps pane, both status
      window placements) and compared page height, horizontal overflow, modal count and the
      size of every main panel. Every modal opens in both; no WebKit clipping or overflow;
      the only differences are 5–17 px of font metrics (Inter and SF Mono versus Consolas),
      not layout. The model, memory and line popups were measured in the running app
      instead (480x266, 480x374, 480x445, all inside the window) against Chromium's
      268/378/459.
- [x] **Status window interrupt card was 71 px too tall.** `fitOverlayWindow` measured the
      card on the frame after asking for the 520 px width, while the window was still 228 px
      wide, where the text wraps taller; the cached height then stuck. WebView2 resizes
      within the frame, WebKit does not. It now asks for the placement's width, waits until
      `innerWidth` matches it, then measures. Polled in the running app: corner card
      228x201, interrupt card 520x369, zero slack in both.
- [x] **Seeing what the capture resolves to.** `SUPERVISOR_PROBE` (above) is the Mac twin of
      `manual_foreground_probe`; it runs inside the bundle because a test binary has no
      Accessibility grant.
- [x] **Driving the app's own flows.** There is no WebDriver for Tauri on macOS, so debug
      builds take `SUPERVISOR_AUTOMATION=<dir>`: a script written to `<dir>/cmd.js` runs
      inside the main window's page (first line `//window=overlay` targets the status
      window) and its return value lands in `<dir>/result.json` (`lib.rs`,
      `start_automation`). Launch with
      `open …/Supervisor.app --env SUPERVISOR_AUTOMATION=/tmp/supervisor-automation`.
      Verified this way on 2026-10-25, each step checked against `sessions.db`: preflight
      (goal, duration) → start → pause → resume → stop → report; a YouTube tab in front
      read by address within one tick; crash recovery (a killed app's session comes back
      `abandoned` with its time); History week navigation, report, delete with its confirm;
      Settings strictness and timer toggle persist, rule add; Voice page with the model
      Installed and the RAM tile; Webcam page listing "FaceTime HD Camera", the camera
      prompt naming Supervisor, and the preview streaming 640x360 frames from the venv
      worker; a full distraction: YouTube in front → `suspicious` 5 s → `distracted`, the
      status window to its centered 520-wide interrupt card, "Not today" back to Locked in
      with YouTube recorded as focused, and the stopped session reporting 2 nudges and
      3 drifts (the session row's counters are written at stop, not live). Later passes
      covered the rest through the UI: webcam monitoring in a session, clip generation for
      the starter voice and for a voice the user recorded through the microphone prompt
      ("Pranav", five clips), the tray menu through `automation_tray`, Spotlight / Cmd-Tab /
      Mission Control through `automation_input`, Firefox / Brave / Edge by address, media
      presence through `automation_media`, and an idle Away span.
- [x] **Window sizes are 28px short on a Mac.** Tauri gives macOS windows a full-size content
      view: a window's "inner" height includes the title bar while the page is laid out below
      it, so the main window asked for 760 gave the page 732, and the status window sized to
      its content (201) showed 173 and cut off its last row. Windows has no such gap.
      `titlebar_inset` (frame minus `contentLayoutRect`, through `objc2-app-kit`) is added
      whenever a window is sized; both pages now measure exactly what was asked.
- [x] **Bundle targets.** `src-tauri/tauri.bundle.conf.json` lists `nsis`, `app` and `dmg`
      together; Tauri skips `nsis` on a Mac without a word, so one list serves both OSes.
      The day-to-day `tauri.conf.json` has `bundle.active` false and only the dev window.

## Backend parity audit (2026-10-25)

Every `cfg(windows)` path was read against its macOS twin. The macOS build's dead-code
warnings list exactly what Windows does that the Mac does not: only `app_identity`'s
Windows-only identity helpers (product family, script hosts) remain.

| Windows does | macOS before | macOS now |
|---|---|---|
| Names an app by its exe's FileDescription ("Visual Studio Code" for Code.exe) | Process name from the window list ("Code") | The bundle's file name, what Finder and the Dock show; the key stays the process name so history and rules carry on |
| Strips the browser name and profile from window titles | Raw AX title ("YouTube - Google Chrome – Alex (Work)"), so title fallbacks and the New Tab check missed | Same cleanup, with the macOS app names and the en-dash profile suffix |
| Clean names in storage | A left-to-right mark stuck to "\u{200e}WhatsApp" in keys, labels and rules | Stripped at capture; schema v13 cleans what was stored |
| Job object kills the worker's whole process tree | `kill -9` of the worker only | Workers lead their own process group; quit and stop kill the group |
| Microphone: denied gives silence too | No permission request; a denied mic recorded silence and cloned from it | Asks first (`microphone_access`, prompt names Supervisor); any silent recording is refused with a clear message on every OS |
| Address bar read retried after 5 s | A freshly launched Chromium browser read "site not read" for up to ~6 s | Searched every tick for 15 s after first asking |
| Script hosts (python, java) named by their script | A GUI app run by python shows as "Python" | Not done; rare on a Mac (most Python GUI apps ship as their own bundle) |

Still to see on this Mac (each needs the desktop, so only with the user's go-ahead): the
microphone prompt and a recorded sample, Brave and Edge reading the address within the first
seconds after launch, and the Now line naming each app the way the Dock does.

## Whole-codebase Mac review (2026-10-25)

Five reviewers read every file for macOS failures; every finding below was checked against the
code and fixed (207 tests, 36-state WebKit check):

- Session time used `Instant`, which stops while a Mac sleeps: a lid close stretched the timer
  instead of recording lost time. Now `SessionClock` (CLOCK_MONOTONIC, counts sleep).
- A front app with no window (last window closed, minimized, desktop clicked) failed capture
  and became "Device asleep" after 15 s. Now shell UI: the previous window keeps the time.
- Finder folders counted as the desktop without Screen Recording (title read after classifying).
- Closing the main window destroyed it for good; it now hides, and a Dock click reopens it.
- Status window now joins every Space and shows over full-screen apps.
- App Nap could throttle the 1 s ticker and nudges; the app holds a user-initiated activity.
- Capture runs in an autorelease pool (memory grew all session); lock screen records nothing.
- Browser reads: failures keyed per window, AX timeout set system-wide, search time-boxed,
  controls skipped, the http(s) web area preferred over side panels and DevTools.
- Other Mac browser builds recognized (Chrome Beta/Dev/Canary, Edge channels, Firefox Dev/Nightly,
  Safari Technology Preview, Opera GX, Zen, Orion, DuckDuckGo); unknown browser schemes are the
  browser's own page.
- Camera list built exactly as OpenCV numbers devices (video + muxed, sorted by uniqueID).
- Day bucketing uses the per-instant offset (`localtime_r`), so daylight saving does not move
  sessions between days.
- v13 migration: only direction marks, never user-typed names, never onto another app's key.
- Flagging while paused no longer closes the pause span; empty spans delete stale checkpoints;
  a database failing its integrity check is moved aside like a corrupt one.
- Voice: worker pid forgotten when it dies, released before a runtime update, memory pressure
  blocks only at critical (1/2/4 levels), permission wait times out, release builds ignore the
  worker override variables, runtime archives cannot write through extracted links.
- Frontend: card copy works under WebKit's clipboard rules, IME Enter does not submit, one
  focus ring, no trackpad bounce, plain search field, "computer" not "PC".
- Installer: atomic replace, certificate pinned, Apple Silicon by sysctl, waits for the old app,
  whole script in main(). Release and signing scripts fail on a wrong certificate or any
  unsigned binary. CI: 45-minute cap, WebKit check with a watchdog.

Still open (needs a person or a decision): the native `<audio controls>` player and the
monospace font look different from Windows (bundling one mono font fixes the second); Chrome
and Safari web-app shims count as apps; Python GUI apps show as "Python"; mixed-DPI status
window placement; Intel Macs.

## 0. Mac code that did not exist (built 2026-10-25)

In the order a user would hit it.

- [x] **Address-bar read: `src-tauri/src/browser_macos.rs`.** Reads the focused window's web
      area `AXURL` through the Accessibility API (raw `AXUIElement` calls over
      `objc2-core-foundation`, which Tauri already pulls in). The web area's place in the
      window is found once per browser (breadth-first, capped at 400 nodes) and walked again
      each tick, so a read is a handful of attribute calls (3-10 ms measured). `editing` is
      always false: the web area reports the loaded page, not the typed text. Chromium is
      told to build its tree (`AXManualAccessibility`, plus `AXEnhancedUserInterface` for old
      builds) the first time it is seen. Verified: Safari and Chrome. Not yet seen: Firefox,
      Edge, Brave, Arc (all expose `AXURL` on the web area, so they should just work).
      `CapturePermissions.accessibility` carries the grant; Home's Getting started card asks.
      Firefox, Brave and Edge verified on 2026-10-25 (`open -a <browser> --args
      --no-first-run --no-default-browser-check <url>` gets a Chromium browser past its
      setup screens unattended): Firefox's first-run `about:` page read as the browser's own
      page, Brave on github.com → GitHub, Edge on reddit.com → Reddit, all by address. Arc
      not installed.
- [x] **Worker lifetime.** The voice worker already exits when its stdin closes.
      `run_tracker.py` now exits as soon as it is orphaned (`os.getppid() == 1`), so a crash
      of the app releases the camera within a frame. A normal quit still kills both from
      `RunEvent::Exit`. `child_pids` has a macOS arm (`proc_listchildpids`), tested by
      spawning a child.
- [x] **Camera names.** `webcam::camera_names()` asks `AVCaptureDeviceDiscoverySession` for
      built-in wide-angle and external cameras, the same session OpenCV's AVFoundation backend
      uses, so index N here is the worker's camera N. This Mac: `camera 0: FaceTime HD Camera`
      (`cargo test manual_camera_names -- --ignored --nocapture`). Not yet confirmed against
      a second camera or Continuity Camera.
- [x] **No console window flag.** Nothing opens on a Mac when a worker starts; `CREATE_NO_WINDOW`
      is only how Windows asks for the same thing.
- [x] **App names carry a bidi mark.** macOS reports "‎WhatsApp" with a leading U+200E, which
      became the app key. Zero-width and bidi marks are stripped from the name now.
- [ ] **Frozen workers and signing**: section 1. These are the only parts that need a Mac to
      build at all; everything above only needs a Mac to run.

## 1. Build

- [x] `npm run check` and `cargo test` (from `src-tauri`) pass on macOS.
- [x] The debug bundle starts the app (`npm run tauri dev` not used; see "Permissions and
      signing").
- [ ] A release build opens without Gatekeeper warnings. Code signing and notarization are
      not set up yet.
- [x] Freeze the webcam worker on the Mac (Apple Silicon done; Intel still needs an Intel Mac):
      `.venv-webcam/bin/python scripts/build_webcam_runtime.py` staged 346 MB under
      `build/webcam/`, and the frozen worker passed both self-tests (CPU 290 ms, CoreML
      2.9 s first frame). Then `npm run build -- --config src-tauri/tauri.bundle.conf.json`.
      PyInstaller cannot cross-compile. Intel Macs use onnxruntime 1.23.2; Apple Silicon
      uses 1.29.0 (`webcam_bakeoff/requirements.txt`). macOS 14 or newer.
- [x] The frozen worker's libraries are signed by `scripts/sign_webcam_runtime.sh` with the
      same identity, hardened runtime, and `com.apple.security.device.camera` plus the two
      entitlements a frozen Python needs (`allow-unsigned-executable-memory`,
      `disable-library-validation`). After signing, no ad-hoc or unsigned Mach-O is left in
      the worker tree, and it still passes both self-tests from inside the bundled app.
      Whether the camera actually opens for the worker under the hardened runtime is still to
      be seen in a webcam session.
- [ ] The camera prompt must name Supervisor, not Terminal or Python. In a dev build it will
      name the terminal, because dev runs the venv directly.
- [x] **A signed debug bundle without entitlements never prompts.** Signing with the hardened
      runtime (any identity) and no `com.apple.security.device.camera` entitlement makes
      `AVCaptureDevice.requestAccess` answer "denied" at once, with no dialog, and the Webcam
      page says camera access is off. `npm run bundle:mac-dev` now passes `Entitlements.plist`
      like the release config does; the Getting started / webcam copy was right, the build
      was wrong.
- [ ] GPU mode (`Run on GPU` in webcam settings) must report CoreML, not an error, on a Mac
      with a GPU. It should show an error, not silently use the CPU, when CoreML cannot start.

## 2. Apple calls (written, not run)

`src-tauri/src/macos.rs` replaces the hand-written declarations. It was type-checked on
Windows with `cargo check --target aarch64-apple-darwin` and `x86_64-apple-darwin`, using a
stand-in for the window library (that library's own Mac bindings need the macOS SDK to
compile). Nothing below has executed on a Mac.

- [ ] Screen Recording check and idle time (`objc2-core-graphics`). Idle time uses
      `CGEventType(!0)` because `kCGAnyInputEventType` is a C macro (`~0`) and the bindings
      cannot export it. Confirm idle time rises when the Mac is untouched and resets on input.
- [ ] Camera permission (`objc2-av-foundation`). Confirm the prompt appears once and that
      denying it shows the System Settings message.
- [ ] CoreServices folder (`objc2-foundation`). Confirm it is `/System/Library/CoreServices`.
- [x] RAM, per-process memory, and free disk (`libc`, plus `mach2` to release a port).
      `cargo test system_resource_queries_return_real_values` passes on this Mac.

- [x] The premise holds: while a YouTube video plays in Chrome, `pmset -g assertions` shows
      Chrome's own pid holding `NoDisplaySleepAssertion "Video Wake Lock"`, which is what
      `keeps_display_awake` looks for. The 3-minute idle path itself was not sat through.
- [x] `keeps_display_awake(chrome pid)` driven from the debug hook (`automation_media`)
      while a long YouTube video played, paused with the `k` key and resumed: true, false,
      true. And a session with no input for 3+ minutes and no video recorded an `idle`
      span. The two halves are verified apart; the one combined 8-minute run was muddied
      by a 19-second test video and YouTube autoplay, so it is not counted.
- [ ] Media keeps an idle user present (`keeps_display_awake` in `macos.rs`, IOKit
      `IOPMCopyAssertionsByProcess`). With no input for 3+ minutes, the session must stay on
      the site (not Away) while: a YouTube video plays in Chrome, in Safari, and in Firefox,
      and during a Zoom or Teams call. It must go Away when the video is paused, when it plays
      in a background tab, and when only music plays. If Safari's assertion is held by a
      WebKit helper that does not set `AssertionOnBehalfOfPID` to Safari, Safari video will
      read as Away: check `pmset -g assertions` while it plays.

## 3. Removed: parsing `ps` and `df`

Replaced by the calls in section 2. Nothing left to swap. Still confirm voice cloning does
not say "Not enough free RAM" on a Mac that has plenty.

## 4. Voice cloning RAM gate

- [ ] Run voice cloning end to end in the app. The RAM check now uses free + inactive pages,
      minus speculative pages (already counted as free). Purgeable pages are not added.
- [x] The worker itself runs on the Mac: with the pinned Chatterbox Nano files in
      `~/Library/Application Support/app.supervisor.desktop/voice/model_packs/chatterbox-nano-minimal/`
      (fetched by hand from the same Hugging Face revision the app downloads),
      `chatterbox_nano_worker.py --device cpu` made a 1.8 s clip from `derek.wav` in 37 s
      wall: imports 7.3 s, model load 10.2 s, conditioning 15.8 s, synthesis 2.8 s. The
      one-shot CLI needs `--cache-dir` outside `--out-dir`, or it refuses its own caches as
      "output directory is not empty" (the app's `--serve` path does not hit this).
      `--device mps` fails inside Chatterbox ("Output channels > 65536 not supported at the
      MPS device", torch 2.6.0); the app always passes `--device cpu`, so nothing to change.

## 5. Window capture (library: `active-win-pos-rs`)

- [ ] App name, window title, and process path are correct for: Safari and Chrome (seen:
      "Safari", `/System/Volumes/Preboot/Cryptexes/App/System/Applications/Safari.app`,
      title "Accessibility - Wikipedia"; "Google Chrome", `/Applications/Google Chrome.app`,
      title "YouTube - Google Chrome – Alex (Work)", so Chrome on a Mac appends the
      profile with an en dash, which the title cleanup does not strip; harmless while the
      address reads). Still to see: a code editor, Finder, Terminal, a full-screen app.
- [x] Window titles no longer need Screen Recording: with it denied the window list gives no
      title and Accessibility supplies it (`focused_window_title`). Both probe runs above
      ran with `screen_recording=false`.
- [ ] Idle time rises when the Mac is untouched and resets on input.
- [ ] Supervisor ignores its own windows. It recognizes itself by process ID and program path
      only. Check that the path `active-win-pos-rs` reports (app bundle or the binary inside
      it) still matches, or that the process ID alone is enough.

## 6. Mac shell surfaces (written, not run)

`classify_macos_surface` in `activity_normalize.rs` ignores apps whose bundle is directly
inside the CoreServices folder (Dock, Spotlight, Control Center, Notification Center, and
whatever Apple adds there later). Finder counts when its window has a title (a folder) and
is ignored when it does not (the desktop). The login window gives no sample. Utilities in
`CoreServices/Applications` (Screen Sharing) still count. The rule is tested on Windows.

- [x] Spotlight, a held Cmd-Tab and Mission Control keep the previous window counting:
      posted as real keystrokes by the app itself (`automation_input` in the debug hook,
      CGEvent, since the app holds the Accessibility grant) during a live session, the
      capture went on crediting Chrome/YouTube and no shell window became a sample.
      Launchpad, Notification Center, Control Center and the Dock not driven.
- [x] Opening a Finder folder counts as Finder: probe with `~/Downloads` in front resolved
      `App Finder [App]` with the folder as the title. Clicking the desktop not driven.
- [ ] Without Screen Recording permission, window titles are missing, so a Finder folder
      looks like the desktop and gets ignored. Confirm that, and whether the app should ask
      for Screen Recording before trusting this.
- [ ] The login window must not record a sample.
- [x] Safari and Chrome on macOS read the address (section 0). Titles are still the fallback
      for Chrome's first second and are not stored.
- [ ] A New Tab counts as the browser app itself, found by its title ("New Tab", Safari's
      "Start Page") while the address can't be read. Confirm Safari's title after the
      browser-name cleanup is exactly "Start Page".

## 6b. Webcam packaging on a Mac

- [ ] Continuity Camera can make an iPhone the default camera. Confirm camera 0 is the
      built-in camera, or that the worker's error is clear when it is not.
- [x] PyInstaller 6.22 on macOS put no symlinks in the frozen folder (`find -type l` is
      empty; Python.framework's `Current`, `Python` and `Resources` are real copies).
      `sign_webcam_runtime.sh` turns those back into symlinks so codesign accepts the
      framework; Tauri's resource copy then flattens them again (`Python` becomes a file,
      `Current` and `Resources` are dropped). The installed worker still runs, and
      `codesign --verify --deep --strict` passes on the app and on the framework inside it.
- [ ] `npm run tauri dev` uses the venv, not the frozen worker. Only `npm run build` with
      `tauri.bundle.conf.json` bundles `build/webcam/`.

## 7. App behavior

- [x] Single instance: a second `Contents/MacOS/supervisor` launched by hand exits at once;
      one process remains.
- [ ] Storage: the database is created under `~/Library/Application Support/app.supervisor.desktop/`
      (seen: `sessions.db` plus WAL, and a 29 s session with its spans after the first run).
      Still to try: a corrupt file is moved aside with the "starting fresh" card; a folder with
      no write permission shows the "history off" notice.
- [ ] Tray (menu bar) icon and its menu work. The status window stays on top when pinned.
- [ ] Microphone and camera permission prompts show the texts in `src-tauri/Info.plist`.
- [x] Voice model: with the pack present, Check for updates re-hashes all 1.9 GB
      (10–50 s) and ends `complete` with `up_to_date`; the popup now says so in place, since
      the 1.4 s toast was the only confirmation and read as nothing having happened. Clip
      generation works (section 4). A fresh download through the UI was not run here: the
      pack was fetched by hand first; the same code path serves Check for updates.
