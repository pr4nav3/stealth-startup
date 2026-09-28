# Webcam bakeoff

Local-only signals for Supervisor:

1. **Phone in frame** — any visible cell phone (YOLO11n COCO class, dual-view).
2. **User present** — YuNet face **or** YOLO person, with hysteresis.

**Gaze / head pose is paused.** Worker always emits `gaze: null` and `looking_at_phone` = smoothed phone state.

## Runtime (lean)

```powershell
python -m venv .venv-webcam
.\.venv-webcam\Scripts\pip install -r webcam_bakeoff\requirements.txt
.\.venv-webcam-tools\Scripts\python webcam_bakeoff\export_models.py   # once, needs ultralytics
.\.venv-webcam\Scripts\python webcam_bakeoff\download_models.py
.\.venv-webcam\Scripts\python webcam_bakeoff\run_tracker.py --preview --fps 8
```

Export uses Ultralytics **offline** only (`sync=False`). Inference is **onnxruntime** + OpenCV — no torch/ultralytics in the runtime venv.

## Eval / benchmarks

Heavy tools live in `.venv-webcam-tools` (gitignored). See `eval/build_eval_set.py` and `benchmark.py`. Results: `RESULTS.md`.

## Tests

```powershell
.\.venv-webcam\Scripts\python -m pytest webcam_bakeoff
# or
.\.venv-webcam\Scripts\python webcam_bakeoff\test_labels.py
```
