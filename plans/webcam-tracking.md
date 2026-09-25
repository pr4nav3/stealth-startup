# Webcam Tracking

Last updated: 2026-09-06

Ship this as a small, optional session feature. Bake off the detector in a
script first, then wire it. Tracker item: `plans/STATUS.md` §7.

Do not add extra product states (walked away, reaching, emotion, identity,
gaze-to-pixel). The signals below are enough.

## Signals

Local only. **8 fps.** Tiny models. Frames are never stored or uploaded.

1. **Face in frame / not in frame**
2. **When a face is in frame, exactly one gaze label:**
   - **at PC** — head toward the screen
   - **at phone** — looking away from the PC **and** a phone is in the camera
   - **away from PC** — looking away, but no phone is visible
3. **Phone in frame** (object detector, `cell phone` class)

Looking down at a hand does **not** count as phone use unless the phone
itself is in frame. Head-down with no phone is `away from PC`.

History can count hits. The UI gets these states, not pixels.

## User controls

Camera is off until the user turns a check on. The app must keep working with
no webcam, or with the camera denied.

During a session, the user can toggle:

| Control | What it does |
| --- | --- |
| Phone use | Off: ignore phone signals. On: react when the check fires. |
| Phone strictness | Only when phone use is on. **Not strict:** wait a couple of seconds; fire on `at phone`. **Strict:** fire sooner; also fire on `phone in frame` while still looking at the PC. |
| Phone reaction | Alarm, or a cached voice clip (same playback path as other nudges). |

Face in/out of frame is always tracked while the camera is on, so later
toggles can use it. Do not invent a “you walked away” nudge unless we add
an explicit toggle later.

One noisy frame must not fire. A second or two of persistence is the only
debounce this needs; reuse the existing snooze / cooldown so camera alerts
do not stack on top of app-drift nudges.

## How to build it

### Bakeoff (script, not the app)

`webcam_bakeoff/`, same shape as the voice bakeoff:

- Grab the webcam at **8 fps**.
- YuNet face + 5 landmarks. Yaw/pitch from image-space ratios (not solvePnP
  Euler angles — those wrap to ~-170° on a normal screen-facing face).
  Mild downward pitch is still at-PC; a deeper nod is not.
- YOLOX-s @ 640, COCO `cell phone` only. Full frame plus a lower-center
  square so lap / half-in-frame phones have enough pixels.
- Print timestamps and labels. Optional overlay. No saved video unless an
  explicit dump flag is set.

This matches the simple local stacks in Focus-Guardian / facetrak (head pose
+ phone box) without MediaPipe wheels, Ultralytics, or PnP Euler wrap bugs.

Gate:

| Signal | Pass if |
| --- | --- |
| Face in / out | Almost never says “gone” while a face is clearly in frame |
| At PC vs away | Looking at the laptop reads as PC; looking down with no phone reads as away |
| At phone | Looking down with a phone in hand reads as phone; the same head pose with no phone in frame stays away |
| Phone in frame | Catches a phone held up, in the lap, or half in the lower frame |
| Budget | Sidecar, kill on session end. Well under 500 MB extra RAM |

ONNX Runtime + OpenCV DNN. Apache-licensed models only. Do not ship official
MediaPipe PyPI wheels (they send usage metrics). Do not use Ultralytics YOLO
(AGPL).

### App wiring

No vision model in the main process.

```
webcam worker  --4 fps-->  JSON states  -->  session checks
     |                          |
  frames die here          never stored
```

Session-only. Kill the worker on stop. macOS needs `NSCameraUsageDescription`
when this is bundled; Windows uses OS camera privacy. Capture in the sidecar,
not the Tauri WebView.

## Out of scope

Walked-away events, reaching, hands models, dual-monitor special cases,
custom action networks, GPU, always-on camera, frame preview on the session
screen (a one-time “can you see me?” check is fine).

## Order

Bakeoff can run on Windows whenever a webcam is handy. Do not block items
1–4 on it. Wire into the app after the bakeoff gate passes and the Windows
voice loop can play a clip.
