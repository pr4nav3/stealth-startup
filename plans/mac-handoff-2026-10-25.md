# Supervisor — Mac handoff, 2026-10-25

The day's summary for whoever picks the Mac up next. The full checklist is
`plans/mac-handoff.md`; release and install steps are in `plans/distribution.md`.

## Where things are

- Repo: `github.com/pr4nav3/stealth-startup`. Working branch `app-v2-simplified-rules`;
  `main` is fast-forwarded to it.
- Public docs in the repo: `plans/mac-handoff.md` (full checklist), `plans/distribution.md`
  (release and install), `plans/known-issues.md`, `plans/STATUS.md`.
- This Mac: MacBook Pro, Apple Silicon, macOS 14.5. Toolchain: Rust 1.99, Node 24,
  Python 3.12 (webcam), Python 3.11 via Homebrew (voice).

## State in one paragraph

Supervisor now builds, runs and is signed on macOS, and the Mac-specific code that was missing
is written and verified. A release zip is built and tested. What is left is publishing it
(two GitHub releases, which need a person), one voice clone through the installed app, and a
few decisions listed at the end.

## What was done today

**Setup**
- SSH key for this Mac added to GitHub (pr4nav3). Repo-local git author is Pranav Karthik.
- Found the clone was on old `main`; switched to `app-v2-simplified-rules` (40 commits ahead).

**Mac capture (what's in front, which site)**
- Browser address read through the Accessibility API (`browser_macos.rs`): Safari, Chrome,
  Firefox, Brave, Edge verified; other builds (Chrome Beta/Canary, Edge channels, Firefox
  Dev/Nightly, Safari Technology Preview, Opera GX, Zen, Orion, DuckDuckGo) recognized.
- Window titles through Accessibility, so Screen Recording is not needed.
- App names as Finder and the Dock show them ("Visual Studio Code", not "Code").
- Browser titles cleaned of the browser name and the Chrome profile suffix.
- An app in front with no window (last window closed, minimized, desktop) keeps the previous
  window's time instead of becoming "Device asleep". Lock screen records nothing.
- Capture runs in an autorelease pool (memory no longer grows over a session).

**Sessions and data**
- Sleep is recorded as lost time. The old clock (`Instant`) stops while a Mac sleeps, so a lid
  close stretched the session timer.
- Days bucketed with the real per-day UTC offset (daylight saving no longer moves sessions).
- Database migration v13 removed an invisible character stuck to some app names ("WhatsApp").
- Flagging while paused, empty checkpoints and a failed integrity check all fixed.

**App shell**
- Closing the main window hides it; the Dock icon brings it back.
- The status window shows on every desktop and over full-screen apps, and is sized correctly
  (macOS counts the title bar inside a window's height; it was 28 px short).
- App Nap disabled for the app so a covered Supervisor keeps tracking and nudging on time.
- Permissions: Accessibility asked once (Getting started card), Camera and Microphone asked
  before use; a silent recording is refused instead of cloned.

**UI on Mac (WebKit, not Chromium)**
- Three layout bugs fixed (Home overflow, Settings cards, report tiles); dropdowns, focus rings,
  search field and scrolling now look the same as on Windows.
- Live session: state and goal grouped properly; new "Now" line shows what is in front.
- Clipboard copy of the day card works under WebKit's rules.
- A WebKit layout check (36 screens and dialogs) runs in CI on every push.

**Voice and webcam**
- Webcam: venv built, YOLO model re-exported on the Mac (both hashes accepted), frozen worker
  signed, CPU and CoreML self-tests pass, camera list numbered like OpenCV.
- Voice: Python 3.11 venv built, Chatterbox model downloaded, a clip generated from the CLI.
  The macOS voice runtime zip is built.
- Workers can't outlive the app (own process groups, killed on quit).

**Packaging and release**
- Debug and release builds signed with the "Supervisor Dev" certificate so users' permission
  grants survive updates (`npm run bundle:mac-dev`, `sh scripts/release-mac.sh`).
- One-line installer (`scripts/install-mac.sh`): HTTPS only, checksum, certificate pinned,
  atomic replace, waits for the old app, refuses Intel and macOS < 14.
- Tested locally: fresh install, update over a running copy, bad checksum refused.

**Verification**
- 207 automated tests pass; WebKit layout check passes; flows driven inside the real app:
  start, pause, resume, stop, report, history, settings, webcam preview, a full distraction with
  the nudge card.

## Files to back up (not in git, can't be recreated)

| What | Where on this Mac | Why |
|---|---|---|
| Signing certificate + password | `~/supervisor-signing/` | Every release must use it, or users lose their permissions on update. Store in a password manager. |
| Voice runtime zip + manifest | `build/voice-runtime/` | The tested macOS voice runtime. |
| Webcam model | `webcam_bakeoff/models/yolo26s.onnx` | Builds refuse any other file. |
| Release zip + checksum | `build/release/Supervisor-macos-aarch64.zip` and `.sha256` | The build to publish. |

## Needs a person

1. **Publish two GitHub releases** (no token on this Mac):
   - App: new release, tag `v0.1.0`, mark latest; attach both files from `build/release/`.
   - Voice runtime: tag `voice-runtime`, mark **pre-release**; attach the zip and
     `voice-runtime-manifest.json` from `build/voice-runtime/`.
2. **Install with the one-liner** and do one voice clone in the installed app:
   `curl -fsSL https://raw.githubusercontent.com/pr4nav3/stealth-startup/main/scripts/install-mac.sh | sh`
3. **Quick checks:** close the main window then click the Dock icon; open Brave or Edge fresh
   and see the site appear within seconds; put a full-screen app on a distraction and check the
   nudge card shows on top.
4. **Decisions:** bundle one monospace font so labels look identical on Mac and Windows;
   replace the native audio player in the recording dialog with the app's own.

## Known, not done

- Web apps installed from Chrome or Safari count as apps, not websites.
- Python GUI apps show as "Python".
- Status window placement with a Retina laptop plus a non-Retina external display is unverified.
- Intel Macs: no build (installer refuses them clearly).
- Windows distribution not decided; Windows voice runtime not rebuilt with today's worker.
- The local keychain password used by the release scripts is in the public scripts. It only
  unlocks a keychain file on the build Mac; the certificate's own password is private. Moving it
  to an environment variable would remove even that.
