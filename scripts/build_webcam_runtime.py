#!/usr/bin/env python3
"""Freeze the webcam worker and stage it next to its models for the installer.

Output (gitignored): build/webcam/
  worker/   PyInstaller folder, run supervisor-webcam-worker
  models/   the two ONNX files the worker checks by hash

Run this on the OS you ship. PyInstaller cannot build for another OS.
From the webcam venv:  python scripts/build_webcam_runtime.py
"""

from __future__ import annotations

import platform
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / "webcam_bakeoff"))

from download_models import REQUIRED, verify_models  # noqa: E402

WORKER_NAME = "supervisor-webcam-worker"
BAKEOFF = REPO / "webcam_bakeoff"
STAGED = REPO / "build" / "webcam"


def main() -> int:
    models = verify_models(BAKEOFF / "models")
    build_root = REPO / "build" / "webcam_pyinstaller"
    if build_root.exists():
        shutil.rmtree(build_root)
    subprocess.check_call(
        [
            sys.executable,
            "-m",
            "PyInstaller",
            "--noconfirm",
            "--clean",
            "--onedir",
            "--name",
            WORKER_NAME,
            "--distpath",
            str(build_root / "dist"),
            "--workpath",
            str(build_root / "work"),
            "--specpath",
            str(build_root),
            "--collect-all",
            "onnxruntime",
            str(BAKEOFF / "run_tracker.py"),
        ],
        cwd=REPO,
    )

    frozen = build_root / "dist" / WORKER_NAME
    if STAGED.exists():
        shutil.rmtree(STAGED)
    worker_dir = STAGED / "worker"
    shutil.copytree(frozen, worker_dir)
    # Camera capture does not use this codec library. It is about 29 MB.
    for extra in worker_dir.rglob("opencv_videoio_ffmpeg*.dll"):
        extra.unlink()

    models_dir = STAGED / "models"
    models_dir.mkdir()
    for name in REQUIRED:
        shutil.copy2(models[name], models_dir / name)

    exe = worker_dir / (WORKER_NAME + (".exe" if platform.system() == "Windows" else ""))
    for device in ("cpu", "gpu"):
        result = subprocess.run(
            [str(exe), "--self-test", "--device", device, "--models", str(models_dir)],
            cwd=worker_dir,
        )
        if result.returncode != 0:
            print(f"frozen worker failed its {device} self-test", file=sys.stderr)
            return result.returncode

    size_mb = sum(p.stat().st_size for p in STAGED.rglob("*") if p.is_file()) / (1024 * 1024)
    print(f"staged {STAGED}  {size_mb:.0f} MB")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
