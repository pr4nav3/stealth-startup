# Known issues

Last updated: 2026-10-24

Starter list when swapping agents. Baseline is **`app-v2-simplified-rules`** (`main` is an
ancestor; fast-forward it when ready). Everything described here is committed on that branch.

Work tracker: `plans/STATUS.md`. This file is the holes to check, not the roadmap.

**Nothing on the branch has run in the built app.** The V2 rules model, History v4, the Voice
page, the Settings polish and the schema v12 migration were proved by `cargo test`, `npm run
check` and the browser mock only. The last live run was on Windows on 2026-10-18, before any of
it. Expect first-run bugs on both OSes; the Windows live pass is item 2 in `plans/STATUS.md`.

---

## Window supervision / history

**macOS has no address-bar read.** `src-tauri/src/browser_macos.rs` is a stub. Mac browsers fall back to titles. Titles are never stored. A Mac is not on hand.

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

**Capture failures count with Away time.** The score is time not on a distraction;
rules apply from the next sample and never change a recorded session. See `plans/window-history-audit.md`.

**Old history from before this rewrite:** browser time becomes "Other websites" on migration. Title rules convert or get deleted. Do not expect old YouTube title-rows to become `youtube.com`.

---

## UI

**Session changes rebuild the Session tab.** Polling is every second, but the page is rebuilt only when what it shows changes (`viewKey`); the clock and status chip are painted in place (`paintLive`). History and Settings are never rebuilt by session changes. A rebuild restores scroll, focus, and caret, and every text field keeps its value in state or sits in an editor that blocks rebuilds. While an editor or a popup is open, or a text field or dropdown has focus, polling only paints the clock, chip, and state slab; the rebuild waits until it closes or loses focus. Measured with a fake backend: 0 rebuilds while nothing changes, one per real change.

**Rules are distractions only (V2).** There is no Relevant or Neutral and no classify step after a session. A site that is the work today is allowed with "Not today" (preflight chip or nudge popup) and the allowance ends with the session. Report rows show time only; the rule toggle on History rows is gone. Old reports keep the decisions recorded at the time; a rule change never changes them.

**History v4, Voice v2 and the mock agree; the app has not been checked.** Every screen was rendered through `mock.html` with fixtures, not against a real database. Places most likely to differ from the mock: the month picker's `get_focus_days` on a sparse history, session rows' miniature timelines on long sessions, the voice page's status tiles when the worker is really down.

**PDF export is unimplemented.** Its nonfunctional button and empty handler were removed.

---

## Voice

**Starter voices have no phrase clips yet.** `derek` / `elise` ship a reference WAV only; the
20 phrase clips are being generated by hand with the worker's one-shot CLI. Until they ship, a
session nudge on a starter voice finds no clip and stays silent (`selectedNudgeClip` →
`playNudge` returns).

**"Beta" and the Gen Z lines depend on the read.** The TTS may say "beta" as the Greek letter,
and the deadpan Gen Z set needs a flat delivery. Listen before keeping those clips.

**Prod voice path is unproven.** No `SUPERVISOR_VOICE_PACK_MANIFEST_URL`. Tauri bundle `"active": false`. Dev venv path on Windows has not been signed off as the ship path.

---

## Webcam

**Detection latency is about a second.** The worker watches at 2 fps and bursts to 5 fps from the first phone-like frame; with 3-of-5 smoothing a phone confirms in roughly a second. On a slow CPU (YOLO26s ~90 ms a frame on a fast laptop) it can be longer. GPU mode is a toggle; if it fails, the session falls back to CPU.

**Phones below the camera are invisible.** A laptop webcam frames the face and shoulders; a phone in the lap never appears. Head-down detection is a possible later signal.

**Camera names come from Windows only.** The webcam select lists devices by name through Media Foundation, in the order OpenCV opens them. On macOS and Linux `camera_names()` returns an empty list and the select shows "Camera 0" to "Camera 3". The saved index is used on every OS.

**Preview runs at 15 pictures a second, detection at 2.** Between detections the worker resends the last labels and boxes (`--preview-fps`). A `detected` flag marks the real frames; nothing in the app reads it yet.

**`away_seconds` stays 0.** Camera Away is written as Away (`idle`) spans like input Away, so the column is unused.

**Gaze is paused.** Worker emits `gaze: null`. Strict and normal phone modes are the same signal (`plans/webcam-tracking.md`).

---

## macOS (untested)

Nothing Mac has been run. Checklist and the list of Mac code that does not exist yet: `plans/mac-handoff.md`. The gaps, in the order they bite:

- **No address-bar read.** `browser_macos.rs` returns "unavailable", so every browser is Other websites or a title match. Site rules cannot fire on a Mac until it reads `AXURL` through Accessibility.
- **Workers can outlive the app.** `WorkerLifetime` (voice.rs) is a Windows job object; on macOS it is empty. Quit while a voice clip generates or a session has the webcam on and the Python process may stay behind. Needs a kill on exit or a process group.
- **No camera names** (above). The select still works by index.
- **`CREATE_NO_WINDOW` and the Win32 paths** are `#[cfg(windows)]`; the Mac spawns the worker plainly. Confirm no Terminal window opens.
- Permissions (Screen Recording for titles, Accessibility for the address bar, Camera, Microphone), the RAM gate, shell surfaces, media presence, bundle-id identity, Continuity Camera, signing and notarization: all written, none run.

Windows `active-win-pos-rs` is now macOS/Linux-only. Do not add it back on Windows.

---

## Ship / packaging

- Tauri `bundle.active` is `false`
- No Windows installer, no code signing
- Frozen voice zips must not be committed; SHA-256s are in `plans/STATUS.md`
- Website privacy copy was stripped; do not reinvent a privacy section
