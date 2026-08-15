# Webcam Tracking (Post-V1)

Last updated: 2026-08-15

Work this as a **bakeoff first**, same shape as the voice bakeoff. Do not put a
camera into the Tauri app until a tiny local detector is reliable on a real
desk. Tracker item: `plans/STATUS.md` §8.

## What already exists

Nothing in code. Camera is a V1 non-goal:

- Privacy promise: no camera in V1, local metadata only.
- Capture table: camera frames are V2 only.
- Acceptance: app must work without a webcam.

The UI mocks already pretend this exists (`phone reaches` / `phone hits` in
the style guides). That is copy, not a pipeline.

Voice is the wrong analogy for *when* the model runs. Voice is generated
ahead of time and only played during a session. Camera has to run **live**
during a session, so the model must stay small.

## Goal

Simple real-life drift, local only:

1. **In seat** — a face is in the webcam.
2. **Looking away** — a face is present but turned off-screen for a few seconds.
3. **Phone visible** — a phone is in the frame.

Compose the product events from those:

- Walked away: no face, and input idle already agrees.
- On the phone: face still there, phone visible, maybe looking down.
- Reaching: phone enters the frame near a hand, or a hand leaves the keyboard
  area toward the lap. Do not train a custom “reach” network for this.

That is enough. Skip emotion, identity, gaze-to-pixel, screenshots of the
monitor, and anything that needs a GPU.

## How to build it

### Bakeoff (now, side by side)

A short-lived Python script, not the app:

- Grab webcam at **1–2 fps**, not a video stream.
- Run off-the-shelf tiny models (MediaPipe face/pose/hands, plus a nano
  object detector with a `cell phone` class).
- Print timestamps and labels. Optional developer overlay. No saved video
  unless you explicitly dump a local debug clip.
- Measure CPU, RAM, false positives at a normal desk (phone on the table,
  phone in hand, empty chair, looking at a second monitor).

Gate before app work:

| Signal | Pass if |
| --- | --- |
| In seat | Almost never says “gone” while you are clearly in frame |
| Phone visible | Catches phone-in-hand; does not fire on a phone sitting unused on the desk for long (or we treat desk-phone as a weak signal) |
| Looking away | Second monitor should not look like a phone session |
| Budget | Fits in a sidecar; kill it when the session ends. Prefer well under 500 MB extra RAM |

### App wiring (later)

Copy the voice-sidecar rule: **no vision model in the main process**.

```
webcam worker  --1-2 fps-->  JSON events  -->  focus engine
     |                         |
  frames die here         never stored
```

The UI gets states (`in_seat`, `looking_away`, `phone_visible`), not pixels.
No preview in the main session screen unless we add a one-time calibration
view. Off by default. Session-only. Kill the worker on stop.

Feed events into the existing focus state machine with the same hysteresis
and snooze rules. One frame of a phone must not nudge.

### Privacy

- Local inference only. No upload, no telemetry, no frame cache by default.
- Camera permission copy on both OS-es (Mac will need `NSCameraUsageDescription`
  later; Windows uses OS camera privacy).
- Easy off. App must keep working with the camera denied or missing.
- Do not log bounding boxes or images into session history. A count like
  “2 phone hits” is enough.

## What to improve vs a naive build

1. **Do not start with action recognition.** “Reaching for phone” is a
   composition of face + phone + hand. A custom visual model is extra work
   for a worse, creepier product.
2. **Keep it optional.** Window rules remain the default supervisor. Camera
   is a second signal for people who want it.
3. **Low FPS is the feature.** It saves CPU, RAM, and makes the privacy
   story honest.
4. **Desk phones will false-positive.** Need a stable-phone-on-desk rule
   (bbox barely moves, or it has been visible since session start) or the
   metric becomes noise.
5. **Second monitors look like “looking away.”** Require looking-away *and*
   (phone visible or hands gone) before a nudge, or people with a side
   screen will hate it.
6. **Same cooldowns as app drift.** Camera should not become a second,
   nagging personality.

## Order

Bakeoff can start on Windows whenever a webcam is handy. Do not block items
1–4 on it. Wire into Supervisor only after the bakeoff gate passes and the
Windows voice loop is usable.
