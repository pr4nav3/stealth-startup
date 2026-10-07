#!/usr/bin/env python3
"""Build a local labeled eval set from public COCO val + optional YouTube frames.

Outputs (gitignored under eval/):
  images/*.jpg
  labels.csv  — image,phone_visible,person_present,source,split

COCO category ids: person=1, cell phone=77.
Hard negatives: COCO images with remote/mouse/book/cup but no cell phone.
"""

from __future__ import annotations

import argparse
import csv
import json
import random
import shutil
import subprocess
import sys
import urllib.request
from pathlib import Path

EVAL_DIR = Path(__file__).resolve().parent
IMAGES_DIR = EVAL_DIR / "images"
LABELS_CSV = EVAL_DIR / "labels.csv"
COCO_DIR = EVAL_DIR / "coco"
ANNOTATIONS_URL = (
    "http://images.cocodataset.org/annotations/annotations_trainval2017.zip"
)
IMAGES_BASE = "http://images.cocodataset.org/val2017"

PERSON_CAT = 1
PHONE_CAT = 77
HARD_NEG_CATS = {
    65: "remote",
    64: "mouse",
    73: "book",
    41: "cup",
    76: "keyboard",
    39: "bottle",
    74: "clock",
}


def download_file(url: str, dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.is_file() and dest.stat().st_size > 1000:
        return
    print(f"download {url}", file=sys.stderr, flush=True)
    req = urllib.request.Request(url, headers={"User-Agent": "SupervisorEval/1"})
    part = dest.with_suffix(dest.suffix + ".part")
    for attempt in range(3):
        try:
            with urllib.request.urlopen(req, timeout=60) as resp, part.open("wb") as out:
                shutil.copyfileobj(resp, out)
            part.replace(dest)
            return
        except (TimeoutError, OSError) as exc:
            if attempt == 2:
                raise
            print(f"retry {url}: {exc}", file=sys.stderr, flush=True)


def ensure_coco_annotations() -> Path:
    ann_zip = COCO_DIR / "annotations_trainval2017.zip"
    ann_json = COCO_DIR / "annotations" / "instances_val2017.json"
    if not ann_json.is_file():
        download_file(ANNOTATIONS_URL, ann_zip)
        import zipfile

        with zipfile.ZipFile(ann_zip, "r") as zf:
            zf.extractall(COCO_DIR)
    return ann_json


def fetch_image(file_name: str) -> Path:
    local = IMAGES_DIR / file_name
    if local.is_file():
        return local
    download_file(f"{IMAGES_BASE}/{file_name}", local)
    return local


def build_from_coco(seed: int, phone_cap: int, neg_cap: int, person_cap: int) -> list[dict]:
    ann_path = ensure_coco_annotations()
    data = json.loads(ann_path.read_text(encoding="utf-8"))
    id_to_name = {img["id"]: img["file_name"] for img in data["images"]}
    id_to_area = {img["id"]: img["width"] * img["height"] for img in data["images"]}

    phone_images: set[int] = set()
    phone_area: dict[int, float] = {}
    person_images: set[int] = set()
    hard_neg_images: dict[int, set[str]] = {}

    for ann in data["annotations"]:
        img_id = ann["image_id"]
        cat = ann["category_id"]
        if cat == PHONE_CAT:
            phone_images.add(img_id)
            _, _, w, h = ann["bbox"]
            share = w * h / id_to_area[img_id]
            phone_area[img_id] = max(phone_area.get(img_id, 0.0), share)
        if cat == PERSON_CAT:
            person_images.add(img_id)
        if cat in HARD_NEG_CATS and cat != PHONE_CAT:
            hard_neg_images.setdefault(img_id, set()).add(HARD_NEG_CATS[cat])

    hard_only = [
        i
        for i, tags in hard_neg_images.items()
        if i not in phone_images and tags
    ]
    person_no_phone = [i for i in person_images if i not in phone_images]

    rng = random.Random(seed)
    phone_pool = list(phone_images)
    rng.shuffle(phone_pool)
    phone_list = phone_pool[:phone_cap]
    rng.shuffle(hard_only)
    neg_list = hard_only[:neg_cap]
    rng.shuffle(person_no_phone)
    person_list = person_no_phone[:person_cap]

    rows: list[dict] = []
    for img_id in phone_list:
        rows.append(
            {
                "image": id_to_name[img_id],
                "phone_visible": "1",
                "person_present": "1" if img_id in person_images else "0",
                "source": "coco_phone",
                "split": "eval",
                "phone_area": f"{phone_area[img_id]:.5f}",
            }
        )
    for img_id in neg_list:
        tags = ",".join(sorted(hard_neg_images.get(img_id, [])))
        rows.append(
            {
                "image": id_to_name[img_id],
                "phone_visible": "0",
                "person_present": "1" if img_id in person_images else "0",
                "source": f"coco_hard_neg:{tags}",
                "split": "eval",
            }
        )
    for img_id in person_list:
        if img_id in phone_list or img_id in neg_list:
            continue
        rows.append(
            {
                "image": id_to_name[img_id],
                "phone_visible": "0",
                "person_present": "1",
                "source": "coco_person_no_phone",
                "split": "eval",
            }
        )
    return rows


def youtube_frames(out_dir: Path, max_videos: int = 3, fps: float = 1.0) -> list[dict]:
    """Best-effort study-webcam clips; labels default person=1 phone=0 (conservative)."""
    rows: list[dict] = []
    queries = [
        "study with me webcam desk",
        "work with me laptop webcam",
        "study with me phone distraction",
    ]
    ytdlp = shutil.which("yt-dlp")
    if not ytdlp:
        print("yt-dlp not found; skipping YouTube frames", file=sys.stderr)
        return rows
    yt_dir = out_dir / "youtube"
    yt_dir.mkdir(parents=True, exist_ok=True)
    for qi, query in enumerate(queries[:max_videos]):
        search_url = f"ytsearch1:{query}"
        video_tpl = yt_dir / f"vid{qi}.%(ext)s"
        try:
            subprocess.run(
                [
                    ytdlp,
                    "-f",
                    "best[height<=480]",
                    "--max-downloads",
                    "1",
                    "-o",
                    str(video_tpl),
                    search_url,
                ],
                check=True,
                capture_output=True,
                timeout=180,
            )
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
            print(f"youtube skip {query}: {exc}", file=sys.stderr)
            continue
        videos = list(yt_dir.glob(f"vid{qi}.*"))
        if not videos:
            continue
        video = videos[0]
        frame_dir = yt_dir / f"frames_{qi}"
        frame_dir.mkdir(exist_ok=True)
        subprocess.run(
            [
                "ffmpeg",
                "-y",
                "-i",
                str(video),
                "-vf",
                f"fps={fps}",
                str(frame_dir / "f_%04d.jpg"),
            ],
            check=False,
            capture_output=True,
        )
        for frame in sorted(frame_dir.glob("f_*.jpg"))[:40]:
            dest_name = f"yt_{qi}_{frame.name}"
            shutil.copy(frame, IMAGES_DIR / dest_name)
            rows.append(
                {
                    "image": dest_name,
                    "phone_visible": "0",
                    "person_present": "1",
                    "source": f"youtube:{query}",
                    "split": "eval",
                }
            )
    return rows


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--phone-cap", type=int, default=100_000, help="default: every val image with a phone")
    parser.add_argument("--neg-cap", type=int, default=250)
    parser.add_argument("--person-cap", type=int, default=150)
    parser.add_argument("--youtube", action="store_true")
    args = parser.parse_args()

    IMAGES_DIR.mkdir(parents=True, exist_ok=True)
    rows = build_from_coco(args.seed, args.phone_cap, args.neg_cap, args.person_cap)
    for row in rows:
        fetch_image(row["image"])
    if args.youtube:
        rows.extend(youtube_frames(EVAL_DIR))
    with LABELS_CSV.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=["image", "phone_visible", "person_present", "source", "split", "phone_area"],
            restval="0",
        )
        writer.writeheader()
        writer.writerows(rows)
    print(f"wrote {len(rows)} labels -> {LABELS_CSV}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
