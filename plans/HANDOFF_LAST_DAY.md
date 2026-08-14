# Supervisor Handoff - 2026-08-14

This note captures the state of the repo at the handoff commit named
`Handoff on last day`. It is meant for continuing development from a Windows
machine after losing access to this Mac.

## Current Product Shape

Supervisor is a local-first desktop focus companion. The current build has:

- Active-window sampling, idle detection, focus classification, cooldowns, and
  session accounting in Rust.
- A vanilla TypeScript UI with Session, Settings, History, a compact status
  overlay, editable rules, voice-library screens, and in-memory session state.
- A Chrome extension bridge that enriches Chrome foreground windows with active
  tab URL/title/domain over localhost only.
- A local voice path: model/runtime download status, cancellation, microphone
  recording through Rust, saved reference voices, cached generated WAV clips,
  playback URLs through Tauri's asset protocol, and profile/clip deletion.

The app remains local-first: no telemetry, analytics, remote fonts, cloud voice
processing, screenshots, OCR, or AI inspection of screen contents.

## What Is Done

### Phase 1 - Activity Capture

- macOS and Windows activity metadata go through `active-win-pos-rs`.
- macOS falls back to app-name rules when Screen Recording permission hides
  window titles.
- Windows confidence remains high for normal desktop apps, with known Win32
  edge cases listed below.

### Phase 2 - Focus Engine

- No false nudge immediately after app switch.
- No nudge while idle.
- Cooldown behavior is covered by unit tests.
- Session ticker owns the 1 Hz mutation path; snapshot reads are pure.

### Phase 3 / 3.1 - UI, Overlay, Rules

- Session screen is intentionally simple before start and changes after start.
- Evidence/session log areas are internally scrollable instead of stretching
  the layout.
- The overlay shows live lock state, remaining time, progress, and tiny controls.
- Snooze suppresses nudges without pausing the elapsed session timer.
- Supervisor-owned windows are ignored for session accounting and evidence.
- Mark relevant/distracting targets the latest external window, not the main app
  or overlay.
- `acceptFirstMouse` is enabled for the main Tauri window to avoid the visual
  "first click pressed but did nothing" issue on macOS.

### Browser Bridge

- `extensions/chrome/` contains the unpacked MV3 Chrome extension.
- It sends only active-tab metadata to `127.0.0.1:43177-43187`.
- The Rust bridge binds localhost only, requires
  `X-Supervisor-Browser-Bridge: 1`, caps header/body size, and does not expose a
  permissive CORS endpoint.
- Today it only claims Chrome. Edge/Firefox should be separate adapters rather
  than widening this one casually.

### Voice

- The app downloads the Chatterbox Nano minimal model pack into app data.
- The app can download/install a separate voice runtime zip from a manifest.
- In debug/dev mode, `voice_runtime.rs` can run the source worker with
  `.venv-voice-runtime-{platform}` at repo root.
- Rust microphone capture uses `cpal`/`hound`, not browser `getUserMedia`.
- Generated phrase clips are WAV files saved under app data and exposed to the
  UI through Tauri's scoped asset protocol.
- Starter voices are still UI/library entries only; the known issue that they
  do not yet ship real audio files is intentionally not fixed in this handoff.

## Important Files

- `src-tauri/src/activity.rs` - OS activity sample shape and active-window path.
- `src-tauri/src/focus.rs` - focus classifier.
- `src-tauri/src/session.rs` - session state machine, settings, summaries, tests.
- `src-tauri/src/lib.rs` - Tauri commands, tray, overlay, ticker, bridge startup.
- `src-tauri/src/browser_bridge.rs` - local Chrome metadata bridge.
- `src-tauri/src/voice.rs` - model pack, recording, profiles, clip generation.
- `src-tauri/src/voice_runtime.rs` - runtime manifest/install/launch logic.
- `voice_worker/chatterbox_nano_worker.py` - out-of-process phrase generator.
- `scripts/build_voice_runtime.py` - native runtime zip builder.
- `extensions/chrome/` - Chrome extension bridge.
- `src/main.ts` and `src/styles.css` - vanilla UI.
- `src-tauri/Info.plist` - macOS microphone usage string.

## Mac-Specific Facts Learned

### App Data

On this Mac, Supervisor app data uses the Tauri identifier
`app.supervisor.desktop`.

The installed model pack currently exists at:

