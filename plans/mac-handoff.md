# Mac handoff: things to test and fix on a real Mac

**Original Mac checklist** from before website-identity supervision. Baseline now is `main`: Windows reads the address bar, titles are never stored, `browser_macos.rs` is a stub. Do not implement window capture or title persistence from this file. Build, signing, permissions, RAM, shell surfaces, and webcam packaging below can still be useful.

Nothing below has been compiled or run on macOS. All development so far was on Windows.
Work through it on a Mac before any Mac release, and tick items off in this file.

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
