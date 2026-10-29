# Known issues

Last updated: 2026-10-25

Starter list when swapping agents. Baseline is **`app-v2-simplified-rules`**; `origin/main` was
fast-forwarded to it on 2026-10-25, and everything since is merged into both.

Work tracker: `plans/STATUS.md`. This file is the holes to check, not the roadmap.

**The branch has run in the built app on a Mac, not yet on Windows.** The Mac pass of
2026-10-25 drove sessions, History, Settings, the Voice page and the webcam inside the real app
(`plans/mac-handoff.md`). On Windows the last live run was 2026-10-18; since then only the tests,
the mock and CI. Shared code is covered by the Mac run; Windows capture, the job objects and the
close-to-hide window are not. The Windows live pass is in `plans/STATUS.md`.

---

## Window supervision / history

**macOS reads the address through Accessibility** (`browser_macos.rs`, 2026-10-25): verified for Safari, Chrome, Firefox, Brave and Edge; Arc not seen. Chrome is asked to build its accessibility tree (`AXManualAccessibility`), which costs Chrome some CPU and memory while Supervisor runs. Chrome's first second or two after Supervisor first sees it reads by title while Chrome builds its accessibility tree. Without the Accessibility grant every browser is Other websites; see "macOS" below for the rebuild trap.

**Marking New Tab or "Other websites" as relevant/distracting is refused.** No rule would mean only that page. Expected for now; easy to mistake for a bug.

**Legacy shell cleanup is limited.** Known old class/ShellHost rows display as
Untracked, preserving their time. Other historical rows lack enough OS identity
to classify safely after the fact.

**App identity is durable since schema v11** (packaged-app id, exe + publisher, or
bundle id, one key per app). Remaining: history recorded before v11 under one stem by
two different programs cannot be split; a typed name rule that two apps share still
classifies both (new ambiguous typed names are refused); macOS bundle-id capture
needs a real Mac run.

**Crash recovery is checkpoint-based.** Open spans now update one saved row every
15 seconds during normal polling. Time since the last successful checkpoint can
still be lost. Recovery marks sessions abandoned and recomputes saved metrics.

**A browser is never a distraction on its own.** No rule can name a browser (typed rules are refused, stored ones are dropped); only websites are. A browser's own pages and unidentified sites count as work.

**Away is 3 minutes without input, dated back to the last input.** Shorter pauses (reading,
thinking) stay with the window in front; strictness no longer changes this. Supervisor's own
window follows the same rule, and its time counts as work (V2: it is never a distraction).

**Media keeps an idle user present, with gaps.** No input counts as Away unless the app in
front is playing media, for up to 45 minutes. Windows reads the system media controls; a
browser counts when the media title is in its window title (the playing tab is in front),
or when it is Edge saying only "A site is playing media" for a site without media details;
that one may be a background tab. Other languages' versions of that phrase are not listed,
so there it stays Away. Calls on Windows (Zoom, Teams, Meet) are not
media sessions and still go Away. macOS uses "keep the display awake" assertions, so music
alone stays Away there; it is untested on a Mac (see `plans/mac-handoff.md`).

**Capture failures count with Away time.** The score is time not on a distraction. Rules apply
from the next sample and never change a recorded session on their own; the user can, from a
report's Edit mode or the post-session review, move an app or site between Work and Distractions
for that one session (and rename it). See `plans/window-history-audit.md`.

**Old history from before this rewrite:** browser time becomes "Other websites" on migration. Title rules convert or get deleted. Do not expect old YouTube title-rows to become `youtube.com`.

---

## UI

**Closing the main window hides it on both OSes** (2026-10-25). The session keeps running; the tray (and the Dock on a Mac) brings it back, and Quit is in the tray menu. On Windows there is no hint that the app is still running; watch for users who think the close button quits.

**Session changes rebuild the Session tab.** Polling is every second, but the page is rebuilt only when what it shows changes (`viewKey`); the clock and status chip are painted in place (`paintLive`). History and Settings are never rebuilt by session changes. A rebuild restores scroll, focus, and caret, and every text field keeps its value in state or sits in an editor that blocks rebuilds. While an editor or a popup is open, or a text field or dropdown has focus, polling only paints the clock, chip, and state slab; the rebuild waits until it closes or loses focus. Measured with a fake backend: 0 rebuilds while nothing changes, one per real change.

**Rules are distractions only (V2).** There is no Relevant or Neutral and no classify step after a session. A site that is the work today is allowed with "Not today" (preflight chip or nudge popup) and the allowance ends with the session. Report rows show time only; the rule toggle on History rows is gone. Old reports keep the decisions recorded at the time; a rule change never changes them. Since 2026-10-28 a report's Edit mode lets the user move a row between Work and Distractions for that session and rename it; the post-session review does the same for the session just ended.

**History v4, Voice v2 and the mock agree; the app has not been checked.** Every screen was rendered through `mock.html` with fixtures, not against a real database. Places most likely to differ from the mock: the month picker's `get_focus_days` on a sparse history, session rows' miniature timelines on long sessions, the voice page's status tiles when the worker is really down.

**The distraction card shows at the nudge time, not at 8 seconds** (2026-10-25). Before, the card came up as soon as a distraction passed the short grace and the voice followed at the strictness time; now both come at the strictness time. The live Session tab still says Distracted from the short grace. A phone card speaks from the voice's Phone lines (four, bundled; starter voices ship them) and falls back to the voice's own tone when that voice has none generated. The voice panel appears only when a line actually played for that card (none picked, or none generated, leaves it out). The card's audio comes from the main window, so it needs the main window's webview alive (it is, hidden or not).

