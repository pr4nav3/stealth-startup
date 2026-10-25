#!/usr/bin/env python3
"""Build the published voice-runtime manifest from runtime asset metadata."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Sequence
from urllib.parse import urlparse


WORKER_API_VERSION = 1
REQUIRED_TARGETS = {"macos-arm64", "windows-x64"}


def load_asset(path: Path) -> dict[str, Any]:
    asset = json.loads(path.read_text(encoding="utf-8"))
    target = asset.get("target")
    url = asset.get("url")
    checksum = asset.get("sha256")
    byte_count = asset.get("bytes")
    executable = asset.get("executable")

    if target not in REQUIRED_TARGETS:
        raise ValueError(f"{path}: unsupported target {target!r}")
    parsed_url = urlparse(url) if isinstance(url, str) else None
    if parsed_url is None or parsed_url.scheme != "https" or not parsed_url.netloc:
        raise ValueError(f"{path}: URL must be HTTPS")
    if (
        not isinstance(checksum, str)
        or len(checksum) != 64
        or any(character not in "0123456789abcdefABCDEF" for character in checksum)
    ):
        raise ValueError(f"{path}: sha256 must contain 64 hexadecimal characters")
    if isinstance(byte_count, bool) or not isinstance(byte_count, int) or byte_count <= 0:
        raise ValueError(f"{path}: bytes must be a positive integer")
    if (
        not isinstance(executable, str)
        or not executable
        or "/" in executable
        or "\\" in executable
        or executable in {".", ".."}
    ):
        raise ValueError(f"{path}: executable must be a file at archive root")

    return {
        "target": target,
        "url": url,
        "sha256": checksum.lower(),
        "bytes": byte_count,
        "executable": executable,
    }


def build_manifest(version: str, asset_paths: Sequence[Path], partial: bool = False) -> dict[str, Any]:
    version = version.strip()
    if not version:
        raise ValueError("version must not be empty")

    assets = [load_asset(path) for path in asset_paths]
    targets = [asset["target"] for asset in assets]
    if len(targets) != len(set(targets)):
        raise ValueError("runtime targets must be unique")
    missing = REQUIRED_TARGETS.difference(targets)
    if missing and not partial:
        raise ValueError(f"missing runtime targets: {', '.join(sorted(missing))}")

    return {
        "version": version,
        "worker_api": WORKER_API_VERSION,
        "runtimes": sorted(assets, key=lambda asset: asset["target"]),
    }


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build a Supervisor voice-runtime release manifest."
    )
    parser.add_argument("--version", required=True, help="Release version.")
    parser.add_argument("--output", type=Path, required=True, help="Manifest output path.")
    parser.add_argument("assets", nargs="+", type=Path, help="Runtime .asset.json files.")
    parser.add_argument("--partial", action="store_true", help="Publish without every target; the missing ones get a clear 'not published' error in the app.")
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    manifest = build_manifest(args.version, args.assets, args.partial)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    temporary = args.output.with_suffix(args.output.suffix + ".tmp")
    temporary.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    temporary.replace(args.output)
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
