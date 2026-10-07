# Webcam detector results

Eval set: 605 COCO val2017 photos (`eval/build_eval_set.py`). 214 contain a phone. 62 of those also contain a person and a phone covering at least 1% of the photo (the webcam-like slice). 250 are look-alikes (remote, mouse, book, cup, keyboard, bottle, clock) with no phone.

CPU times below are one full-frame pass on this laptop (i5-13450HX, 4 threads). GPU times use DirectML on the RTX 4050.

## Phone detection, full frame, threshold 0.15

| Model | File | Phone F1 | Webcam-like recall | Look-alike false phones | CPU ms | GPU ms |
| --- | --- | --- | --- | --- | --- | --- |
| YOLO26n | 10 MB | 0.66 | 0.79 | 2% | 44 |  |
| YOLO11n | 10 MB | 0.67 | 0.77 | 3% | 70 |  |
| **YOLO26s** | **37 MB** | **0.80** | **0.90** | **4%** | **95** | **35** |
| YOLO11s | 38 MB | 0.77 | 0.82 | 4% | 97 |  |
| YOLO26m | 80 MB | 0.83 | 0.89 | 4% | 242 | 36 |

**Shipped: YOLO26s, full frame only, phone threshold 0.15, person threshold 0.35, 2 frames per second.**

A second pass on the lower half of the frame found a few more phones and a lot more look-alikes, and doubled the time. It is off.

YOLO26 medium is a little more accurate and just as fast on a GPU, but more than twice as slow on a CPU. CPU is the default, because not every laptop has a usable GPU.

The old shipped stack was YOLO11n at threshold 0.06 with two views: phone F1 0.76 and 14% look-alike false phones on this larger set.

Presence (YuNet face or YOLO person) is about 0.98 F1 for every model here. Gaze stays off.

## What can still go wrong

- A remote or a thick case that scores above 0.15 looks like a phone on every frame. Hysteresis cannot filter that, because it never misses.
- These numbers are COCO photos, not a webcam at a desk. Check it on a real camera before trusting the threshold.
- GPU mode keeps a laptop GPU awake. It is a setting, off by default, and it reports an error if the GPU provider cannot start.

Full rows: `eval/results_v2.jsonl` (every model and threshold) and `eval/results_final.jsonl` (CPU vs GPU). Both are gitignored.