**PDF export is unimplemented.** Its nonfunctional button and empty handler were removed.

---

## Voice

**Decided on 2026-10-28.** A session start re-pins the status window on purpose ("Unpin" lasts one session). "Break over" plays nothing: the green card and its button say it. A break that runs out starts the next round by itself after one more break's length. The tray's pause item says what it will do ("Start round 3" during a break). A mute keeps its wall-clock end through a pause. **Focused % is work divided by time at the desk**: away and asleep time count for neither side, in the report, the day card and the streak days.

**Starter clips are machine-checked, not listened to.** Since 2026-10-25 the starter voices (named Derek
and Elise on screen) ship all 24 bundled lines, the 20 tone lines plus 4 Phone lines (`src-tauri/voices/<id>/`), copied into the starter profiles once. Each was
picked from four seeded takes by length against its words; nobody has heard them. A user who
edited a bundled line's words before this version keeps the original words in that clip until
they regenerate it.

**"Beta" and the Gen Z lines depend on the read.** The TTS may say "beta" as the Greek letter,
and the deadpan Gen Z set needs a flat delivery. Listen before keeping those clips.

**Prod voice path is unproven end to end.** The manifest address is compiled in (`DEFAULT_MANIFEST_URL`), but the `voice-runtime` GitHub Release does not exist yet, so an installed copy's "Check for updates" fails until it is published. Both runtimes are built and tested (macOS arm64 on the Mac, Windows x64 on the Windows machine, 2026-10-25); neither is uploaded.

**A voice worker that fails says why on the Voice page** (fixed 2026-10-25). The failed line's reason shows under Generate instead of only in a tooltip, and a failed queue no longer keeps Generate and the voice picker disabled.

---

## Webcam

**Detection latency is about a second.** The worker watches at 2 fps and bursts to 5 fps from the first phone-like frame; with 3-of-5 smoothing a phone confirms in roughly a second. On a slow CPU (YOLO26s ~90 ms a frame on a fast laptop) it can be longer. GPU mode is a toggle; if it fails, the session falls back to CPU.

**Phones below the camera are invisible.** A laptop webcam frames the face and shoulders; a phone in the lap never appears. Head-down detection is a possible later signal.

**Camera names come from Windows and macOS.** The webcam select lists devices by name (Media Foundation on Windows, an AVFoundation discovery session on macOS), in the order OpenCV opens them. Linux still shows "Camera 0" to "Camera 3". The saved index is used on every OS. Continuity Camera is not in the macOS list (OpenCV does not open it either); unconfirmed with a second camera attached.

**Preview runs at 15 pictures a second, detection at 2.** Between detections the worker resends the last labels and boxes (`--preview-fps`). A `detected` flag marks the real frames; nothing in the app reads it yet.

**`away_seconds` stays 0.** Camera Away is written as Away (`idle`) spans like input Away, so the column is unused.

**Gaze is paused.** Worker emits `gaze: null`. Strict and normal phone modes are the same signal (`plans/webcam-tracking.md`).

---

## macOS (first pass 2026-10-25)

Checklist with what was seen: `plans/mac-handoff.md`. The app builds, tests pass, the debug bundle runs, and the address read works for Safari and Chrome. What still bites:

- **An unsigned rebuild silently drops the Accessibility grant.** An ad-hoc debug bundle carries a per-build identifier, and macOS keys the grant to it: System Settings keeps showing Supervisor on, but `AXIsProcessTrusted` is false, every browser is Other websites and the Getting started step reads as not done. Build with `npm run bundle:mac-dev` after `scripts/make_mac_dev_cert.sh` (once per Mac) and the grant follows the certificate instead. There is no Developer ID certificate on any machine yet; a release needs one.
- **Webcam and voice are verified through the UI on the Mac** (camera prompt, preview, a session with the camera live, five clips generated from the Voice page). Not seen: recording a new voice (microphone prompt), the tray menu.
- **Anything the page `fetch`es from app data needs `asset:` in `connect-src`** as well as `media-src`: on macOS the asset URL is `asset://localhost/…`, on Windows `http://asset.localhost`. The clip waveforms were blank on the Mac for this (fixed 2026-10-25).
- **A signed debug build must carry `Entitlements.plist`** or the camera is refused silently with no prompt; `npm run bundle:mac-dev` does this now.
- **The session row's `nudge_count`, `drift_count` and `distracted_seconds` are written at stop**, not during the session; read the live snapshot or the spans while it runs.
- **No Developer ID.** Dev builds are signed with a local certificate; Gatekeeper rejects the DMG on any other Mac until a Developer ID signs and notarizes it.
- **The Mac YOLO export is a different file** from the benchmarked Windows one (same weights, newer export tools). `download_models.py` accepts both hashes.
- Shell surfaces (Cmd-Tab, Spotlight, Dock, desktop), media presence, Continuity Camera, Intel Macs: written, not yet run.

Windows `active-win-pos-rs` is now macOS/Linux-only. Do not add it back on Windows.

---

## Ship / packaging

- Tauri `bundle.active` is `false` in `tauri.conf.json`; releases use `tauri.bundle.conf.json`
- No GitHub Release exists yet (app or `voice-runtime`); the Mac one-line installer fails until one does
- The repository is public: everything in `plans/` and the commit history is readable by anyone
- No Windows installer, no code signing
- Frozen voice zips must not be committed; SHA-256s are in `plans/STATUS.md`
- Website privacy copy was stripped; do not reinvent a privacy section
