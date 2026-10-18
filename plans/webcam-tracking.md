# Webcam Tracking

Last updated: 2026-10-18

The tracker runs as a Settings preview and, when the user turns it on, during
every session (see Sessions below).

## Signals

Local only. **2 fps watching, 5 fps from the first frame that looks like a phone until it clears** (a phone confirms in about a second). Frames are never stored or uploaded.

1. **Phone in frame** — visible cell phone in the camera (any pose/lighting).
2. **User present** — face (YuNet) **or** person (YOLO COCO), smoothed.

**Gaze / at-PC / at-phone head pose is paused.** The worker emits `gaze: null`.
Phone alerts use **phone in frame** (and optional strict mode in the UI).

One noisy frame must not flip state. The worker applies k-of-n hysteresis;
the app may add another second of persistence for nudges.

## User controls

Camera is off until the user turns a check on. The app must keep working with
no webcam, or with the camera denied.

| Control | What it does |
| --- | --- |
| Phone use | Off: ignore phone signals. On: react when the check fires. |
| Phone strictness | **Strict:** fire on phone in frame. **Not strict:** same today (uses `looking_at_phone`, which equals phone in frame while gaze is paused). |
| Phone reaction | Alarm, or a cached voice clip. |

## Bakeoff stack (2026-09)

`webcam_bakeoff/`:

- YuNet face + **YOLO11n** ONNX @ 640 (phone + person).
- Full frame plus lower-center square for lap / half-frame phones.
- `run_tracker.py` prints JSON lines; `--preview` adds a JPEG for settings.
- Metrics and candidates: `webcam_bakeoff/RESULTS.md`.

Runtime: ONNX Runtime + OpenCV DNN. Export YOLO with Ultralytics in a **tools**
venv only; no Ultralytics/MediaPipe in the shipped sidecar.

## App wiring

```
webcam worker  --JSON-->  Tauri emits webcam-frame  -->  settings / session
     |                          |
  frames die here          never stored
```

One worker serves both: the Settings page asks for preview JPEGs, a session
runs it without them. Every reading goes to `SessionManager::record_webcam`
(flags only).

## Sessions

The worker runs while a session is Active or Muted and "Turn on webcam
monitoring" (saved as `webcam_enabled`) is on. Pause and Stop stop it within a
second. After a failure (no camera, denied) sessions retry every 30 s.

**Phone = its own span.** The first reading that sees a phone closes the
window's span and opens a `phone` span (marker activity `phone`, state
`phone`). It ends 2 s after the last reading that saw it. It counts as
distracted time and also as `phone_seconds`, is drift for stretches and
recovery, shows striped coral in the timeline with a Phone legend entry, and
is a "Phone" row under Distracting in Apps & sites. The phone outranks the
window and input Away; the site in front doesn't accrue time meanwhile. A span
of its own (not an overlay on the window's span) keeps the gap-free tiling
trivial: the fuzz test in `session.rs` interleaves webcam readings.

**Phone nudges.** Distracted, popup ("Phone detected.") and first nudge after
the strictness's Slipping grace: Gentle 15 s, Normal 8 s, Strict 5 s. Muted
records but never nudges or shows the popup. **One cooldown for all nudges**
(`FocusEngine::nudge_ready` / `note_nudge`): a window nudge and a phone nudge
never play within the strictness cooldown of each other. A phone nudge plays
through the same voice clip path (`metrics.nudges`, event detail `phone`).
The Settings-page alarm only sounds outside a session.

**Away.** Someone in view keeps a reader present without input, like media
(up to 45 minutes). Nobody in view for 60 s with no input since is Away,
dated back to when the camera last saw them. Input always wins. Away is
recorded, never nudged.

## Out of scope

Gaze-to-pixel, walked-away nudges, cloud models,
frame storage, MediaPipe runtime wheels. A GPU toggle exists for the preview.
