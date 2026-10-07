#!/usr/bin/env python3
"""Export YOLO26s to ONNX. Run in .venv-webcam-tools only. Telemetry stays off."""

from __future__ import annotations

import hashlib
import shutil
import sys
from pathlib import Path

DEST = Path(__file__).resolve().parent / "models" / "yolo26s.onnx"


def main() -> int:
    try:
        from ultralytics import YOLO, settings
    except ImportError:
        print("install ultralytics in .venv-webcam-tools first", file=sys.stderr)
        return 1

    settings.update({"sync": False})
    exported = Path(YOLO("yolo26s.pt").export(format="onnx", imgsz=640, simplify=True, device="cpu"))
    DEST.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(exported, DEST)
    digest = hashlib.sha256(DEST.read_bytes()).hexdigest()
    print(f"{DEST.name}  {DEST.stat().st_size}  sha256={digest}")
    print("Put that sha256 in download_models.py before shipping.", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
