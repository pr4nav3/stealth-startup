#!/usr/bin/env python3
"""Fetch the ONNX files the webcam bakeoff needs. Local disk only."""

from __future__ import annotations

import hashlib
import sys
import urllib.request
from pathlib import Path
from typing import Dict

MODELS_DIR = Path(__file__).resolve().parent / "models"

# YuNet 2023mar from OpenCV Zoo (Apache-2.0). Face box + 5 landmarks.
# YOLOX-s from Megvii (Apache-2.0). COCO, class 67 = cell phone.
# Nano could not see handheld / edge-on / half-cropped phones; s @ 640 can.
MODELS = {
    "face_detection_yunet_2023mar.onnx": {
        "url": "https://huggingface.co/opencv/face_detection_yunet/resolve/main/face_detection_yunet_2023mar.onnx",
        "sha256": "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
        "min_bytes": 200_000,
    },
    "yolox_s.onnx": {
        "url": "https://github.com/Megvii-BaseDetection/YOLOX/releases/download/0.1.1rc0/yolox_s.onnx",
        "sha256": "c5c2d13e59ae883e6af3b45daea64af4833a4951c92d116ec270d9ddbe998063",
        "min_bytes": 30_000_000,
    },
}

USER_AGENT = "SupervisorWebcamBakeoff/1"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download(url: str, dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".part")
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=180) as response, tmp.open("wb") as out:
        while True:
            chunk = response.read(1024 * 1024)
            if not chunk:
                break
            out.write(chunk)
    tmp.replace(dest)


def ensure_models(models_dir: Path = MODELS_DIR) -> Dict[str, Path]:
    paths = {}
    for name, spec in MODELS.items():
        path = models_dir / name
        if path.is_file() and path.stat().st_size >= spec["min_bytes"]:
            digest = sha256_file(path)
            if digest == spec["sha256"]:
                paths[name] = path
                continue
            path.unlink()

        print(f"downloading {name}", file=sys.stderr, flush=True)
        download(spec["url"], path)
        if path.stat().st_size < spec["min_bytes"]:
            raise RuntimeError(f"{name} is too small; got a Git LFS pointer or a truncated file")
        digest = sha256_file(path)
        if digest != spec["sha256"]:
            raise RuntimeError(f"{name} sha256 mismatch: {digest}")
        paths[name] = path
    return paths


def main() -> int:
    try:
        paths = ensure_models()
    except Exception as exc:
        print(f"download failed: {exc}", file=sys.stderr)
        return 1
    for name, path in paths.items():
        print(f"{name}  {path.stat().st_size}  {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
