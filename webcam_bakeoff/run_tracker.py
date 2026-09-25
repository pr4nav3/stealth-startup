#!/usr/bin/env python3
"""Live webcam bakeoff: face in/out, at-PC / away / at-phone, phone in frame.

8 fps. Frames stay in memory. JSON lines to stdout.
"""

from __future__ import annotations

import argparse
import base64
import json
import sys
import time
from pathlib import Path
from typing import Dict, List, Optional, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import cv2
import numpy as np
import onnxruntime as ort

from download_models import ensure_models
from labels import classify, face_yaw_pitch, looking_at_pc

CELL_PHONE_CLASS = 67
YOLOX_SIZE = 640
YOLOX_STRIDES = (8, 16, 32)
YUNET_MAX_SIDE = 960


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--camera", type=int, default=0)
    parser.add_argument("--fps", type=float, default=8.0)
    parser.add_argument("--overlay", action="store_true")
    parser.add_argument("--preview", action="store_true", help="JSON lines with jpeg for the app")
    parser.add_argument("--face-score", type=float, default=0.55)
    parser.add_argument("--phone-score", type=float, default=0.15)
    parser.add_argument("--models", type=Path, default=SCRIPT_DIR / "models")
    return parser.parse_args()


def best_face(faces) -> Optional[np.ndarray]:
    if faces is None or len(faces) == 0:
        return None
    return max(faces, key=lambda row: float(row[14]))


def face_points(face: np.ndarray):
    right_eye = (float(face[4]), float(face[5]))
    left_eye = (float(face[6]), float(face[7]))
    nose = (float(face[8]), float(face[9]))
    right_mouth = (float(face[10]), float(face[11]))
    left_mouth = (float(face[12]), float(face[13]))
    return right_eye, left_eye, nose, right_mouth, left_mouth


def letterbox(bgr: np.ndarray, size: int = YOLOX_SIZE, pad: int = 114):
    height, width = bgr.shape[:2]
    scale = min(size / height, size / width)
    new_w = int(round(width * scale))
    new_h = int(round(height * scale))
    resized = cv2.resize(bgr, (new_w, new_h), interpolation=cv2.INTER_LINEAR)
    canvas = np.full((size, size, 3), pad, dtype=np.uint8)
    canvas[:new_h, :new_w] = resized
    return canvas, scale


def yolox_grids(size: int = YOLOX_SIZE) -> Tuple[np.ndarray, np.ndarray]:
    grids = []
    strides = []
    for stride in YOLOX_STRIDES:
        cells = size // stride
        xv, yv = np.meshgrid(np.arange(cells), np.arange(cells))
        grid = np.stack((xv, yv), 2).reshape(1, -1, 2)
        grids.append(grid)
        strides.append(np.full((1, grid.shape[1], 1), stride))
    return np.concatenate(grids, 1), np.concatenate(strides, 1)


_YOLOX_GRIDS, _YOLOX_STRIDE_MAP = yolox_grids()


def decode_yolox(raw: np.ndarray, scale: float, score_thresh: float) -> List[List[float]]:
    # Megvii ONNXRuntime decode: https://github.com/Megvii-BaseDetection/YOLOX/tree/main/demo/ONNXRuntime
    preds = raw[0].copy()
    preds[:, :2] = (preds[:, :2] + _YOLOX_GRIDS[0]) * _YOLOX_STRIDE_MAP[0]
    preds[:, 2:4] = np.exp(preds[:, 2:4]) * _YOLOX_STRIDE_MAP[0]

    obj = preds[:, 4]
    cls = preds[:, 5:]
    class_ids = cls.argmax(axis=1)
    scores = obj * cls.max(axis=1)
    keep = (class_ids == CELL_PHONE_CLASS) & (scores >= score_thresh)
    if not np.any(keep):
        return []

    boxes = preds[keep, :4]
    scores = scores[keep]
    x1 = boxes[:, 0] - boxes[:, 2] / 2.0
    y1 = boxes[:, 1] - boxes[:, 3] / 2.0
    xywh = np.stack([x1, y1, boxes[:, 2], boxes[:, 3]], axis=1)
    nms = cv2.dnn.NMSBoxes(xywh.tolist(), scores.tolist(), score_thresh, 0.45)
    if nms is None or len(nms) == 0:
        return []

    phones = []
    for idx in np.array(nms).reshape(-1):
        x, y, w, h = xywh[idx]
        phones.append(
            [
                float(x / scale),
                float(y / scale),
                float(w / scale),
                float(h / scale),
                float(scores[idx]),
            ]
        )
    return phones


def detect_phones_on(session: ort.InferenceSession, bgr: np.ndarray, score_thresh: float):
    padded, scale = letterbox(bgr)
    rgb = padded[:, :, ::-1].astype(np.float32)
    blob = np.transpose(rgb, (2, 0, 1))[None]
    input_name = session.get_inputs()[0].name
    raw = session.run(None, {input_name: blob})[0]
    if raw.ndim == 2:
        raw = raw[None]
    return decode_yolox(raw, scale, score_thresh)


