# Eval data (gitignored)

Run from repo root:

```powershell
.\.venv-webcam-tools\Scripts\python webcam_bakeoff\eval\build_eval_set.py
.\.venv-webcam-tools\Scripts\python webcam_bakeoff\eval\build_eval_set.py --youtube
```

Produces `images/`, `labels.csv`, and cached COCO annotations under `coco/`.
