#!/usr/bin/env python3
"""Export YOLO11n to ONNX for the runtime worker (run in .venv-webcam-tools only)."""

from __future__ import annotations

import hashlib
import shutil
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
MODELS_DIR = SCRIPT_DIR / "models"


def main() -> int:
    try:
        from ultralytics import YOLO, settings
    except ImportError:
        print("install ultralytics in .venv-webcam-tools first", file=sys.stderr)
        return 1

    settings.update({"sync": False})
    model = YOLO("yolo11n.pt")
    export_path = Path(model.export(format="onnx", imgsz=640, simplify=True, opset=12))
    dest = MODELS_DIR / "yolo11n.onnx"
    MODELS_DIR.mkdir(parents=True, exist_ok=True)
    shutil.copy2(export_path, dest)
    digest = hashlib.sha256(dest.read_bytes()).hexdigest()
    print(f"yolo11n.onnx  {dest.stat().st_size}  sha256={digest}")
    print("Update download_models.py sha256 if this differs from the pinned release.", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
