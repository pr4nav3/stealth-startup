# Mac handoff: things to build and test on a real Mac

Last updated: 2026-10-24. Baseline: branch `app-v2-simplified-rules` (`main` is an ancestor
of it and will be fast-forwarded; `origin/main` is older than both, so pull the branch, not
`main`). Tracker: `plans/STATUS.md`. Holes to expect: `plans/known-issues.md`.

Nothing in this app has been compiled or run on macOS. All development so far was on Windows.
The Mac paths were type-checked from Windows with `cargo check --target aarch64-apple-darwin`
and `x86_64-apple-darwin`. Work through this file on a Mac, tick items off here, and keep
`plans/STATUS.md` and `plans/known-issues.md` current as items move.

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

- [ ] **Pull the branch.** `git fetch && git checkout app-v2-simplified-rules`. `origin/main`
      is older than the branch; do not start from it.
- [ ] **Toolchain.** Xcode Command Line Tools, Rust stable (Tauri 2 needs it; `rustup` adds
      the host target), Node 20+, Python 3.11 (Chatterbox is documented against it). Then
      `npm install`, `npm run check`, and `cargo test` from `src-tauri`. On Windows the suite
      is 201 tests (197 pass, 4 ignored); the Mac count differs because the Win32 probe tests
      are `#[cfg(windows)]`. `system_resource_queries_return_real_values` must pass: it is the
      first real run of the `macos.rs` memory and disk calls.
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
- [ ] **Run the app.** `npm run dev` (Tauri dev). `npm run dev:ui` plus
      `http://127.0.0.1:1420/mock.html` renders every screen with fixtures and no backend.
- [ ] **Seeing what the capture resolves to.** The `manual_foreground_probe` test in
      `activity.rs` prints every sample for 30 s but is `#[cfg(target_os = "windows")]`. Lift
      that gate (or add a Mac twin) first; it is the fastest way to check sections 5 and 6.
- [ ] **Bundle targets.** `src-tauri/tauri.bundle.conf.json` lists `nsis`, `app` and `dmg`
      together. Confirm Tauri skips `nsis` on a Mac, or split the targets per OS. The
      day-to-day `tauri.conf.json` has `bundle.active` false and only the dev window.

## 0. Mac code that does not exist yet

In the order a user would hit it.

- [ ] **Address-bar read: `src-tauri/src/browser_macos.rs`.** Today `read_browser` returns
      `BrowserRead::default()` ("unavailable"), so every browser on a Mac shows as Other
      websites, or as a verified title match, and no site rule can fire. Build it as the
      file's header says: read the focused window's `AXURL` through the Accessibility API
      for the sample's `process_id` (Safari, Chrome, Edge, Brave and Arc expose it on the web
      area), and report `editing` when the address field has keyboard focus. The Windows
      version (`browser_win32.rs`) is the reference for what the rest of the app expects
      from a `BrowserRead`. Needs the Accessibility permission; check it next to the Screen
      Recording check in `activity.rs` and say so in the UI when it is missing.
- [ ] **Worker lifetime.** `voice::WorkerLifetime` is a Windows job object (the voice and
      webcam workers die when the app does). On macOS the struct is empty, so a quit during
      clip generation or a webcam session can leave `python` running. Kill the children on
      exit (track the pids and `SIGTERM` them from the exit handler, or spawn them in their
      own process group). `process_ram_one` has a Mac arm; `child_pids` does not (only
      Windows and Linux are implemented, macOS returns an empty list), so the memory readout
      of the worker's children is also blind there until it gets one (`proc_listchildpids`
      or `sysctl KERN_PROC`).
- [ ] **Camera names.** `webcam::camera_names()` is Windows Media Foundation; the
      `#[cfg(not(windows))]` arm returns an empty list and Settings shows "Camera 0" to
      "Camera 3". Add an AVFoundation arm (`AVCaptureDevice` is already a dependency in
      `Cargo.toml`) in the order OpenCV opens devices, and confirm that order matches.
- [ ] **No console window flag.** The worker spawn sets `CREATE_NO_WINDOW` on Windows only.
      Confirm nothing visible opens on a Mac when a worker starts.
- [ ] **Frozen workers and signing**: section 1. These are the only parts that need a Mac to
      build at all; everything above only needs a Mac to run.

## 1. Build

- [ ] `npm run check` and `cargo test` (from `src-tauri`) pass on macOS.
- [ ] `npm run tauri dev` starts the app.
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
- [ ] RAM, per-process memory, and free disk (`libc`, plus `mach2` to release a port).
      `cargo test system_resource_queries_return_real_values` must pass. Available RAM must
      be more than 2% of total RAM (the old code counted only unused pages and was far too
      small).

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

- [ ] App name, window title, and process path are correct for: Safari, Chrome, a code
      editor, Finder, Terminal, a full-screen app.
- [ ] Window titles need Screen Recording permission. Check what happens with permission
      denied (titles missing, app names still work) and that the app asks for it clearly.
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
- [ ] Safari and Chrome on macOS. Address-bar read is a stub, so identity falls back to
      titles. Titles are not stored. Confirm the fallback is tolerable until
      `browser_macos.rs` exists.
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
- [ ] Storage: the database is created under `~/Library/Application Support/app.supervisor.desktop/`.
      A corrupt file is moved aside with the "starting fresh" card. A folder with no write
      permission shows the "history off" notice.
- [ ] Tray (menu bar) icon and its menu work. The status window stays on top when pinned.
- [ ] Microphone and camera permission prompts show the texts in `src-tauri/Info.plist`.
- [ ] Voice model download and clip generation work (voice runtime paths are per-OS).
