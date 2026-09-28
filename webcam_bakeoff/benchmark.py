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

from detectors import load_detector

EVAL_DIR = SCRIPT_DIR / "eval"
LABELS = EVAL_DIR / "labels.csv"
IMAGES = EVAL_DIR / "images"


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


def run(args: argparse.Namespace) -> dict:
    rows = load_rows()
    detector = load_detector(
        args.model,
        args.models,
        score_thresh=args.phone_score,
        threads=args.threads,
    )
    face_fn = yunet_face()

    phone_true: list[bool] = []
    phone_pred: list[bool] = []
    person_true: list[bool] = []
    person_pred: list[bool] = []
    hard_neg_pred: list[bool] = []
    latencies: list[float] = []

    for row in rows:
        img_path = IMAGES / row["image"]
        bgr = cv2.imread(str(img_path))
        if bgr is None:
            continue
        t0 = time.perf_counter()
        if hasattr(detector, "_detect_all"):
            all_boxes = detector._detect_all(bgr)
            phones = [b for b in all_boxes if b.class_id == 67]
            persons = [b for b in all_boxes if b.class_id == 0]
        else:
            phones = detector.detect_phones(bgr)
            persons = detector.detect_persons(bgr)
        face = face_fn(bgr)
        latencies.append(time.perf_counter() - t0)

        gt_phone = row["phone_visible"] == "1"
        gt_person = row["person_present"] == "1"
        pred_phone = len(phones) > 0
        pred_person = face or len(persons) > 0

        phone_true.append(gt_phone)
        phone_pred.append(pred_phone)
        person_true.append(gt_person)
        person_pred.append(pred_person)
        if not gt_phone and "hard_neg" in row.get("source", ""):
            hard_neg_pred.append(pred_phone)

    phone_m = metrics(phone_true, phone_pred)
    person_m = metrics(person_true, person_pred)
    hard_fp = sum(hard_neg_pred) / len(hard_neg_pred) if hard_neg_pred else 0.0
    mean_ms = 1000.0 * float(np.mean(latencies)) if latencies else 0.0
    fps = 1000.0 / mean_ms if mean_ms > 0 else 0.0

    return {
        "model": args.model,
        "phone_score": args.phone_score,
        "n": len(latencies),
        "phone": phone_m,
        "presence_face_or_person": person_m,
        "hard_neg_phone_fp_rate": round(hard_fp, 4),
        "mean_ms_per_frame": round(mean_ms, 2),
        "fps_est": round(fps, 2),
        "peak_rss_mb": round(peak_rss_mb(), 1),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, help="onnx file name in models/")
    parser.add_argument("--models", type=Path, default=SCRIPT_DIR / "models")
    parser.add_argument("--phone-score", type=float, default=0.25)
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--out", type=Path, default=SCRIPT_DIR / "eval" / "results.jsonl")
    args = parser.parse_args()

    try:
        result = run(args)
    except Exception as exc:
        print(json.dumps({"model": args.model, "error": str(exc)}), flush=True)
        return 1
    print(json.dumps(result, indent=2))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(result) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
