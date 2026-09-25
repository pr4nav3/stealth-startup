# Webcam Bakeoff

Tiny local models for Supervisor item 8.

Labels:

- `face_in_frame`
- `gaze`: `at_pc` | `away_from_pc` | `at_phone` | `null` (no face)
- `phone_in_frame`
- `looking_at_phone` — true only when `gaze` is `at_phone`

`at_phone` means looking away from the PC **and** a phone is in the camera.
Looking down with no phone in frame is `away_from_pc`.

Head pose is YuNet 5-point yaw/pitch ratios in image space. A little
downward pitch still counts as at-PC (webcam sits above the screen). A deeper
nod does not. Phone detect is **YOLOX-s @ 640** on the full frame plus a
lower-center square, not YOLOX-nano. Target **8 fps**.

## Setup

```powershell
python -m venv .venv-webcam
.\.venv-webcam\Scripts\Activate.ps1
pip install -r webcam_bakeoff/requirements.txt
python webcam_bakeoff/download_models.py
python webcam_bakeoff/test_labels.py
```

Models land in `webcam_bakeoff/models/` (gitignored): YuNet (~230 KB) and
YOLOX-s (~36 MB). Both Apache-2.0.

## Run

```powershell
python webcam_bakeoff/run_tracker.py
python webcam_bakeoff/run_tracker.py --overlay --fps 8
```

JSON lines on stdout. Overlay also prints `yaw` and `pitch` ratios. `q`
closes the overlay. Nothing is written to disk.
