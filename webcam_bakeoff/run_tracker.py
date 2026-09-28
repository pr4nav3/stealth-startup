#!/usr/bin/env python3
"""Live webcam worker: phone in frame + user present. Gaze paused.

8 fps target. Frames stay in memory. JSON lines to stdout.
"""

from __future__ import annotations

import argparse
import base64
import json
import sys
import time
from pathlib import Path
from typing import List, Optional, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import cv2
import numpy as np

from detectors import CELL_PHONE_CLASS, PERSON_CLASS, Box, UltralyticsYoloDetector, load_detector
from download_models import ensure_models
from labels import TrackerState, compose_frame

YUNET_MAX_SIDE = 960
DEFAULT_DETECTOR = "yolo11n.onnx"
DEFAULT_PHONE_SCORE = 0.06


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--camera", type=int, default=0)
    parser.add_argument("--fps", type=float, default=8.0)
    parser.add_argument("--overlay", action="store_true")
    parser.add_argument("--preview", action="store_true", help="JSON lines with jpeg for the app")
    parser.add_argument("--face-score", type=float, default=0.55)
    parser.add_argument("--phone-score", type=float, default=DEFAULT_PHONE_SCORE)
    parser.add_argument("--detector", default=DEFAULT_DETECTOR, help="onnx file name in models/")
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--models", type=Path, default=SCRIPT_DIR / "models")
    return parser.parse_args()


def best_face(faces) -> Optional[np.ndarray]:
    if faces is None or len(faces) == 0:
        return None
    return max(faces, key=lambda row: float(row[14]))


def detect_face(detector, frame: np.ndarray, last_size):
    height, width = frame.shape[:2]
    scale = 1.0
    work = frame
    longest = max(height, width)
    if longest > YUNET_MAX_SIDE:
        scale = YUNET_MAX_SIDE / longest
        work = cv2.resize(
            frame,
            (max(1, int(width * scale)), max(1, int(height * scale))),
            interpolation=cv2.INTER_AREA,
        )
    size = (work.shape[1], work.shape[0])
    if size != last_size:
        detector.setInputSize(size)
        last_size = size
    _, faces = detector.detect(work)
    face = best_face(faces)
    if face is not None and scale != 1.0:
        face = face.copy()
        face[:14] /= scale
    return face, last_size


def phone_confidence(phones: List[Box]) -> float:
    if not phones:
        return 0.0
    return max(p.score for p in phones)


def draw_overlay(frame, face, phones: List[Box], labels: dict) -> None:
    if face is not None:
        x, y, w, h = [int(v) for v in face[:4]]
        cv2.rectangle(frame, (x, y), (x + w, y + h), (0, 180, 0), 2)
    for box in phones:
        cv2.rectangle(
            frame,
            (int(box.x), int(box.y)),
            (int(box.x + box.w), int(box.y + box.h)),
            (0, 0, 220),
            2,
        )
    text = (
        f"present={labels.get('present')} "
        f"face={labels.get('face_in_frame')} "
        f"phone={labels.get('phone_in_frame')}"
    )
    cv2.putText(
        frame,
        text,
        (12, 24),
        cv2.FONT_HERSHEY_SIMPLEX,
        0.6,
        (240, 240, 240),
        2,
        cv2.LINE_AA,
    )


def preview_row(labels: dict, jpeg: str = "", error: Optional[str] = None) -> dict:
    row = {
        "face_in_frame": bool(labels.get("face_in_frame", False)),
        "person_in_frame": bool(labels.get("person_in_frame", False)),
        "present": bool(labels.get("present", False)),
        "gaze": None,
        "phone_in_frame": bool(labels.get("phone_in_frame", False)),
        "looking_at_phone": bool(labels.get("looking_at_phone", False)),
        "phone_count": int(labels.get("phone_count", 0)),
        "phone_confidence": float(labels.get("phone_confidence", 0.0)),
        "jpeg": jpeg,
    }
    if error:
        row["error"] = error
    return row


def emit_preview(row: dict) -> None:
    print(json.dumps(row), flush=True)


def encode_preview(frame) -> str:
    height, width = frame.shape[:2]
    if width > 640:
        scale = 640 / width
        frame = cv2.resize(
            frame,
            (640, max(1, int(height * scale))),
            interpolation=cv2.INTER_AREA,
        )
    ok, buffer = cv2.imencode(".jpg", frame, [int(cv2.IMWRITE_JPEG_QUALITY), 55])
    if not ok:
        return ""
    return base64.b64encode(buffer).decode("ascii")


def main() -> int:
    args = parse_args()
    if args.fps <= 0:
        print("fps must be positive", file=sys.stderr)
        return 1

    paths = ensure_models(args.models)
    face_detector = cv2.FaceDetectorYN.create(
        str(paths["face_detection_yunet_2023mar.onnx"]),
        "",
        (320, 320),
        args.face_score,
        0.3,
        5000,
    )
    object_detector = load_detector(
        args.detector,
        args.models,
        score_thresh=args.phone_score,
        threads=args.threads,
    )
    if not isinstance(object_detector, UltralyticsYoloDetector):
        print("only Ultralytics YOLO detectors are supported in the live worker", file=sys.stderr)
        return 1

    cap = cv2.VideoCapture(args.camera)
    if not cap.isOpened():
        message = f"could not open camera {args.camera}"
        if args.preview:
            emit_preview(preview_row({}, error=message))
        print(message, file=sys.stderr)
        return 1

    interval = 1.0 / args.fps
    last_size = None
    tracker_state = TrackerState.default()
    try:
        while True:
            started = time.monotonic()
            ok, frame = cap.read()
            if not ok:
                message = "camera frame failed"
                if args.preview:
                    emit_preview(preview_row({}, error=message))
                print(message, file=sys.stderr)
                return 1

            face, last_size = detect_face(face_detector, frame, last_size)
            detections = object_detector._detect_all(frame)
            phones = [b for b in detections if b.class_id == CELL_PHONE_CLASS]
            persons = [b for b in detections if b.class_id == PERSON_CLASS]

            labels = compose_frame(
                tracker_state,
                face_in_frame=face is not None,
                person_in_frame=len(persons) > 0,
                phone_raw=len(phones) > 0,
                phone_count=len(phones),
                phone_confidence=phone_confidence(phones),
            )

            if args.preview:
                preview = frame.copy()
                draw_overlay(preview, face, phones, labels)
                emit_preview(preview_row(labels, jpeg=encode_preview(preview)))
            else:
                print(json.dumps({"ts": time.time(), **labels}), flush=True)

            if args.overlay and not args.preview:
                draw_overlay(frame, face, phones, labels)
                cv2.imshow("supervisor webcam bakeoff", frame)
                if cv2.waitKey(1) & 0xFF == ord("q"):
                    break

            leftover = interval - (time.monotonic() - started)
            if leftover > 0:
                time.sleep(leftover)
    except KeyboardInterrupt:
        pass
    finally:
        cap.release()
        if args.overlay and not args.preview:
            cv2.destroyAllWindows()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
