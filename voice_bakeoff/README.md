# Voice Bakeoff

Eval only as of 2026-10-16. The app worker is `voice_worker/`. Do not treat this folder as the in-app path.

Phase 0 decides whether Chatterbox-Nano is good enough for Supervisor V1 before any full voice UI work.

## Setup

Chatterbox is documented against Python 3.11, so use that if it is available:

```bash
python3.11 -m venv .venv
source .venv/bin/activate
pip install -r voice_bakeoff/requirements.txt
```

This bakeoff pins Chatterbox to the official GitHub source commit used in setup because the current PyPI wheel installed here exposed Turbo but not the documented Nano loader.

The first model run may download weights from Hugging Face. For the cleanest timing, run once to warm the cache, then run again for the actual bakeoff measurement.

By default, model caches stay inside `voice_bakeoff/model_cache/` rather than the user-level Hugging Face cache.

## Download Chatterbox-Nano Weights

You can prepare the model cache before a reference clip is ready:

```bash
python voice_bakeoff/generate_phrase_bank.py \
  --prepare-only \
  --device cpu
```

This loads Chatterbox-Nano in a short-lived worker process, downloads the local weights if needed, writes a preparation report, and exits.

On this macOS setup, `resemble-perth` did not expose a callable implicit watermarker. The bakeoff harness patches that to a no-op watermarker and records `workarounds.noop_watermarker` in `worker_report.json`; generation tests can still run, but packaging needs a later decision on whether to ship with this workaround or resolve Perth per platform.

## Download Chatterbox-Nano ONNX/int4

The ONNX/int4 bakeoff lane uses the independent `owensong/chatterbox-nano-ONNX` conversion. It is not an official Resemble AI release, so treat it as an evaluation candidate until quality and compatibility are tested.

```bash
hf download owensong/chatterbox-nano-ONNX \
  --local-dir voice_bakeoff/onnx_models/chatterbox-nano-ONNX \
  --max-workers 4
```

Verify the downloaded files:

```bash
cd voice_bakeoff/onnx_models/chatterbox-nano-ONNX
tr -d '\r' < SHA256SUMS | shasum -a 256 -c -
```

Smoke-test that all four q4f16 graphs load:

```bash
python -c 'from pathlib import Path; import time; import psutil; import os; import run_onnx; p=psutil.Process(os.getpid()); model=Path(".").resolve(); before=p.memory_info().rss; started=time.perf_counter(); sessions={name: run_onnx.make_session(run_onnx.graph_path(model, name, "q4f16"), "cpu") for name in ("embed_tokens", "speech_encoder", "language_model", "conditional_decoder")}; after=p.memory_info().rss; print(f"loaded {len(sessions)} ONNX sessions in {time.perf_counter() - started:.2f}s; rss_delta_mb={(after-before)/(1024*1024):.2f}; rss_mb={after/(1024*1024):.2f}")'
```

## Run Chatterbox-Nano

Use a 20 to 30 second WAV reference clip that you own or have permission to use:

```bash
python voice_bakeoff/generate_phrase_bank.py \
  --reference /path/to/reference.wav \
  --device cpu
```

## Run Chatterbox-Nano ONNX/int4

Use the same reference clip and one phrase first:

```bash
cd voice_bakeoff/onnx_models/chatterbox-nano-ONNX
python run_onnx.py \
  --model . \
  --voice /path/to/reference.wav \
  --output ../../runs/onnx-smoke/output.wav \
  --text "Deep breath. Back to the thing." \
  --dtype q4f16 \
  --provider cpu \
  --max-new-tokens 256 \
  --seed 1337
```

The PyTorch phrase-bank script writes a timestamped run folder under `voice_bakeoff/runs/`:

```text
runs/<timestamp>/
  clips/
  input_phrases.json
  quality_review.csv
  report.json
  worker_report.json
```

## Gate

Compare the generated `report.json` against the V1 targets:

| Metric | Preferred | Maximum |
| --- | ---: | ---: |
| Cold model load | 30 sec | - |
| Generate 20 short clips | 3 min | 8 min |
| Voice worker peak RAM | 2 GB | 4 GB |
| Parent process RAM after worker exits | 300 MB | 1 GB |

Then listen through `quality_review.csv` and score voice similarity, admonishment quality, and hallucinations. Nano passes only if the measured numbers are acceptable and the subjective scores are 4/5 or better with zero hallucinated phrase-bank clips.
