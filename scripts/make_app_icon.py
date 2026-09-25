#!/usr/bin/env python3
"""Generate placeholder Supervisor window/tray icons for Windows tauri-build."""

from __future__ import annotations

import struct
import zlib
from pathlib import Path


INK = (16, 16, 16, 255)
PAPER = (244, 234, 215, 255)
ACID = (183, 255, 60, 255)
COBALT = (50, 103, 255, 255)
CLEAR = (0, 0, 0, 0)


def lerp(a: int, b: int, t: float) -> int:
    return int(round(a + (b - a) * t))


def mix(c0: tuple[int, int, int, int], c1: tuple[int, int, int, int], t: float) -> tuple[int, int, int, int]:
    return tuple(lerp(a, b, t) for a, b in zip(c0, c1))  # type: ignore[return-value]


def draw_icon(size: int) -> list[list[tuple[int, int, int, int]]]:
    pixels = [[CLEAR for _ in range(size)] for _ in range(size)]
    cx = cy = (size - 1) / 2
    # Leave a 1px transparent margin so the ICO doesn't look like a hard square in the tray.
    outer = size * 0.46
    border = max(1.2, size * 0.06)
    acid_r = size * 0.22
    cobalt_r = size * 0.13
    pupil_r = size * 0.055

    for y in range(size):
        for x in range(size):
            dx = x - cx
            dy = y - cy
            dist = (dx * dx + dy * dy) ** 0.5
            if dist > outer:
                continue
            if dist > outer - border:
                pixels[y][x] = INK
            elif dist > acid_r:
                pixels[y][x] = PAPER
            elif dist > cobalt_r:
                pixels[y][x] = ACID
            elif dist > pupil_r:
                pixels[y][x] = COBALT
            else:
                pixels[y][x] = INK
    return pixels


def png_bytes(pixels: list[list[tuple[int, int, int, int]]]) -> bytes:
    height = len(pixels)
    width = len(pixels[0])
    raw = b"".join(
        b"\x00" + b"".join(struct.pack("BBBB", *pixel) for pixel in row) for row in pixels
    )

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico_bytes(images: dict[int, bytes]) -> bytes:
    sizes = sorted(images)
    count = len(sizes)
    offset = 6 + 16 * count
    entries = b""
    payloads = b""
    for size in sizes:
        data = images[size]
        width_byte = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", width_byte, width_byte, 0, 0, 1, 32, len(data), offset)
        payloads += data
        offset += len(data)
    return struct.pack("<HHH", 0, 1, count) + entries + payloads


def main() -> None:
    repo = Path(__file__).resolve().parents[1]
    icons = repo / "src-tauri" / "icons"
    icons.mkdir(parents=True, exist_ok=True)

    pngs = {size: png_bytes(draw_icon(size)) for size in (16, 32, 48, 128, 256)}
    (icons / "32x32.png").write_bytes(pngs[32])
    (icons / "128x128.png").write_bytes(pngs[128])
    (icons / "icon.png").write_bytes(pngs[256])
    (icons / "icon.ico").write_bytes(ico_bytes(pngs))
    print(f"wrote icons in {icons}")


if __name__ == "__main__":
    main()
