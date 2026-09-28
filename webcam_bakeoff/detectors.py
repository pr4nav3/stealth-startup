"""ONNX detectors for phone (COCO cell phone) and person. CPU only."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional, Sequence, Tuple

import cv2
import numpy as np
import onnxruntime as ort

# COCO class indices (Ultralytics / YOLO training order).
PERSON_CLASS = 0
CELL_PHONE_CLASS = 67

YOLOX_SIZE = 640
YOLOX_STRIDES = (8, 16, 32)


@dataclass(frozen=True)
class Box:
    x: float
    y: float
    w: float
    h: float
    score: float
    class_id: int = CELL_PHONE_CLASS


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
    height, width = bgr.shape[:2]
    views: List[Tuple[np.ndarray, int, int]] = [(bgr, 0, 0)]
    side = min(height, width)
    if side < 32:
        return views
    y0 = height - side
    x0 = max(0, (width - side) // 2)
    views.append((bgr[y0:, x0 : x0 + side], x0, y0))
    return views


def nms_boxes(
    boxes: List[List[float]],
    scores: List[float],
    score_thresh: float,
    iou: float = 0.45,
) -> List[int]:
    if not boxes:
        return []
    keep = cv2.dnn.NMSBoxes(boxes, scores, score_thresh, iou)
    if keep is None or len(keep) == 0:
        return []
    return [int(idx) for idx in np.array(keep).reshape(-1)]


class YoloXPhoneDetector:
    """Megvii YOLOX-s ONNX (legacy baseline)."""

    def __init__(self, onnx_path: Path, score_thresh: float = 0.15, threads: int = 4):
        opts = ort.SessionOptions()
        opts.intra_op_num_threads = threads
        opts.inter_op_num_threads = 1
        self._session = ort.InferenceSession(
            str(onnx_path),
            sess_options=opts,
            providers=["CPUExecutionProvider"],
        )
        self._score_thresh = score_thresh
        self._input_name = self._session.get_inputs()[0].name
        self._grids, self._strides = self._yolox_grids(YOLOX_SIZE)

    @staticmethod
    def _yolox_grids(size: int):
        grids = []
        strides = []
        for stride in YOLOX_STRIDES:
            cells = size // stride
            xv, yv = np.meshgrid(np.arange(cells), np.arange(cells))
            grid = np.stack((xv, yv), 2).reshape(1, -1, 2)
            grids.append(grid)
            strides.append(np.full((1, grid.shape[1], 1), stride))
        return np.concatenate(grids, 1), np.concatenate(strides, 1)

    def _decode_yolox(self, raw: np.ndarray, scale: float) -> List[Box]:
        preds = raw[0].copy()
        preds[:, :2] = (preds[:, :2] + self._grids[0]) * self._strides[0]
        preds[:, 2:4] = np.exp(preds[:, 2:4]) * self._strides[0]
        obj = preds[:, 4]
        cls = preds[:, 5:]
        class_ids = cls.argmax(axis=1)
        scores = obj * cls.max(axis=1)
        keep = (class_ids == CELL_PHONE_CLASS) & (scores >= self._score_thresh)
        if not np.any(keep):
            return []
        boxes = preds[keep, :4]
        scores = scores[keep]
        x1 = boxes[:, 0] - boxes[:, 2] / 2.0
        y1 = boxes[:, 1] - boxes[:, 3] / 2.0
        xywh = np.stack([x1, y1, boxes[:, 2], boxes[:, 3]], axis=1)
        indices = nms_boxes(
            xywh.tolist(),
            scores.tolist(),
            self._score_thresh,
        )
        out: List[Box] = []
        for idx in indices:
            x, y, w, h = xywh[idx]
            out.append(
                Box(
                    float(x / scale),
                    float(y / scale),
                    float(w / scale),
                    float(h / scale),
                    float(scores[idx]),
                )
            )
        return out

    def _run_view(self, bgr: np.ndarray) -> List[Box]:
        padded, scale = letterbox(bgr, YOLOX_SIZE)
        rgb = padded[:, :, ::-1].astype(np.float32)
        blob = np.transpose(rgb, (2, 0, 1))[None]
        raw = self._session.run(None, {self._input_name: blob})[0]
        if raw.ndim == 2:
            raw = raw[None]
        return self._decode_yolox(raw, scale)

    def detect_phones(self, bgr: np.ndarray) -> List[Box]:
        height, width = bgr.shape[:2]
        merged: List[Box] = []
        for crop, ox, oy in phone_views(bgr):
            if crop.size == 0:
                continue
            for box in self._run_view(crop):
                x2 = min(box.x + box.w + ox, float(width))
                y2 = min(box.y + box.h + oy, float(height))
                x1 = min(max(box.x + ox, 0.0), float(width - 1))
                y1 = min(max(box.y + oy, 0.0), float(height - 1))
                bw = x2 - x1
                bh = y2 - y1
                if bw < 8 or bh < 8:
                    continue
                merged.append(Box(x1, y1, bw, bh, box.score))
        if not merged:
            return []
        xywh = [[b.x, b.y, b.w, b.h] for b in merged]
        scores = [b.score for b in merged]
        keep = nms_boxes(xywh, scores, self._score_thresh)
        return [merged[i] for i in keep]

    def detect_persons(self, bgr: np.ndarray) -> List[Box]:
        return []


class UltralyticsYoloDetector:
    """YOLOv8 / YOLO11 ONNX export (1, 84, N) with built-in NMS-free decode."""

    def __init__(
        self,
        onnx_path: Path,
        score_thresh: float = 0.25,
        input_size: int = 640,
        threads: int = 4,
        dual_view: bool = True,
    ):
        opts = ort.SessionOptions()
        opts.intra_op_num_threads = threads
        opts.inter_op_num_threads = 1
        self._session = ort.InferenceSession(
            str(onnx_path),
            sess_options=opts,
            providers=["CPUExecutionProvider"],
        )
        self._score_thresh = score_thresh
        self._size = input_size
        self._dual_view = dual_view
        self._input_name = self._session.get_inputs()[0].name

    def _decode(self, raw: np.ndarray, scale: float) -> List[Box]:
        # (1, 4+nc, anchors) -> transpose to (anchors, 4+nc)
        if raw.ndim == 3:
            preds = raw[0].T
        else:
            preds = raw.T
        boxes_xywh = preds[:, :4]
        scores_all = preds[:, 4:]
        class_ids = scores_all.argmax(axis=1)
        scores = scores_all.max(axis=1)
        keep = scores >= self._score_thresh
        if not np.any(keep):
            return []
        boxes_xywh = boxes_xywh[keep]
        scores = scores[keep]
        class_ids = class_ids[keep]
        xywh = []
        out: List[Box] = []
        for (cx, cy, w, h), sc, cid in zip(boxes_xywh, scores, class_ids):
            x = (cx - w / 2.0) / scale
            y = (cy - h / 2.0) / scale
            bw = w / scale
            bh = h / scale
            xywh.append([float(x), float(y), float(bw), float(bh)])
            out.append(
                Box(float(x), float(y), float(bw), float(bh), float(sc), int(cid))
            )
        if not out:
            return []
        indices = nms_boxes(xywh, [b.score for b in out], self._score_thresh)
        return [out[i] for i in indices]

    def _run_view(self, bgr: np.ndarray) -> List[Box]:
        padded, scale = letterbox(bgr, self._size)
        rgb = padded[:, :, ::-1].astype(np.float32) / 255.0
        blob = np.transpose(rgb, (2, 0, 1))[None]
        raw = self._session.run(None, {self._input_name: blob})[0]
        return self._decode(raw, scale)

    def _detect_all(self, bgr: np.ndarray) -> List[Box]:
        height, width = bgr.shape[:2]
        views = phone_views(bgr) if self._dual_view else [(bgr, 0, 0)]
        merged: List[Box] = []
        for crop, ox, oy in views:
            if crop.size == 0:
                continue
            for box in self._run_view(crop):
                x1 = min(max(box.x + ox, 0.0), float(width - 1))
                y1 = min(max(box.y + oy, 0.0), float(height - 1))
                x2 = min(box.x + box.w + ox, float(width))
                y2 = min(box.y + box.h + oy, float(height))
                bw = x2 - x1
                bh = y2 - y1
                if bw < 8 or bh < 8:
                    continue
                merged.append(Box(x1, y1, bw, bh, box.score, box.class_id))
        if not merged:
            return []
        xywh = [[b.x, b.y, b.w, b.h] for b in merged]
        scores = [b.score for b in merged]
        keep = nms_boxes(xywh, scores, self._score_thresh)
        return [merged[i] for i in keep]

    def detect_phones(self, bgr: np.ndarray) -> List[Box]:
        return [b for b in self._detect_all(bgr) if b.class_id == CELL_PHONE_CLASS]

    def detect_persons(self, bgr: np.ndarray) -> List[Box]:
        return [b for b in self._detect_all(bgr) if b.class_id == PERSON_CLASS]


def load_detector(
    name: str,
    models_dir: Path,
    score_thresh: float = 0.25,
    threads: int = 4,
) -> YoloXPhoneDetector | UltralyticsYoloDetector:
    path = models_dir / name
    if name == "yolox_s.onnx":
        return YoloXPhoneDetector(path, score_thresh=0.15, threads=threads)
    if "_416" in name:
        input_size = 416
    elif "_512" in name:
        input_size = 512
    else:
        input_size = 640
    return UltralyticsYoloDetector(
        path,
        score_thresh=score_thresh,
        input_size=input_size,
        threads=threads,
    )
