# Webcam Tracking

Last updated: 2026-10-07

The tracker runs as a Settings preview. Phone and away time are not written
during a real session yet. That wiring, and how strictness changes a voice
nudge, is the webcam branch in `plans/STATUS.md`.

## Signals

Local only. **~8 fps target, 4 fps floor.** Frames are never stored or uploaded.

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

Session-only worker in Python; kill on stop.

## Out of scope

Gaze-to-pixel, walked-away nudges (without a toggle), cloud models,
frame storage, MediaPipe runtime wheels. A GPU toggle exists for the preview.
