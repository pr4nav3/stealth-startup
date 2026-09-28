# Webcam detector bakeoff results

Eval set: **318 frames** (`eval/build_eval_set.py`) — 120 COCO val images with cell-phone boxes, 120 hard negatives (remote/mouse/book/cup/keyboard/bottle/clock, no phone), 78 person-without-phone. Sources are COCO val2017 only in this run (YouTube optional via `--youtube`).

CPU: 4 ONNX Runtime threads, dual-view (full frame + lower-center square), YuNet face @ 0.55 for presence fusion **face OR YOLO person**.

## Phone detection (COCO-centric eval)

| Model | Input | score | Phone F1 | Phone recall | Hard-neg phone FP | ms/frame | Est. FPS |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **YOLOX-s (baseline)** | 640 | 0.15 | **0.692** | 0.600 | 0.075 | 180 | 5.6 |
| YOLOv8n | 640 | 0.15 | 0.670 | 0.533 | 0.058 | 181 | 5.5 |
| YOLO11n | 640 | 0.25 | 0.586 | 0.425 | 0.025 | 241 | 4.2 |
| **YOLO11n (chosen)** | 640 | **0.06** | **0.720** | **0.633** | 0.092 | 185 | 5.4 |
| YOLO11s | 640 | 0.25 | 0.719 | 0.575 | **0.025** | 338 | 3.0 |
| YOLO11s | 512 | 0.15 | 0.711 | 0.583 | 0.050 | 436 | 2.3 |
| YOLO11n | 512 | 0.20 | 0.602 | 0.442 | 0.025 | 90 | 11.1 |

**Chosen runtime stack:** YuNet + **YOLO11n @ 640** (`phone_score=0.06`), single ORT session, dual-view phone decode, temporal hysteresis on phone/presence.

**Why:** Best **phone F1 (0.72)** at **≥4 FPS** on this machine. YOLO11s beats hard-negative FP but cannot hold 4 FPS with dual-view + face. YOLOX baseline is slightly worse on F1 and recall. Live hysteresis (3-of-5 on, 2-clear off) trims single-frame FPs further.

Sources: [YOLO11](https://github.com/ultralytics/ultralytics), [YOLOX](https://github.com/Megvii-BaseDetection/YOLOX), [YuNet](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet).

## Presence (face OR person)

| Model | Presence F1 | Notes |
| --- | --- | --- |
| YOLOX + YuNet (face only) | 0.805 | No person class |
| **YOLO11n + YuNet** | **0.976** | Person backs face when head turns / looks down |

SCRFD was not adopted: YuNet + YOLO person matched presence needs without a second face model.

## Not run (time / CPU budget)

RT-DETR, D-FINE, DEIM, RF-DETR — no ONNX exports attempted in this pass; YOLO11n met the FPS gate with the best F1 among models we exported.

## Known failure cases

- COCO hard negatives still fire ~9% at score 0.06 (remotes, thick cases). Hysteresis + strict mode for alerts.
- Very small / edge-on phones in the upper frame (dual-view targets lap/desk).
- Webcam-perspective YouTube frames not in the default eval CSV (optional builder flag).

## Reproduce

```powershell
.\.venv-webcam-tools\Scripts\python webcam_bakeoff\eval\build_eval_set.py
.\.venv-webcam\Scripts\python webcam_bakeoff\benchmark.py --model yolox_s.onnx --phone-score 0.15
.\.venv-webcam\Scripts\python webcam_bakeoff\benchmark.py --model yolo11n.onnx --phone-score 0.06
```
