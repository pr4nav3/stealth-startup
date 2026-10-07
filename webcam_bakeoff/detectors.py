"""Ultralytics YOLO ONNX detector for phone (COCO cell phone) and person. CPU only."""

from __future__ import annotations

import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Tuple

import cv2
import numpy as np
import onnxruntime as ort

# COCO class indices (Ultralytics / YOLO training order).
PERSON_CLASS = 0
CELL_PHONE_CLASS = 67

# Windows ships onnxruntime-directml (any DirectX 12 GPU); macOS onnxruntime includes CoreML.
GPU_PROVIDERS = {
    "win32": ("DmlExecutionProvider", {}),
    "darwin": ("CoreMLExecutionProvider", {"ModelFormat": "MLProgram"}),
}


def create_session(onnx_path: Path, device: str, threads: int) -> ort.InferenceSession:
    """CPU, or the platform GPU provider. GPU mode fails loudly instead of running on CPU."""
    opts = ort.SessionOptions()
    opts.intra_op_num_threads = threads
    opts.inter_op_num_threads = 1
    if device == "cpu":
        return ort.InferenceSession(str(onnx_path), sess_options=opts, providers=["CPUExecutionProvider"])

    if sys.platform not in GPU_PROVIDERS:
        raise RuntimeError(f"GPU mode is not supported on {sys.platform}")
    name, options = GPU_PROVIDERS[sys.platform]
    if name not in ort.get_available_providers():
        raise RuntimeError(f"GPU mode needs {name}, which this onnxruntime build does not include")
    # DirectML cannot use memory patterns or parallel execution.
    opts.enable_mem_pattern = False
    opts.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
    session = ort.InferenceSession(
        str(onnx_path), sess_options=opts, providers=[(name, options), "CPUExecutionProvider"]
    )
    if session.get_providers()[0] != name:
        raise RuntimeError(f"GPU mode: {name} could not start on this computer (no usable GPU?)")
    return session


@dataclass(frozen=True)
class Box:
    x: float
    y: float
    w: float
    h: float
    score: float
    class_id: int


def letterbox(bgr: np.ndarray, size: int, pad: int = 114) -> Tuple[np.ndarray, float]:
    height, width = bgr.shape[:2]
    scale = min(size / height, size / width)
    new_w = int(round(width * scale))
    new_h = int(round(height * scale))
    resized = cv2.resize(bgr, (new_w, new_h), interpolation=cv2.INTER_LINEAR)
    canvas = np.full((size, size, 3), pad, dtype=np.uint8)
    canvas[:new_h, :new_w] = resized
    return canvas, scale


def phone_views(bgr: np.ndarray) -> List[Tuple[np.ndarray, int, int]]:
    """Full frame plus the lower-center square, where a phone held at the desk usually is."""
    height, width = bgr.shape[:2]
    views: List[Tuple[np.ndarray, int, int]] = [(bgr, 0, 0)]
    side = min(height, width)
    if side < 32 or side == max(height, width):
        return views
    y0 = height - side
    x0 = max(0, (width - side) // 2)
    views.append((bgr[y0:, x0 : x0 + side], x0, y0))
    return views


def nms_per_class(boxes: List[Box], iou: float = 0.45) -> List[Box]:
    kept: List[Box] = []
    for class_id in {b.class_id for b in boxes}:
        group = [b for b in boxes if b.class_id == class_id]
        keep = cv2.dnn.NMSBoxes(
            [[b.x, b.y, b.w, b.h] for b in group], [b.score for b in group], 0.0, iou
        )
        kept.extend(group[int(i)] for i in np.array(keep).reshape(-1))
    return kept


class UltralyticsYoloDetector:
    """YOLO11 / YOLO26 ONNX export with raw (1, 84, anchors) output; NMS done here."""

    def __init__(
        self,
        onnx_path: Path,
        phone_score: float,
        person_score: float,
        input_size: int = 640,
        threads: int = 4,
        dual_view: bool = True,
        device: str = "cpu",
    ):
        self._session = create_session(onnx_path, device, threads)
        self.provider = self._session.get_providers()[0]
        self._thresholds: Dict[int, float] = {
            CELL_PHONE_CLASS: phone_score,
            PERSON_CLASS: person_score,
        }
        self._size = input_size
        self._dual_view = dual_view
        self._input_name = self._session.get_inputs()[0].name

    def _decode(self, raw: np.ndarray, scale: float) -> List[Box]:
        # (1, 4 + classes, anchors) -> (anchors, 4 + classes). Each class is scored on
        # its own column so a phone next to a face is not hidden by a stronger "person".
        preds = raw[0].T
        out: List[Box] = []
        for class_id, thresh in self._thresholds.items():
            scores = preds[:, 4 + class_id]
            keep = scores >= thresh
            for (cx, cy, w, h), score in zip(preds[keep, :4], scores[keep]):
                out.append(
                    Box(
                        float((cx - w / 2.0) / scale),
                        float((cy - h / 2.0) / scale),
                        float(w / scale),
                        float(h / scale),
                        float(score),
                        class_id,
                    )
                )
        return out

    def _run_view(self, bgr: np.ndarray) -> List[Box]:
        padded, scale = letterbox(bgr, self._size)
        rgb = padded[:, :, ::-1].astype(np.float32) / 255.0
        blob = np.ascontiguousarray(np.transpose(rgb, (2, 0, 1))[None])
        raw = self._session.run(None, {self._input_name: blob})[0]
        return self._decode(raw, scale)

    def detect(self, bgr: np.ndarray) -> List[Box]:
        height, width = bgr.shape[:2]
        views = phone_views(bgr) if self._dual_view else [(bgr, 0, 0)]
        merged: List[Box] = []
        for crop, ox, oy in views:
            for box in self._run_view(crop):
                x1 = min(max(box.x + ox, 0.0), float(width - 1))
                y1 = min(max(box.y + oy, 0.0), float(height - 1))
                x2 = min(box.x + box.w + ox, float(width))
                y2 = min(box.y + box.h + oy, float(height))
                if x2 - x1 < 8 or y2 - y1 < 8:
                    continue
                merged.append(Box(x1, y1, x2 - x1, y2 - y1, box.score, box.class_id))
        return nms_per_class(merged)


def load_detector(
    name: str,
    models_dir: Path,
    phone_score: float,
    person_score: float,
    threads: int = 4,
    dual_view: bool = True,
    device: str = "cpu",
) -> UltralyticsYoloDetector:
    if "_416" in name:
        input_size = 416
    elif "_512" in name:
        input_size = 512
    else:
        input_size = 640
    return UltralyticsYoloDetector(
        models_dir / name,
        phone_score=phone_score,
        person_score=person_score,
        input_size=input_size,
        threads=threads,
        dual_view=dual_view,
        device=device,
    )
