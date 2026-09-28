#!/usr/bin/env python3
"""Freeze webcam_bakeoff/run_tracker.py with PyInstaller (optional release build)."""

from __future__ import annotations

import argparse
import hashlib
import platform
import shutil
import subprocess
import sys
from pathlib import Path


def runtime_target() -> str:
    system = platform.system().lower()
    machine = platform.machine().lower()
    os_name = {"darwin": "macos", "windows": "windows"}.get(system, system)
    arch = {"arm64": "arm64", "aarch64": "arm64", "amd64": "x64", "x86_64": "x64"}.get(
        machine, machine
    )
    return f"{os_name}-{arch}"


def sha256_file(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", default=runtime_target())
    args = parser.parse_args()
    if args.target != runtime_target():
        print("build on the target OS/arch only", file=sys.stderr)
        return 1

    repo = Path(__file__).resolve().parents[1]
    bakeoff = repo / "webcam_bakeoff"
    build_root = repo / "build" / "webcam_runtime"
    dist = build_root / "dist"
    work = build_root / "pyinstaller"
    name = "supervisor-webcam-worker"
    if shutil.which("pyinstaller") is None:
        print("pip install pyinstaller in .venv-webcam first", file=sys.stderr)
        return 1

    subprocess.check_call(
        [
            "pyinstaller",
            "--noconfirm",
            "--clean",
            "--distpath",
            str(dist),
            "--workpath",
            str(work),
            "--name",
            name,
            "--collect-all",
            "onnxruntime",
            str(bakeoff / "run_tracker.py"),
        ],
        cwd=repo,
    )
    exe = dist / name / f"{name}.exe" if platform.system() == "Windows" else dist / name / name
    if not exe.is_file():
        print(f"missing {exe}", file=sys.stderr)
        return 1
    print(f"built {exe} sha256={sha256_file(exe)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
