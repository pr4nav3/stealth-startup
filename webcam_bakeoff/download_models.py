#!/usr/bin/env python3
"""The ONNX files the webcam worker needs, pinned by sha256. Local disk only."""

from __future__ import annotations

import hashlib
import sys
import urllib.request
from pathlib import Path
from typing import Dict

MODELS_DIR = Path(__file__).resolve().parent / "models"

FACE_MODEL = "face_detection_yunet_2023mar.onnx"
DETECTOR = "yolo26s.onnx"

# Shipped with the app, but not in git (webcam_bakeoff/.gitignore ignores models/). The YOLO file
# is exported once with export_models.py and pinned here, so every build ships exactly the model
# that was benchmarked. See README.md for where to get it.
REQUIRED = {
    FACE_MODEL: {
        "url": "https://huggingface.co/opencv/face_detection_yunet/resolve/main/face_detection_yunet_2023mar.onnx",
        "sha256": "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
    },
    DETECTOR: {
        # The Windows export that was benchmarked (RESULTS.md).
        "sha256": "ad1008fe5d198e6a9cbf11233ce50826d2a12aba2b6f5a0987476a120141e77b",
        # Same weights exported on the Mac on 2026-10-25 with ultralytics 8.4.188 / onnx 1.23.2;
        # the Windows tools venv was older, so the bytes differ. Either file ships; both pass
        # the worker's tests and self-tests.
        "also_sha256": "00db196e9bdaa36036abd5ff6621331aee013420e411133fe674f0494bef55f9",
    },
}

USER_AGENT = "SupervisorWebcam/1"


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
        while chunk := response.read(1024 * 1024):
            out.write(chunk)
    tmp.replace(dest)


def verify_models(models_dir: Path = MODELS_DIR) -> Dict[str, Path]:
    """Every required model present with the pinned hash. Never downloads."""
    problems = []
    for name, spec in REQUIRED.items():
        path = models_dir / name
        if not path.is_file():
            problems.append(f"{name} is missing")
        elif sha256_file(path) not in (spec["sha256"], spec.get("also_sha256")):
            problems.append(f"{name} does not match its pinned sha256")
    if problems:
        raise RuntimeError(f"webcam models in {models_dir}: " + "; ".join(problems))
    return {name: models_dir / name for name in REQUIRED}


def ensure_models(models_dir: Path = MODELS_DIR) -> Dict[str, Path]:
    """Dev helper: fetch the downloadable models, then verify all of them."""
    for name, spec in REQUIRED.items():
        path = models_dir / name
        if "url" in spec and not path.is_file():
            print(f"downloading {name}", file=sys.stderr, flush=True)
            download(spec["url"], path)
    return verify_models(models_dir)


def main() -> int:
    for name, path in ensure_models().items():
        print(f"{name}  {path.stat().st_size}  {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
