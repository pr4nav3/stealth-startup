# Webcam worker

Shipped inside the app. It reports whether a phone is in frame and whether the user is present. Gaze is off.

The model is YOLO26s (`models/yolo26s.onnx`) plus YuNet for faces. See `RESULTS.md`.

## Dev setup

```powershell
python -m venv .venv-webcam
.\.venv-webcam\Scripts\python -m pip install -r webcam_bakeoff\build-requirements.txt
.\.venv-webcam\Scripts\python webcam_bakeoff\download_models.py
.\.venv-webcam\Scripts\python webcam_bakeoff\run_tracker.py --preview
.\.venv-webcam\Scripts\python webcam_bakeoff\run_tracker.py --self-test --device gpu
```

On macOS the venv path is `.venv-webcam/bin/python`. macOS 14 or newer.

`download_models.py` only checks the pinned files. It downloads YuNet if that file is missing. YOLO26s is produced by `export_models.py` in `.venv-webcam-tools` (Ultralytics, telemetry off) and then committed.

## Freeze it into the installer

```powershell
.\.venv-webcam\Scripts\python scripts\build_webcam_runtime.py
npm run build -- --config src-tauri/tauri.bundle.conf.json
```

Build on the OS you ship. PyInstaller cannot cross-compile. The script refuses to finish if the frozen worker fails its CPU or GPU self-test.

## Checks

```powershell
.\.venv-webcam\Scripts\python webcam_bakeoff\test_labels.py
.\.venv-webcam\Scripts\python webcam_bakeoff\test_detectors.py
```
