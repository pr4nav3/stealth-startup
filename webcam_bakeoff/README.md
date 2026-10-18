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

`download_models.py` only checks the pinned files. It downloads YuNet if that file is missing.

`models/yolo26s.onnx` (38 MB) is **not in git**: `webcam_bakeoff/.gitignore` ignores `models/`. A fresh checkout must get it before the worker will start. Either:

- copy it from an earlier build, e.g. `~/.cargo-target/supervisor/release/webcam/models/yolo26s.onnx`, or
- export it with `export_models.py` in `.venv-webcam-tools` (Ultralytics, telemetry off).

Either way, `download_models.py` must then pass. It rejects any file that doesn't match the pinned sha256 (`ad1008fe…`), so a fresh export only works if it reproduces that exact file.

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