`/Users/pranav.karthik/Library/Application Support/app.supervisor.desktop/voice/model_packs/chatterbox-nano-minimal`

It is about 1.8 GB and contains:

- `added_tokens.json`
- `conds.pt`
- `merges.txt`
- `s3gen_meanflow.safetensors`
- `special_tokens_map.json`
- `t3_nano_v1.safetensors`
- `tokenizer_config.json`
- `ve.safetensors`
- `vocab.json`

Do not expect that model pack to be in Git. Transfer it separately only if you
need to preserve this exact local install; otherwise redownload it from the app.

### Mac Permissions

- `NSMicrophoneUsageDescription` is required in `src-tauri/Info.plist` for mic
  access.
- Screen Recording permission is required for macOS window titles. Without it,
  activity capture can still see app names and Supervisor falls back to app-name
  rules.
- If a permission prompt is missing or stuck, reset from System Settings or with
  macOS privacy reset tooling, then restart the app.

### Mac Voice Runtime

- `.venv-voice-runtime-macos` is ignored and should not be committed.
- It is useful for dev smoke tests but is not portable to Windows.
- A release runtime zip is represented by
  `voice_worker/release_manifest.macos-arm64.example.json`.
- Runtime zips are installed under app data, not bundled into the main process.

### Mac Voice E2E Result

The full Chatterbox Nano path was tested on this Mac with:

- Reference: `voice-samples/Derek - Fun & Energetic - Longer.wav`
- Model pack: the app-data path listed above
- Runtime: `.venv-voice-runtime-macos/bin/python`
- Result: 20 generated WAV clips, 24 kHz PCM mono, CoreAudio-readable
- Worker report from the run: `status=completed`, `clip_count=20`,
  `sample_rate=24000`, `model_load_seconds=21.588`,
  `generation_total_seconds=136.028`, `workarounds.noop_watermarker=true`

The temp output folder from that smoke test is not a durable artifact. Re-run
the worker or the app flow if you need fresh clips.

### Perth Watermarker Workaround

The worker patches `perth.PerthImplicitWatermarker` to a no-op before loading
Chatterbox Nano. This is an intentional compatibility workaround for the frozen
runtime path. It does not change the model inputs, generated waveform path, or
clip cache contract; it only omits the watermarking post-step. The worker report
records this as `workarounds.noop_watermarker=true`.

## Windows Continuation Plan

### 1. Get The Repo Running

On Windows:

```powershell
npm install
cargo test --manifest-path src-tauri/Cargo.toml
npm run check
npm run build -- --debug --no-bundle
```

Then run the app and validate:

- Start/stop session.
- Overlay appears and can be unpinned/repinned.
- Snooze does not pause the timer.
- Main-window and overlay clicks interact on first click where the OS supports
  it.
- Chrome extension can enrich active Chrome tabs.

### 2. Build A Windows Voice Runtime

PyInstaller cannot cross-compile the runtime from macOS. Build the Windows zip
on a Windows x64 host:

```powershell
py -3.11 -m venv .venv-voice-build
.\.venv-voice-build\Scripts\python -m pip install -r voice_worker\build-requirements.txt
.\.venv-voice-build\Scripts\python -m pip install -r voice_worker\requirements.txt
.\.venv-voice-build\Scripts\python scripts\build_voice_runtime.py --target windows-x64
```

The script should produce:

- `dist/supervisor-voice-runtime-windows-x64.zip`
- `dist/supervisor-voice-runtime-windows-x64.asset.json`

Attach the zip to a release, create/update a runtime manifest with a
`windows-x64` entry, and set `SUPERVISOR_VOICE_PACK_MANIFEST_URL` before
testing app-side runtime downloads.

### 3. Validate Windows Voice End To End

On Windows, test both paths:

- Dev path: create `.venv-voice-runtime-windows` at repo root and confirm
  `voice_runtime.rs` launches the Python worker directly.
- Release path: download the runtime zip through the app and confirm it launches
  the frozen `supervisor-voice-worker.exe`.

Manual voice acceptance:

- Download model/runtime.
- Record a 20-30 second sample.
- Play it back in the modal.
- Save voice and generate audio clips.
- Confirm all phrase clips play in the UI.
- Add a phrase and confirm the selected/generated profile regenerates.
- Delete only clips, then regenerate.
- Delete whole voice profile.

### 4. Validate Windows Activity Edge Cases

The Windows path is based on official Win32 APIs for foreground window, title,
PID/process path, app name, bounds, UWP host fallback, and idle time. Still test:

