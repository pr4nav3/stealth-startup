#!/usr/bin/env python3
"""Supervisor Chatterbox Nano phrase generator.

This is the app worker, not the research bakeoff harness. It accepts explicit
paths from the Rust backend, writes cached WAV clips, and exits so the main app
never keeps the model resident.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import multiprocessing
import os
import re
import sys
import time
import traceback
from pathlib import Path
from typing import Any, Dict, List, Sequence


WORKER_API_VERSION = 1


class WorkerError(Exception):
    """Expected input/setup failure."""


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def slugify(value: str) -> str:
    slug = re.sub(r"[^a-zA-Z0-9]+", "_", value.strip().lower()).strip("_")
    return slug or "phrase"


def read_json(path: Path) -> Dict[str, Any]:
    try:
        with path.open("r", encoding="utf-8") as handle:
            data = json.load(handle)
    except FileNotFoundError as exc:
        raise WorkerError(f"File not found: {path}") from exc
    except json.JSONDecodeError as exc:
        raise WorkerError(f"Invalid JSON in {path}: {exc}") from exc

    if not isinstance(data, dict):
        raise WorkerError(f"Expected a JSON object in {path}")
    return data


def write_json(path: Path, data: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2, sort_keys=True)
        handle.write("\n")


def load_phrases(path: Path) -> List[Dict[str, str]]:
    data = read_json(path)
    phrases = data.get("phrases")
    if not isinstance(phrases, list) or not phrases:
        raise WorkerError(f"{path} must contain a non-empty 'phrases' list")

    normalized: List[Dict[str, str]] = []
    seen_ids = set()
    for index, item in enumerate(phrases, start=1):
        if not isinstance(item, dict):
            raise WorkerError(f"Phrase {index} must be an object")
        phrase_id = str(item.get("id") or "").strip()
        tone = str(item.get("tone") or "").strip()
        text = str(item.get("text") or "").strip()
        if not phrase_id:
            phrase_id = slugify(text)[:80]
        if not tone:
            raise WorkerError(f"Phrase {index} is missing 'tone'")
        if not text:
            raise WorkerError(f"Phrase {index} is missing 'text'")
        if phrase_id in seen_ids:
            raise WorkerError(f"Duplicate phrase id: {phrase_id}")
        seen_ids.add(phrase_id)
        normalized.append({"id": phrase_id, "tone": tone, "text": text})
    return normalized


def patch_perth_watermarker() -> bool:
    try:
        import perth
    except Exception:
        return False

    class NoOpWatermarker:
        def apply_watermark(self, wav: Any, sample_rate: int, **_: Any) -> Any:
            return wav

    perth.PerthImplicitWatermarker = NoOpWatermarker
    return True


def load_model(args: argparse.Namespace) -> tuple[Any, Any, bool]:
    try:
        import torch
        from chatterbox.tts_turbo import ChatterboxTurboTTS
    except Exception as exc:
        raise WorkerError(f"Could not import the Chatterbox Nano runtime: {exc}") from exc

    if args.threads is not None:
        torch.set_num_threads(args.threads)
    if args.seed is not None:
        torch.manual_seed(args.seed)

    patched = patch_perth_watermarker()
    model = ChatterboxTurboTTS.from_local(str(args.model_dir), device=args.device, nano=True)
    return model, torch, patched


def configure_runtime_environment(out_dir: Path) -> None:
    hf_home = out_dir / ".hf_cache"
    os.environ.setdefault("HF_HOME", str(hf_home))
    os.environ.setdefault("HF_HUB_CACHE", str(hf_home / "hub"))
    os.environ.setdefault("HUGGINGFACE_HUB_CACHE", str(hf_home / "hub"))
    os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
    os.environ.setdefault("NUMBA_CACHE_DIR", str(out_dir / ".numba_cache"))


def generate_clip(
    args: argparse.Namespace,
    model: Any,
    torch_module: Any,
    phrase: Dict[str, str],
    output_path: Path,
) -> Dict[str, Any]:
    import torchaudio as ta

    started = time.perf_counter()
    wav = model.generate(
        phrase["text"],
        audio_prompt_path=str(args.reference),
        temperature=args.temperature,
        top_p=args.top_p,
        top_k=args.top_k,
        repetition_penalty=args.repetition_penalty,
    )
    save_wav = wav.detach().cpu() if hasattr(wav, "detach") else torch_module.as_tensor(wav).detach().cpu()
    if len(save_wav.shape) == 1:
        save_wav = save_wav.unsqueeze(0)

    ta.save(str(output_path), save_wav, model.sr, encoding="PCM_S", bits_per_sample=16)
    elapsed = time.perf_counter() - started
    audio_seconds = float(save_wav.shape[-1]) / float(model.sr)
    return {
        "phrase_id": phrase["id"],
        "tone": phrase["tone"],
        "text": phrase["text"],
        "path": str(output_path.resolve()),
        "generation_seconds": round(elapsed, 3),
        "audio_seconds": round(audio_seconds, 3),
    }


def run(args: argparse.Namespace) -> int:
    started_at = utc_now()
    if not args.model_dir.exists():
        raise WorkerError(f"Model directory not found: {args.model_dir}")
    if not args.reference.exists():
        raise WorkerError(f"Reference audio not found: {args.reference}")
    if args.out_dir.exists() and any(args.out_dir.iterdir()) and not args.overwrite:
        raise WorkerError(f"Output directory is not empty: {args.out_dir}")

    phrases = load_phrases(args.phrases)
    clips_dir = args.out_dir / "clips"
    clips_dir.mkdir(parents=True, exist_ok=True)
    configure_runtime_environment(args.out_dir)

    load_started = time.perf_counter()
    model, torch_module, watermarker_patched = load_model(args)
    model_load_seconds = round(time.perf_counter() - load_started, 3)

    clips: List[Dict[str, Any]] = []
    generation_started = time.perf_counter()
    for index, phrase in enumerate(phrases, start=1):
        output_path = clips_dir / f"{index:02d}_{slugify(phrase['id'])}.wav"
        if output_path.exists() and not args.overwrite:
            raise WorkerError(f"Clip already exists: {output_path}")
        clips.append(generate_clip(args, model, torch_module, phrase, output_path))

    generation_seconds = round(time.perf_counter() - generation_started, 3)
    report = {
        "schema_version": 1,
        "worker_api": WORKER_API_VERSION,
        "status": "completed",
        "started_at": started_at,
        "completed_at": utc_now(),
        "engine": "chatterbox-nano",
        "device": args.device,
        "model_load_seconds": model_load_seconds,
        "generation_total_seconds": generation_seconds,
        "sample_rate": model.sr,
        "clip_count": len(clips),
        "clips": clips,
        "workarounds": {
            "noop_watermarker": watermarker_patched,
        },
    }
    write_json(args.out_dir / "worker_report.json", report)
    return 0


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate Supervisor voice clips with Chatterbox Nano.")
    parser.add_argument("--model-dir", type=Path, required=True)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--phrases", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--device", default="cpu")
    parser.add_argument("--overwrite", action="store_true")
    parser.add_argument("--threads", type=int)
    parser.add_argument("--seed", type=int)
    parser.add_argument("--temperature", type=float, default=0.8)
    parser.add_argument("--top-p", type=float, default=0.95)
    parser.add_argument("--top-k", type=int, default=1000)
    parser.add_argument("--repetition-penalty", type=float, default=1.2)
    args = parser.parse_args(argv)
    args.model_dir = args.model_dir.expanduser().resolve()
    args.reference = args.reference.expanduser().resolve()
    args.phrases = args.phrases.expanduser().resolve()
    args.out_dir = args.out_dir.expanduser().resolve()
    if args.threads is not None and args.threads <= 0:
        raise WorkerError("--threads must be greater than zero")
    return args


def main(argv: Sequence[str] | None = None) -> int:
    multiprocessing.freeze_support()
    try:
        return run(parse_args(argv))
    except WorkerError as exc:
        print(f"voice worker error: {exc}", file=sys.stderr)
        if os.getenv("SUPERVISOR_VOICE_WORKER_DEBUG") and exc.__cause__ is not None:
            traceback.print_exception(exc.__cause__, file=sys.stderr)
        return 2
    except Exception:
        traceback.print_exc(file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
