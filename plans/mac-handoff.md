# Mac handoff: things to test and fix on a real Mac

Nothing below has been compiled or run on macOS. All development so far was on Windows.
Work through it on a Mac before any Mac release, and tick items off in this file.

## 1. Build

- [ ] `npm run check` and `cargo test` (from `src-tauri`) pass on macOS.
- [ ] `npm run tauri dev` starts the app.
- [ ] A release build opens without Gatekeeper warnings. Code signing and notarization are
      not set up yet.

## 2. Hand-written Apple declarations (replace, then test)

These are typed by hand instead of coming from maintained bindings, the same mistake that was
fixed on Windows by switching to Microsoft's `windows` crate.

- [ ] `src-tauri/src/activity.rs`: `CGPreflightScreenCaptureAccess` and
      `CGEventSourceSecondsSinceLastEventType`, plus the hand-typed constants
      `HID_SYSTEM_STATE = 1` and `ANY_INPUT_EVENT = u32::MAX`.
      Replace with the `objc2-core-graphics` crate (the Apple-bindings family Tauri's own Mac
      code uses).
- [ ] `src-tauri/src/voice.rs`: `sysctlbyname`. Replace with `libc::sysctlbyname`.

## 3. Reading other programs' text output (replace, then test)

- [ ] `voice.rs` `process_ram_bytes_impl` runs `ps -o rss= -p <pid>` and parses the text.
      Replace with a direct call (`libc::proc_pid_rusage` or `proc_pidinfo`).
- [ ] `voice.rs` `available_disk_bytes` runs `df -k <path>` and parses the text.
      Replace with `libc::statvfs`.

## 4. Likely bug: voice cloning may refuse to run

- [ ] `voice.rs` `available_ram_bytes` on macOS counts only `vm.page_free_count` (completely
      unused pages). macOS keeps most memory in use as cache, so this number is usually tiny,
      and voice cloning will probably say "Not enough free RAM" on a Mac with plenty.
      Use the memory macOS itself reports as available (free + inactive + purgeable pages, as
      Activity Monitor does), then test voice cloning end to end.

## 5. Window capture (library: `active-win-pos-rs`)

- [ ] App name, window title, and process path are correct for: Safari, Chrome, a code
      editor, Finder, Terminal, a full-screen app.
- [ ] Window titles need Screen Recording permission. Check what happens with permission
      denied (titles missing, app names still work) and that the app asks for it clearly.
- [ ] Idle time rises when the Mac is untouched and resets on input.
- [ ] Supervisor ignores its own windows. It recognizes itself by process ID and program path
      only. Check that the path `active-win-pos-rs` reports (app bundle or the binary inside
      it) still matches, or that the process ID alone is enough.

## 6. Mac shell surfaces (not handled yet)

On Windows, Start, Search, Alt-Tab, the taskbar, and the desktop are ignored and the previous
window keeps counting. On Mac, `shell_surface` is always `false`, so nothing is ignored.

- [ ] Find out what the frontmost app is during: Cmd-Tab, Spotlight, Mission Control,
      Launchpad, Notification Center, the Dock, clicking the desktop (Finder).
- [ ] Decide which should keep the previous window, and detect them by something stable
      (bundle identifier such as `com.apple.Spotlight`, not display names).

## 7. App behavior

- [ ] Single instance: launching a second copy focuses the running window and exits.
- [ ] Storage: the database is created under `~/Library/Application Support/app.supervisor.desktop/`.
      A corrupt file is moved aside with the "starting fresh" card. A folder with no write
      permission shows the "history off" notice.
- [ ] Tray (menu bar) icon and its menu work. The status window stays on top when pinned.
- [ ] Microphone and camera permission prompts show the texts in `src-tauri/Info.plist`.
- [ ] Voice model download and clip generation work (voice runtime paths are per-OS).
