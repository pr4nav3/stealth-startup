#!/usr/bin/env python3
"""Build a Supervisor voice runtime zip for GitHub Releases.

Run this on the target OS/architecture from a clean Python 3.11 environment
with voice_worker/build-requirements.txt and voice_worker/requirements.txt
installed. The zip layout intentionally has the worker executable at archive
root because the Rust installer expects that simple contract.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path
from typing import Sequence


WORKER_API_VERSION = 1
WORKER_NAME = "supervisor-voice-worker"
HIDDEN_IMPORTS = [
    "scipy._cyutility",
]
COPY_METADATA = [
    "requests",
]


def runtime_target() -> str:
    system = platform.system().lower()
    machine = platform.machine().lower()
    os_name = {"darwin": "macos", "windows": "windows"}.get(system, system)
    arch = {
        "arm64": "arm64",
        "aarch64": "arm64",
        "amd64": "x64",
        "x86_64": "x64",
    }.get(machine, machine)
    return f"{os_name}-{arch}"


def ensure_native_target(target: str) -> None:
    host = runtime_target()
    if target != host:
        raise RuntimeError(
            "PyInstaller cannot cross-compile the Supervisor voice runtime. "
            f"Build {target} on a {target} host; this host is {host}."
        )


def sha256_file(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def zip_dir(source: Path, output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(source.rglob("*")):
            if path.is_dir():
                continue
            archive.write(path, path.relative_to(source))


def build_runtime(args: argparse.Namespace) -> Path:
    repo = Path(__file__).resolve().parents[1]
    worker = repo / "voice_worker" / "chatterbox_nano_worker.py"
    hooks_dir = repo / "voice_worker" / "pyinstaller_hooks"
    build_root = repo / "build" / "voice_runtime"
    dist_dir = build_root / "dist"
    work_dir = build_root / "pyinstaller"
    spec_dir = build_root / "spec"
    runtime_dir = dist_dir / WORKER_NAME
    target = args.target or runtime_target()
    ensure_native_target(target)
    executable = f"{WORKER_NAME}.exe" if target.startswith("windows-") else WORKER_NAME
    output = args.output or (repo / "dist" / f"supervisor-voice-runtime-{target}.zip")

    if runtime_dir.exists():
        shutil.rmtree(runtime_dir)
    env = os.environ.copy()
    env.setdefault("PYINSTALLER_CONFIG_DIR", str(build_root / "config"))
    env.setdefault("MPLCONFIGDIR", str(build_root / "matplotlib"))
    command = [
        sys.executable,
        "-m",
        "PyInstaller",
        "--noconfirm",
        "--clean",
        # Transformers' lazy import machinery can ask importlib/inspect for
        # external .pyc paths in frozen apps. noarchive keeps those bytecode
        # files beside the collected sources instead of only inside PYZ.
        "--debug",
        "noarchive",
        "--onedir",
        "--name",
        WORKER_NAME,
        "--distpath",
        str(dist_dir),
        "--workpath",
        str(work_dir),
        "--specpath",
        str(spec_dir),
        "--additional-hooks-dir",
        str(hooks_dir),
    ]
    for module in HIDDEN_IMPORTS:
        command.extend(["--hidden-import", module])
    for package in COPY_METADATA:
        command.extend(["--copy-metadata", package])
    command.append(str(worker))

    subprocess.run(
        command,
        check=True,
        cwd=repo,
        env=env,
    )

    metadata = {
        "schema_version": 1,
        "worker_api": WORKER_API_VERSION,
        "target": target,
        "executable": executable,
        "engine": "chatterbox-nano",
    }
    (runtime_dir / "runtime_manifest.json").write_text(
        json.dumps(metadata, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    zip_dir(runtime_dir, output)
    return output


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build a Supervisor voice runtime zip.")
    parser.add_argument("--target", help="Override release target, for example macos-arm64 or windows-x64.")
    parser.add_argument("--output", type=Path, help="Output zip path.")
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    output = build_runtime(args)
    asset = {
        "target": args.target or runtime_target(),
        "url": "<attach this zip to a GitHub Release and paste the asset URL here>",
        "sha256": sha256_file(output),
        "bytes": output.stat().st_size,
        "executable": f"{WORKER_NAME}.exe" if (args.target or runtime_target()).startswith("windows-") else WORKER_NAME,
    }
    sidecar = output.with_suffix(".asset.json")
    sidecar.write_text(json.dumps(asset, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(output)
    print(sidecar)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
