# Known issues

Last updated: 2026-10-17

Starter list when swapping agents. Baseline is **`feat/window-supervision`** (dirty working tree, same SHA as `main`). Window targeting and history storage are being rewritten on this branch; do not treat committed `main` as the spec.

Work tracker: `plans/STATUS.md`. This file is the holes to check, not the roadmap.

---

## Process (this checkout)

**Voice WIP is mixed into the same dirty tree as window supervision.** `src-tauri/src/voice.rs`, `voice_worker/chatterbox_nano_worker.py`, and chunks of `src/main.ts` / `src/styles.css` are a persistent `--serve` worker, not supervision. Do not commit them as one blob. Park the voice files before a supervision commit.

Old local branches (`feat/sqlite`, `feat/ui`, `feat/voice-runtime-hardening`, `feat/webcam-models`, `feat/win32-capture`, `test-features`) are fully inside `main`. Do not merge them.

---

## Window supervision / history

**macOS has no address-bar read.** `src-tauri/src/browser_macos.rs` is a stub. Mac browsers fall back to titles. Titles are never stored. A Mac is not on hand.

**Marking New Tab or "Other websites" as relevant/distracting is refused.** No rule would mean only that page. Expected for now; easy to mistake for a bug.

**Legacy shell cleanup is limited.** Known old class/ShellHost rows display as
Untracked, preserving their time. Other historical rows lack enough OS identity
to classify safely after the fact.

**App identity is still an executable stem on Windows and the captured app name
on macOS.** New app rules from Mark current use that key, and user renames do
not change it. Two unrelated apps with the same stem could still share a rule.

**Crash recovery is checkpoint-based.** Open spans now update one saved row every
15 seconds during normal polling. Time since the last successful checkpoint can
still be lost. Recovery marks sessions abandoned and recomputes saved metrics.

**Browsers are always Neutral.** No rule can name a browser (typed rules are refused, stored ones are dropped), so a browser's own pages and unidentified sites stay Neutral; only websites are classified. Schema v10 rescored history for this.

**Away is 3 minutes without input, dated back to the last input.** Shorter pauses (reading,
thinking) stay with the window in front; strictness no longer changes this. Supervisor's own
window follows the same rule, and its time is Neutral, never focus (older sessions are
re-read that way).

**Media keeps an idle user present, with gaps.** No input counts as Away unless the app in
front is playing media, for up to 45 minutes. Windows reads the system media controls; a
browser counts when the media title is in its window title (the playing tab is in front),
or when it is Edge saying only "A site is playing media" for a site without media details;
that one may be a background tab. Other languages' versions of that phrase are not listed,
so there it stays Away. Calls on Windows (Zoom, Teams, Meet) are not
media sessions and still go Away. macOS uses "keep the display awake" assertions, so music
alone stays Away there; it is untested on a Mac (see `plans/mac-handoff.md`).

**Capture failures count with Away time.** The score counts Relevant time only;
unrated time is shown apart and counts once rated. See `plans/window-history-audit.md`.

**Old history from before this rewrite:** browser time becomes "Other websites" on migration. Title rules convert or get deleted. Do not expect old YouTube title-rows to become `youtube.com`.

---

## UI

**Session changes rebuild the Session tab.** Polling is every second, but the page is rebuilt only when what it shows changes (`viewKey`); the clock and status chip are painted in place (`paintLive`). History and Settings are never rebuilt by session changes. A rebuild restores scroll, focus, and caret, and every text field keeps its value in state or sits in an editor that blocks rebuilds. While an editor or the classify popup is open, or a text field or dropdown has focus, polling only paints the clock, chip, and state slab; the rebuild waits until it closes or loses focus. Measured with a fake backend: 0 rebuilds while nothing changes, one per real change.

**PDF export is unimplemented.** Its nonfunctional button and empty handler were removed.

---

## Voice

**Starter voices are selectable with no phrase clips.** `derek` / `elise` ship a reference WAV. `starterVoiceCard` sets `selectable: true` and kind `"Bundled"`. Coverage is `0/N`. A session nudge finds no clip and stays silent (`selectedNudgeClip` → `playNudgeAudioIfNeeded` returns).

**Fallback beep leaks an `AudioContext`.** `playWebcamAlarm` in `src/main.ts` constructs one per call and never closes it. Webcam "voice clip" with no clip falls through here.

**Mic recording is an uncapped `Vec<i16>`.** `voice.rs` `capture_*_samples`. Leave the mic on and RAM grows. There is a 60 MB cap on *reference* bytes later, not on the live buffer.

**Installed voice runtime never updates.** `voice_runtime.rs` `install_runtime` returns immediately if any compatible runtime exists. A bugfix zip with the same worker API will not install.

**Generation staging (half-written profile) is fixed only in the uncommitted voice code.** Working-tree `generate_clips_inner` writes to a staging dir then replace/merge. Committed `main` without that dump may still leave a mixed profile on failure.

**Prod voice path is unproven.** No `SUPERVISOR_VOICE_PACK_MANIFEST_URL`. Tauri bundle `"active": false`. Dev venv path on Windows has not been signed off as the ship path.

---

## Webcam

**Settings preview only.** `webcam.rs` emits frames for Settings. `session.rs` never writes `phone_seconds` / `away_seconds`. Those columns stay 0. Worker is not tied to session start/stop.

**Gaze is paused.** Worker emits `gaze: null`. Strict and normal phone modes are the same signal (`plans/webcam-tracking.md`).

---

## macOS (untested)

Nothing Mac has been run. Checklist: `plans/mac-handoff.md` (original handoff; do not implement capture from it). Likely still true: RAM gate, Screen Recording titles, shell surfaces, signing/notarization, Continuity Camera.

Windows `active-win-pos-rs` is now macOS/Linux-only. Do not add it back on Windows.

---

## Ship / packaging

- Tauri `bundle.active` is `false`
- No Windows installer, no code signing
- Frozen voice zips must not be committed; SHA-256s are in `plans/STATUS.md`
- Website privacy copy was stripped; do not reinvent a privacy section
