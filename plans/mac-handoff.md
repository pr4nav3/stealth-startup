# Mac handoff: things to build and test on a real Mac

Last updated: 2026-10-25 (first Mac pass, MacBook Pro M-series, macOS 14.5). Baseline:
branch `app-v2-simplified-rules` (`main` is an ancestor of it and will be fast-forwarded;
`origin/main` is older than both, so pull the branch, not `main`). Tracker:
`plans/STATUS.md`. Holes to expect: `plans/known-issues.md`.

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
  the login keychain), then always build with `npm run bundle:mac-dev`. The designated
  requirement becomes `identifier "app.supervisor.desktop" and certificate leaf = H"…"`,
  which survives rebuilds. Switching from ad-hoc to signed once needs
  `tccutil reset Accessibility app.supervisor.desktop` and a fresh approval. A release needs
  a Developer ID certificate from the Apple Developer Program (none on this Mac yet).
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
- [ ] **Webcam worker venv.** `python3 -m venv .venv-webcam`, then with
      `.venv-webcam/bin/python`: `pip install -r webcam_bakeoff/build-requirements.txt` and
      `webcam_bakeoff/download_models.py`. Debug builds run the worker from this venv
      (`webcam.rs`, `worker_command`). The pinned onnxruntime differs for Intel and Apple
      Silicon (`webcam_bakeoff/requirements.txt`).
- [ ] **`webcam_bakeoff/models/yolo26s.onnx` is not in git** and the worker will not start
      without it. Copy it from the Windows machine (38 MB; `download_models.py` checks its
      SHA-256 and rejects any other file, so a fresh export only works if it reproduces the
      exact file). Then `run_tracker.py --self-test` on CPU and on GPU (CoreML).
- [ ] **Voice worker venv.** Debug builds look for `.venv-voice-runtime-macos/bin/python` at
      the repo root (`voice_runtime.rs`, `debug_worker_python`). Create it with Python 3.11 and
      `pip install -r voice_worker/requirements.txt` (Chatterbox from a pinned git commit; the
      first run downloads weights). The `resemble-perth` watermarker that failed on macOS in
      the bakeoff is already patched to a no-op inside the worker.
- [ ] **Voice runtime zip.** A frozen `macos-arm64` runtime already exists in a GitHub
      release (SHA-256 in `plans/STATUS.md`; example manifest in
      `voice_worker/release_manifest.macos-arm64.example.json`). There is no Intel build.
      Rebuilding is `scripts/build_voice_runtime.py` from the voice venv, on the Mac.
- [x] **Run the app.** The debug bundle (see "Permissions and signing"), not `npm run dev`,
      so permissions attach to Supervisor. `npm run dev:ui` plus
      `http://127.0.0.1:1420/mock.html` renders every screen with fixtures and no backend.
      Home fits the 920x760 window exactly; WebKit needed `.app-shell { height: 100vh }`
      (with `min-height` it stretched the Home grid to the whole viewport and the page
      scrolled by 110px).
- [x] **Seeing what the capture resolves to.** `SUPERVISOR_PROBE` (above) is the Mac twin of
      `manual_foreground_probe`; it runs inside the bundle because a test binary has no
      Accessibility grant.
- [ ] **Bundle targets.** `src-tauri/tauri.bundle.conf.json` lists `nsis`, `app` and `dmg`
      together. Confirm Tauri skips `nsis` on a Mac, or split the targets per OS. The
      day-to-day `tauri.conf.json` has `bundle.active` false and only the dev window.

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
- [ ] Freeze the webcam worker on the Mac, Apple Silicon and Intel separately:
      `.venv-webcam/bin/python scripts/build_webcam_runtime.py`
      then `npm run build -- --config src-tauri/tauri.bundle.conf.json`.
      PyInstaller cannot cross-compile. The script fails if the frozen worker's CPU or GPU
      self-test fails. Intel Macs use onnxruntime 1.23.2; Apple Silicon uses 1.29.0
      (`webcam_bakeoff/requirements.txt`). macOS 14 or newer.
- [ ] The frozen worker's libraries must be signed for notarization. `Entitlements.plist`
      gives the app camera and microphone. Confirm the worker, which is a separate program
      inside the app, is allowed to open the camera under the hardened runtime. If Gatekeeper
      blocks it, sign the worker with the same identity and
      `com.apple.security.device.camera`.
- [ ] The camera prompt must name Supervisor, not Terminal or Python. In a dev build it will
      name the terminal, because dev runs the venv directly.
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

- [ ] Run voice cloning end to end. The RAM check now uses free + inactive pages, minus
      speculative pages (already counted as free). Purgeable pages are not added.

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

- [ ] Cmd-Tab, Spotlight, Mission Control, Launchpad, Notification Center, Control Center,
      and the Dock must keep the previous window counting.
- [ ] Clicking the desktop must keep the previous window. Opening a Finder folder must count
      as Finder.
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
- [ ] PyInstaller on macOS puts symlinks inside the frozen folder. Confirm Tauri's resource
      copy keeps them, and that the installed worker still runs.
- [ ] `npm run tauri dev` uses the venv, not the frozen worker. Only `npm run build` with
      `tauri.bundle.conf.json` bundles `build/webcam/`.

## 7. App behavior

- [ ] Single instance: launching a second copy focuses the running window and exits.
- [ ] Storage: the database is created under `~/Library/Application Support/app.supervisor.desktop/`
      (seen: `sessions.db` plus WAL, and a 29 s session with its spans after the first run).
      Still to try: a corrupt file is moved aside with the "starting fresh" card; a folder with
      no write permission shows the "history off" notice.
- [ ] Tray (menu bar) icon and its menu work. The status window stays on top when pinned.
- [ ] Microphone and camera permission prompts show the texts in `src-tauri/Info.plist`.
- [ ] Voice model download and clip generation work (voice runtime paths are per-OS).
