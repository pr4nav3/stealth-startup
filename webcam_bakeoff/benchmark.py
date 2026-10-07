#!/usr/bin/env python3
"""Score phone/presence detectors on eval/labels.csv. One model at a time."""

from __future__ import annotations

import argparse
import csv
import json
import sys
import time
from pathlib import Path

import cv2
import numpy as np

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from detectors import CELL_PHONE_CLASS, PERSON_CLASS, load_detector

EVAL_DIR = SCRIPT_DIR / "eval"
LABELS = EVAL_DIR / "labels.csv"
IMAGES = EVAL_DIR / "images"
# A phone held at a laptop covers well over 1% of a webcam frame.
WEBCAM_LIKE_PHONE_AREA = 0.01


def peak_rss_mb() -> float:
    try:
        import psutil

        return psutil.Process().memory_info().rss / (1024 * 1024)
    except ImportError:
        return 0.0


def metrics(y_true: list[bool], y_pred: list[bool]) -> dict:
    tp = sum(1 for t, p in zip(y_true, y_pred) if t and p)
    fp = sum(1 for t, p in zip(y_true, y_pred) if not t and p)
    fn = sum(1 for t, p in zip(y_true, y_pred) if t and not p)
    tn = sum(1 for t, p in zip(y_true, y_pred) if not t and not p)
    prec = tp / (tp + fp) if (tp + fp) else 0.0
    rec = tp / (tp + fn) if (tp + fn) else 0.0
    f1 = 2 * prec * rec / (prec + rec) if (prec + rec) else 0.0
    fpr = fp / (fp + tn) if (fp + tn) else 0.0
    return {
        "tp": tp,
        "fp": fp,
        "fn": fn,
        "tn": tn,
        "precision": round(prec, 4),
        "recall": round(rec, 4),
        "f1": round(f1, 4),
        "fpr_hard_neg": round(fpr, 4),
    }


def yunet_face(score: float = 0.55):
    from download_models import ensure_models

    paths = ensure_models()
    det = cv2.FaceDetectorYN.create(
        str(paths["face_detection_yunet_2023mar.onnx"]),
        "",
        (320, 320),
        score,
        0.3,
        5000,
    )

    def has_face(bgr: np.ndarray) -> bool:
        h, w = bgr.shape[:2]
        det.setInputSize((w, h))
        _, faces = det.detect(bgr)
        return faces is not None and len(faces) > 0

    return has_face


def load_rows() -> list[dict]:
    if not LABELS.is_file():
        raise SystemExit(f"missing {LABELS}; run eval/build_eval_set.py")
    with LABELS.open(encoding="utf-8") as handle:
        return list(csv.DictReader(handle))


def is_webcam_like(row: dict) -> bool:
    return row["person_present"] == "1" and float(row.get("phone_area") or 0) >= WEBCAM_LIKE_PHONE_AREA


def run(args: argparse.Namespace) -> list[dict]:
    """One detector pass at the lowest threshold, then every threshold scored from it."""
    rows = load_rows()
    detector = load_detector(
        args.model,
        args.models,
        phone_score=min(args.phone_score),
        person_score=args.person_score,
        threads=args.threads,
        dual_view=not args.single_view,
        device=args.device,
    )
    face_fn = yunet_face()

    scored: list[tuple[dict, float]] = []
    person_true: list[bool] = []
    person_pred: list[bool] = []
    latencies: list[float] = []

    for row in rows:
        bgr = cv2.imread(str(IMAGES / row["image"]))
        if bgr is None:
            continue
        t0 = time.perf_counter()
        boxes = detector.detect(bgr)
        face = face_fn(bgr)
        latencies.append(time.perf_counter() - t0)

        top_phone = max((b.score for b in boxes if b.class_id == CELL_PHONE_CLASS), default=0.0)
        scored.append((row, top_phone))
        person_true.append(row["person_present"] == "1")
        person_pred.append(face or any(b.class_id == PERSON_CLASS for b in boxes))

    mean_ms = 1000.0 * float(np.mean(latencies)) if latencies else 0.0
    shared = {
        "model": args.model,
        "provider": detector.provider,
        "person_score": args.person_score,
        "dual_view": not args.single_view,
        "n": len(latencies),
        "presence_face_or_person": metrics(person_true, person_pred),
        "mean_ms_per_frame": round(mean_ms, 2),
        "fps_est": round(1000.0 / mean_ms, 2) if mean_ms else 0.0,
        "peak_rss_mb": round(peak_rss_mb(), 1),
    }
    results = []
    for thresh in sorted(args.phone_score):
        truth = [row["phone_visible"] == "1" for row, _ in scored]
        pred = [score >= thresh for _, score in scored]
        hard = [p for (row, _), p in zip(scored, pred) if "hard_neg" in row["source"]]
        webcam = [p for (row, _), p in zip(scored, pred) if row["phone_visible"] == "1" and is_webcam_like(row)]
        results.append(
            {
                **shared,
                "phone_score": thresh,
                "phone": metrics(truth, pred),
                "webcam_like_phone_recall": round(sum(webcam) / max(1, len(webcam)), 4),
                "webcam_like_n": len(webcam),
                "hard_neg_phone_fp_rate": round(sum(hard) / max(1, len(hard)), 4),
            }
        )
    return results


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, help="onnx file name in models/")
    parser.add_argument("--models", type=Path, default=SCRIPT_DIR / "models")
    parser.add_argument("--phone-score", type=float, nargs="+", default=[0.25])
    parser.add_argument("--person-score", type=float, default=0.35)
    parser.add_argument("--single-view", action="store_true", help="skip the lower-center crop pass")
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--device", choices=["cpu", "gpu"], default="cpu")
    parser.add_argument("--out", type=Path, default=SCRIPT_DIR / "eval" / "results.jsonl")
    args = parser.parse_args()

    results = run(args)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("a", encoding="utf-8") as handle:
        for result in results:
            print(json.dumps(result), flush=True)
            handle.write(json.dumps(result) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