- Foreground `HWND` can be null during activation transitions.
- Titles can be empty or truncated.
- `OpenProcess` can fail for protected/system processes.
- UWP/ApplicationFrameHost may need fallback handling.
- DWM extended frame bounds can differ from DPI-virtualized `GetWindowRect`.
- Idle time is session-scoped.
- Secure desktops, UAC prompts, lock screen, and exclusive fullscreen are not
  normal app surfaces.

### 5. Validate Windows Microphone/Packaging

- Plain desktop builds should be governed by Windows microphone privacy settings
  and device access.
- If you later package as MSIX/Store, declare the microphone capability and test
  privacy denial paths explicitly.

## Project Next Steps

1. Copy/push the committed repo off this Mac.
2. Decide how to publish the voice runtime manifest and binaries.
3. Build the Windows runtime zip on Windows.
4. Run full Windows E2E: session, overlay, Chrome extension, voice download,
   record, generate, play, delete, regenerate.
5. Add real starter voice audio files or remove starter playback affordances
   until those files exist.
6. Add persistence for rules/settings/history/selected voice. Keep it local; a
   small SQLite or JSON-backed store is enough.
7. Add per-phrase generation progress if the worker protocol grows. Current UI
   correctly blocks with a generating modal, but the worker only reports after
   completion.
8. Decide on signing/notarization/installer strategy only after Windows voice
   runtime works.

## Current Risks

- A fresh Windows machine will not have a runtime zip until it is built and
  published for `windows-x64`.
- Generation is CPU-heavy and can take minutes for a full phrase bank.
- The model pack is large and deliberately not committed.
- Starter voices do not yet contain actual bundled audio.
- Browser enrichment is Chrome-only.
- The UI is still memory-first for rules/history/settings, except voice files
  and generated clips which are persisted under app data.

## Transfer Checklist

Commit/push or otherwise copy:

- The Git repo at `/Users/pranav.karthik/Supervisor`.
- Any remote release assets/manifest URLs used for voice runtime downloads.

Optionally copy from this Mac:

- App-data model pack:
  `/Users/pranav.karthik/Library/Application Support/app.supervisor.desktop/voice/model_packs/chatterbox-nano-minimal`
- Local sample WAVs in `voice-samples/`.

Do not bother copying:

- `.venv-voice-runtime-macos` to Windows.
- `dist/`, `build/`, `src-tauri/target/`, `node_modules/`, or Python caches.

## Verification Commands

Known good before this handoff:

```sh
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
npm run check
npm run build -- --debug --no-bundle
node --check extensions/chrome/service_worker.js
PYTHONPYCACHEPREFIX=/private/tmp/supervisor-pycache python3 -m py_compile scripts/build_voice_runtime.py voice_worker/chatterbox_nano_worker.py voice_worker/pyinstaller_hooks/hook-librosa.py
```

The cross-target guard was also checked from macOS and intentionally failed
with a clear error:

```sh
python3 scripts/build_voice_runtime.py --target windows-x64 --output /private/tmp/supervisor-voice-runtime-test.zip
```

## Reference Links

- Tauri CSP: https://v2.tauri.app/security/csp/
- Tauri asset protocol: https://v2.tauri.app/ja/security/asset-protocol/
- Tauri WebviewWindow API: https://tauri.app/reference/javascript/api/namespacewebviewwindow/
- Tauri sidecar/external binary shape: https://v2.tauri.app/learn/sidecar-nodejs/
- Tauri scopes/security boundary: https://v2.tauri.app/security/scope/
- Chrome extension permissions: https://developer.chrome.com/docs/extensions/develop/concepts/declare-permissions
- Chrome extension network requests: https://developer.chrome.com/docs/extensions/develop/concepts/network-requests
- Chrome Manifest V3 background/service worker model: https://developer.chrome.com/docs/extensions/develop/migrate/what-is-mv3
- Apple microphone plist key: https://developer.apple.com/documentation/BundleResources/Information-Property-List/NSMicrophoneUsageDescription
- Apple Info.plist privacy keys archive: https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/CocoaKeys.html
- Microsoft app capability declarations: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations
- Microsoft microphone privacy controls: https://learn.microsoft.com/en-us/windows/privacy/manage-connections-from-windows-operating-system-components-to-Microsoft-services
- Win32 DWM window attributes: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmgetwindowattribute
