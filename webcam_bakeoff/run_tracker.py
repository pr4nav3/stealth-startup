#!/usr/bin/env python3
"""Live webcam worker: phone in frame + user present. Gaze paused.

Frames stay in memory. JSON lines to stdout. Shipped frozen (PyInstaller) inside the app.
"""

from __future__ import annotations

import argparse
import base64
import json
import sys
import time
from pathlib import Path
from typing import List, Optional

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import cv2
import numpy as np

from detectors import CELL_PHONE_CLASS, PERSON_CLASS, Box, load_detector
from download_models import DETECTOR, FACE_MODEL, MODELS_DIR, verify_models
from labels import TrackerState, compose_frame

YUNET_MAX_SIDE = 960
# Chosen in RESULTS.md: YOLO26s, full frame only, 0.15 (static look-alikes like remotes stay
# under it; hysteresis cannot filter an object that scores the same on every frame).
DEFAULT_PHONE_SCORE = 0.15
DEFAULT_PERSON_SCORE = 0.35

CAMERA_HELP = {
    "win32": "Check that no other app is using it, and that Settings > Privacy & security > Camera "
    "allows desktop apps to use the camera.",
    "darwin": "Check that no other app is using it, and that System Settings > Privacy & Security > "
    "Camera allows Supervisor.",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--camera", type=int, default=0)
    parser.add_argument("--fps", type=float, default=2.0, help="watch rate while no phone is suspected")
    parser.add_argument(
        "--burst-fps", type=float, default=5.0, help="rate from the first frame that looks like a phone until it clears"
    )
    parser.add_argument("--preview", action="store_true", help="include a jpeg in every line for the app")
    parser.add_argument("--device", choices=["cpu", "gpu"], default="cpu")
    parser.add_argument("--self-test", action="store_true", help="load models, run one blank frame, exit")
    parser.add_argument("--face-score", type=float, default=0.55)
    parser.add_argument("--phone-score", type=float, default=DEFAULT_PHONE_SCORE)
    parser.add_argument("--person-score", type=float, default=DEFAULT_PERSON_SCORE)
    # Two threads: a frame takes ~40% longer than with four, but the worker never ties up
    # every core of a slower laptop for a whole session.
    parser.add_argument("--threads", type=int, default=2)
    parser.add_argument("--models", type=Path, default=MODELS_DIR)
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


def draw_boxes(frame, face, phones: List[Box], persons: List[Box]) -> None:
    for box, color in [(b, (200, 140, 0)) for b in persons] + [(b, (0, 0, 220)) for b in phones]:
        cv2.rectangle(
            frame,
            (int(box.x), int(box.y)),
            (int(box.x + box.w), int(box.y + box.h)),
            color,
            2,
        )
    if face is not None:
        x, y, w, h = [int(v) for v in face[:4]]
        cv2.rectangle(frame, (x, y), (x + w, y + h), (0, 180, 0), 2)


def encode_preview(frame) -> str:
    height, width = frame.shape[:2]
    if width > 640:
        frame = cv2.resize(frame, (640, max(1, int(height * 640 / width))), interpolation=cv2.INTER_AREA)
    ok, buffer = cv2.imencode(".jpg", frame, [int(cv2.IMWRITE_JPEG_QUALITY), 55])
    return base64.b64encode(buffer).decode("ascii") if ok else ""


def emit(row: dict) -> None:
    print(json.dumps(row), flush=True)


def fail(device: str, message: str) -> int:
    emit({"device": device, "error": message})
    print(message, file=sys.stderr)
    return 1


def main() -> int:
    args = parse_args()
    if args.fps <= 0 or args.burst_fps <= 0:
        return fail(args.device, "fps must be positive")

    try:
        paths = verify_models(args.models)
        face_detector = cv2.FaceDetectorYN.create(
            str(paths[FACE_MODEL]), "", (320, 320), args.face_score, 0.3, 5000
        )
        object_detector = load_detector(
            DETECTOR,
            args.models,
            phone_score=args.phone_score,
            person_score=args.person_score,
            threads=args.threads,
            dual_view=False,
            device=args.device,
        )
    except Exception as exc:
        return fail(args.device, str(exc))
    provider = object_detector.provider

    if args.self_test:
        blank = np.zeros((720, 1280, 3), dtype=np.uint8)
        started = time.perf_counter()
        object_detector.detect(blank)
        detect_face(face_detector, blank, None)
        emit(
            {
                "self_test": "ok",
                "device": args.device,
                "provider": provider,
                "ms": round(1000 * (time.perf_counter() - started), 1),
            }
        )
        return 0

    cap = cv2.VideoCapture(args.camera)
    if not cap.isOpened():
        return fail(args.device, f"Could not open camera {args.camera}. {CAMERA_HELP.get(sys.platform, '')}".strip())
    # Read the newest frame, not one queued while the last frame was being checked.
    cap.set(cv2.CAP_PROP_BUFFERSIZE, 1)

    last_size = None
    tracker_state = TrackerState.default()
    try:
        while True:
            started = time.monotonic()
            ok, frame = cap.read()
            if not ok:
                return fail(args.device, "The camera stopped sending frames.")

            detections = object_detector.detect(frame)
            phones = [b for b in detections if b.class_id == CELL_PHONE_CLASS]
            persons = [b for b in detections if b.class_id == PERSON_CLASS]
            # Present means a face or a person. A person already answers it, so the face pass
            # only runs when nobody was found, or for the Settings preview that draws it.
            face = None
            if args.preview or not persons:
                face, last_size = detect_face(face_detector, frame, last_size)
            labels = compose_frame(
                tracker_state,
                face_in_frame=face is not None,
                person_in_frame=bool(persons),
                phone_raw=bool(phones),
                phone_count=len(phones),
                phone_confidence=max((p.score for p in phones), default=0.0),
            )
            row = {"ts": time.time(), "device": args.device, "provider": provider, **labels}
            if args.preview:
                draw_boxes(frame, face, phones, persons)
                row["jpeg"] = encode_preview(frame)
            emit(row)

            # Watch slowly; check fast while any recent frame looked like a phone, so it is
            # confirmed or cleared in about a second without paying the fast rate all session.
            fps = args.burst_fps if tracker_state.phone.recent_hit() else args.fps
            leftover = 1.0 / fps - (time.monotonic() - started)
            if leftover > 0:
                time.sleep(leftover)
    except KeyboardInterrupt:
        pass
    finally:
        cap.release()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