def phone_views(bgr: np.ndarray) -> List[Tuple[np.ndarray, int, int]]:
    """Full frame plus a lower-center square.

    Landscape webcams letterbox by width, so a full-width bottom strip does
    not zoom. A square on the lower half does, which is where phones live.
    Two views stay inside an 8 fps budget; three did not.
    """
    height, width = bgr.shape[:2]
    views = [(bgr, 0, 0)]
    side = min(height, width)
    if side < 32:
        return views
    y0 = height - side
    x0 = max(0, (width - side) // 2)
    views.append((bgr[y0:, x0 : x0 + side], x0, y0))
    return views


def nms_phones(phones: List[List[float]], score_thresh: float) -> List[List[float]]:
    if not phones:
        return []
    xywh = [[p[0], p[1], p[2], p[3]] for p in phones]
    scores = [p[4] for p in phones]
    keep = cv2.dnn.NMSBoxes(xywh, scores, score_thresh, 0.45)
    if keep is None or len(keep) == 0:
        return []
    return [phones[int(idx)] for idx in np.array(keep).reshape(-1)]


def detect_phones(session: ort.InferenceSession, bgr: np.ndarray, score_thresh: float):
    height, width = bgr.shape[:2]
    merged: List[List[float]] = []
    for crop, origin_x, origin_y in phone_views(bgr):
        if crop.size == 0:
            continue
        for x, y, w, h, score in detect_phones_on(session, crop, score_thresh):
            x1 = x + origin_x
            y1 = y + origin_y
            x2 = x1 + w
            y2 = y1 + h
            x1 = min(max(x1, 0.0), float(width - 1))
            y1 = min(max(y1, 0.0), float(height - 1))
            x2 = min(max(x2, 0.0), float(width))
            y2 = min(max(y2, 0.0), float(height))
            bw = x2 - x1
            bh = y2 - y1
            if bw < 8 or bh < 8:
                continue
            merged.append([x1, y1, bw, bh, score])
    return nms_phones(merged, score_thresh)


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


def draw_overlay(frame, face, phones, labels: dict, debug: Optional[Dict] = None) -> None:
    if face is not None:
        x, y, w, h = [int(v) for v in face[:4]]
        cv2.rectangle(frame, (x, y), (x + w, y + h), (0, 180, 0), 2)
        for i in range(4, 14, 2):
            cv2.circle(frame, (int(face[i]), int(face[i + 1])), 2, (0, 180, 0), -1)
    for x, y, w, h, score in phones:
        cv2.rectangle(
            frame,
            (int(x), int(y)),
            (int(x + w), int(y + h)),
            (0, 0, 220),
            2,
        )
        cv2.putText(
            frame,
            f"phone {score:.2f}",
            (int(x), max(16, int(y) - 6)),
            cv2.FONT_HERSHEY_SIMPLEX,
            0.5,
            (0, 0, 220),
            1,
            cv2.LINE_AA,
        )
    text = f"face={labels['face_in_frame']} gaze={labels['gaze']} phone={labels['phone_in_frame']}"
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
    if debug:
        pose = (
            f"yaw={debug.get('yaw', 0):.2f} pitch={debug.get('pitch', 0):.2f}"
        )
        cv2.putText(
            frame,
            pose,
            (12, 48),
            cv2.FONT_HERSHEY_SIMPLEX,
            0.55,
            (240, 240, 240),
            2,
            cv2.LINE_AA,
        )


def preview_row(
    labels: dict,
    phone_count: int,
    jpeg: str = "",
    error: Optional[str] = None,
) -> dict:
    row = {
        "face_in_frame": bool(labels.get("face_in_frame", False)),
        "gaze": labels.get("gaze"),
        "phone_in_frame": bool(labels.get("phone_in_frame", False)),
        "looking_at_phone": bool(labels.get("looking_at_phone", False)),
        "phone_count": int(phone_count),
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
    detector = cv2.FaceDetectorYN.create(
        str(paths["face_detection_yunet_2023mar.onnx"]),
        "",
        (320, 320),
        args.face_score,
        0.3,
        5000,
    )
    phone_session = ort.InferenceSession(
        str(paths["yolox_s.onnx"]),
        providers=["CPUExecutionProvider"],
    )

    cap = cv2.VideoCapture(args.camera)
    if not cap.isOpened():
        message = f"could not open camera {args.camera}"
        if args.preview:
            emit_preview(preview_row({}, 0, error=message))
        print(message, file=sys.stderr)
        return 1

    interval = 1.0 / args.fps
    last_size = None
    try:
        while True:
            started = time.monotonic()
            ok, frame = cap.read()
            if not ok:
                message = "camera frame failed"
                if args.preview:
                    emit_preview(preview_row({}, 0, error=message))
                print(message, file=sys.stderr)
                return 1

            height, width = frame.shape[:2]
            face, last_size = detect_face(detector, frame, last_size)
            phones = detect_phones(phone_session, frame, args.phone_score)

            debug = {"yaw": 0.0, "pitch": 0.0}
            at_pc = False
            if face is not None:
                points = face_points(face)
                pose = face_yaw_pitch(*points)
                if pose is not None:
                    debug["yaw"] = pose[0]
                    debug["pitch"] = pose[1]
                at_pc = looking_at_pc(*points)

            labels = classify(face is not None, at_pc, bool(phones))
            if args.preview:
                preview = frame.copy()
                draw_overlay(preview, face, phones, labels, debug)
                emit_preview(preview_row(labels, len(phones), jpeg=encode_preview(preview)))
            else:
                print(
                    json.dumps(
                        {
                            "ts": time.time(),
                            **labels,
                            "phone_count": len(phones),
                            **debug,
                        }
                    ),
                    flush=True,
                )

            if args.overlay and not args.preview:
                draw_overlay(frame, face, phones, labels, debug)
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
