#!/usr/bin/env python3
"""Generate a Supervisor V1 voice phrase bank and measure the worker process.

The parent process stays lightweight and launches a short-lived worker for
Chatterbox/PyTorch. This mirrors the intended app architecture: the focus app
should play cached clips and should not keep the voice model resident.
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import json
import os
import platform
import queue
import re
import subprocess
import sys
import threading
import time
import traceback
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence, Tuple


SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_PHRASES = SCRIPT_DIR / "phrases.json"
DEFAULT_RUNS_DIR = SCRIPT_DIR / "runs"
DEFAULT_CACHE_DIR = SCRIPT_DIR / "model_cache"

ENGINE_CHOICES = ("chatterbox-nano", "chatterbox-turbo")

THRESHOLDS = {
    "model_load_seconds_preferred": 30.0,
    "generate_20_seconds_preferred": 180.0,
    "generate_20_seconds_maximum": 480.0,
    "worker_peak_rss_mb_preferred": 2048.0,
    "worker_peak_rss_mb_maximum": 4096.0,
    "parent_rss_after_exit_mb_preferred": 300.0,
    "parent_rss_after_exit_mb_maximum": 1024.0,
}


class BakeoffError(Exception):
    """Raised for expected bakeoff input/setup failures."""


def utc_now() -> dt.datetime:
    return dt.datetime.now(dt.timezone.utc)


def iso_timestamp(value: dt.datetime) -> str:
    return value.isoformat(timespec="seconds").replace("+00:00", "Z")


def run_id_from_time(value: dt.datetime) -> str:
    return value.strftime("%Y%m%dT%H%M%SZ")


def slugify(value: str) -> str:
    slug = re.sub(r"[^a-zA-Z0-9]+", "_", value.strip().lower()).strip("_")
    return slug or "phrase"


def read_json(path: Path) -> Dict[str, Any]:
    try:
        with path.open("r", encoding="utf-8") as handle:
            data = json.load(handle)
    except FileNotFoundError:
        raise BakeoffError(f"File not found: {path}")
    except json.JSONDecodeError as exc:
        raise BakeoffError(f"Invalid JSON in {path}: {exc}") from exc

    if not isinstance(data, dict):
        raise BakeoffError(f"Expected a JSON object in {path}")
    return data


def write_json(path: Path, data: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2, sort_keys=True)
        handle.write("\n")


def load_phrase_bank(path: Path, limit: Optional[int] = None) -> Dict[str, Any]:
    data = read_json(path)
    phrases = data.get("phrases")
    if not isinstance(phrases, list) or not phrases:
        raise BakeoffError(f"{path} must contain a non-empty 'phrases' list")

    normalized: List[Dict[str, str]] = []
    seen_ids = set()
    for index, item in enumerate(phrases, start=1):
        if not isinstance(item, dict):
            raise BakeoffError(f"Phrase {index} must be an object")

        phrase_id = str(item.get("id") or "").strip()
        tone = str(item.get("tone") or "").strip()
        text = str(item.get("text") or "").strip()
        if not phrase_id:
            phrase_id = slugify(text)[:80]
        if not tone:
            raise BakeoffError(f"Phrase {index} is missing 'tone'")
        if not text:
            raise BakeoffError(f"Phrase {index} is missing 'text'")
        if phrase_id in seen_ids:
            raise BakeoffError(f"Duplicate phrase id: {phrase_id}")
        seen_ids.add(phrase_id)
        normalized.append({"id": phrase_id, "tone": tone, "text": text})

    if limit is not None:
        if limit <= 0:
            raise BakeoffError("--limit must be greater than zero")
        normalized = normalized[:limit]

    return {
        "schema_version": data.get("schema_version", 1),
        "description": data.get("description", ""),
        "phrases": normalized,
    }


def copy_phrase_snapshot(phrase_bank: Dict[str, Any], out_dir: Path) -> Path:
    path = out_dir / "input_phrases.json"
    write_json(path, phrase_bank)
    return path


def write_quality_review_template(
    out_dir: Path,
    phrases: Sequence[Dict[str, str]],
    clip_paths_by_id: Optional[Dict[str, str]] = None,
) -> Path:
    path = out_dir / "quality_review.csv"
    fieldnames = [
        "phrase_id",
        "tone",
        "expected_text",
        "clip_path",
        "transcript_matches",
        "voice_similarity_1_to_5",
        "admonishment_quality_1_to_5",
        "hallucination_notes",
        "accept_for_v1",
    ]
    clip_paths_by_id = clip_paths_by_id or {}
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        for phrase in phrases:
            writer.writerow(
                {
                    "phrase_id": phrase["id"],
                    "tone": phrase["tone"],
                    "expected_text": phrase["text"],
                    "clip_path": clip_paths_by_id.get(phrase["id"], ""),
                    "transcript_matches": "",
                    "voice_similarity_1_to_5": "",
                    "admonishment_quality_1_to_5": "",
                    "hallucination_notes": "",
                    "accept_for_v1": "",
                }
            )
    return path


def import_psutil():
    try:
        import psutil  # type: ignore

        return psutil, None
    except Exception as exc:  # pragma: no cover - depends on local environment
        return None, str(exc)


def process_tree_rss_bytes(psutil_module: Any, pid: int) -> int:
    try:
        process = psutil_module.Process(pid)
    except Exception:
        return 0

    processes = [process]
    try:
        processes.extend(process.children(recursive=True))
    except Exception:
        pass

    total = 0
    for item in processes:
        try:
            total += int(item.memory_info().rss)
        except Exception:
            pass
    return total


def current_process_rss_mb() -> Optional[float]:
    psutil_module, _error = import_psutil()
    if psutil_module is None:
        return None
    rss_bytes = process_tree_rss_bytes(psutil_module, os.getpid())
    return bytes_to_mb(rss_bytes)


def bytes_to_mb(value: Optional[int]) -> Optional[float]:
    if value is None:
        return None
    return round(value / (1024 * 1024), 2)


def enqueue_output(stream: Any, output_queue: "queue.Queue[str]") -> None:
    try:
        for line in iter(stream.readline, ""):
            output_queue.put(line)
    finally:
        try:
            stream.close()
        except Exception:
            pass


def build_worker_command(args: argparse.Namespace, out_dir: Path, phrases_path: Path) -> List[str]:
    command = [
        sys.executable,
        str(Path(__file__).resolve()),
        "--worker",
        "--engine",
        args.engine,
        "--phrases",
        str(phrases_path),
        "--out-dir",
        str(out_dir),
        "--device",
        args.device,
        "--temperature",
        str(args.temperature),
        "--top-p",
        str(args.top_p),
        "--top-k",
        str(args.top_k),
        "--repetition-penalty",
        str(args.repetition_penalty),
        "--exaggeration",
        str(args.exaggeration),
        "--cfg-weight",
        str(args.cfg_weight),
        "--min-p",
        str(args.min_p),
        "--cache-dir",
        str(args.cache_dir),
    ]
    if args.reference:
        command.extend(["--reference", str(args.reference)])
    if args.model_dir:
        command.extend(["--model-dir", str(args.model_dir)])
    if args.seed is not None:
        command.extend(["--seed", str(args.seed)])
    if args.threads is not None:
        command.extend(["--threads", str(args.threads)])
    if args.skip_existing:
        command.append("--skip-existing")
    if args.prepare_only:
        command.append("--prepare-only")
    if args.allow_builtin_voice:
        command.append("--allow-built-in-voice")
    return command


def classify_preferred_max(
    actual: Optional[float],
    preferred: Optional[float],
    maximum: Optional[float],
) -> str:
    if actual is None:
        return "not_measured"
    if preferred is not None and actual <= preferred:
        return "preferred_pass"
    if maximum is not None and actual <= maximum:
        return "acceptable_pass"
    return "fail"


def build_gate_summary(report: Dict[str, Any]) -> Dict[str, Any]:
    measurements = report.get("measurements", {})
    load_seconds = measurements.get("model_load_seconds")
    generation_seconds = measurements.get("generation_total_seconds")
    peak_rss_mb = measurements.get("worker_peak_rss_mb")
    parent_rss_mb = measurements.get("parent_rss_after_exit_mb")

    return {
        "cold_model_load": {
            "actual_seconds": load_seconds,
            "preferred_seconds": THRESHOLDS["model_load_seconds_preferred"],
            "status": classify_preferred_max(
                load_seconds,
                THRESHOLDS["model_load_seconds_preferred"],
                None,
            ),
        },
        "generate_phrase_bank": {
            "actual_seconds": generation_seconds,
            "preferred_seconds": THRESHOLDS["generate_20_seconds_preferred"],
            "maximum_seconds": THRESHOLDS["generate_20_seconds_maximum"],
            "status": classify_preferred_max(
                generation_seconds,
                THRESHOLDS["generate_20_seconds_preferred"],
                THRESHOLDS["generate_20_seconds_maximum"],
            ),
        },
        "worker_peak_ram": {
            "actual_mb": peak_rss_mb,
            "preferred_mb": THRESHOLDS["worker_peak_rss_mb_preferred"],
            "maximum_mb": THRESHOLDS["worker_peak_rss_mb_maximum"],
            "status": classify_preferred_max(
                peak_rss_mb,
                THRESHOLDS["worker_peak_rss_mb_preferred"],
                THRESHOLDS["worker_peak_rss_mb_maximum"],
            ),
        },
        "parent_ram_after_worker_exit": {
            "actual_mb": parent_rss_mb,
            "preferred_mb": THRESHOLDS["parent_rss_after_exit_mb_preferred"],
            "maximum_mb": THRESHOLDS["parent_rss_after_exit_mb_maximum"],
            "status": classify_preferred_max(
                parent_rss_mb,
                THRESHOLDS["parent_rss_after_exit_mb_preferred"],
                THRESHOLDS["parent_rss_after_exit_mb_maximum"],
            ),
        },
        "manual_quality_review": {
            "voice_similarity_target": "4/5 or better",
            "admonishment_quality_target": "4/5 or better",
            "bad_hallucinations_target": "zero accepted clips",
            "status": "pending_manual_review",
        },
    }


def run_orchestrator(args: argparse.Namespace) -> int:
    started_at = utc_now()
    run_id = args.run_id or run_id_from_time(started_at)
    out_dir = args.out_dir or (DEFAULT_RUNS_DIR / run_id)
    out_dir = out_dir.resolve()

    if out_dir.exists() and any(out_dir.iterdir()) and not args.overwrite:
        raise BakeoffError(f"Output directory is not empty: {out_dir}. Use --overwrite or choose another --out-dir.")
    out_dir.mkdir(parents=True, exist_ok=True)

    phrase_bank = load_phrase_bank(args.phrases, limit=args.limit)
    phrases_path = copy_phrase_snapshot(phrase_bank, out_dir)
    write_quality_review_template(out_dir, phrase_bank["phrases"])

    configure_model_cache(args.cache_dir)
    psutil_module, psutil_error = import_psutil()
    command = build_worker_command(args, out_dir, phrases_path)

    print(f"Run directory: {out_dir}")
    print(f"Engine: {args.engine}")
    print(f"Device: {args.device}")
    print(f"Phrases: {len(phrase_bank['phrases'])}")
    print("Starting voice worker...")

    worker_started = time.perf_counter()
    worker_env = os.environ.copy()

    process = subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
        env=worker_env,
    )

    output_queue: "queue.Queue[str]" = queue.Queue()
    output_thread = threading.Thread(
        target=enqueue_output,
        args=(process.stdout, output_queue),
        daemon=True,
    )
    output_thread.start()

    worker_log: List[str] = []
    peak_rss_bytes: Optional[int] = None
    rss_samples: List[Dict[str, Any]] = []

    while process.poll() is None:
        while True:
            try:
                line = output_queue.get_nowait()
            except queue.Empty:
                break
            worker_log.append(line)
            print(line, end="")

        if psutil_module is not None:
            rss_bytes = process_tree_rss_bytes(psutil_module, process.pid)
            if rss_bytes:
                peak_rss_bytes = max(peak_rss_bytes or 0, rss_bytes)
                rss_samples.append(
                    {
                        "elapsed_seconds": round(time.perf_counter() - worker_started, 3),
                        "rss_mb": bytes_to_mb(rss_bytes),
                    }
                )
        time.sleep(args.poll_interval)

    return_code = process.wait()
    output_thread.join(timeout=2.0)
    while True:
        try:
            line = output_queue.get_nowait()
        except queue.Empty:
            break
        worker_log.append(line)
        print(line, end="")

    completed_at = utc_now()
    worker_duration_seconds = round(time.perf_counter() - worker_started, 3)

    worker_report_path = out_dir / "worker_report.json"
    worker_report: Dict[str, Any] = {}
    if worker_report_path.exists():
        worker_report = read_json(worker_report_path)

    clip_paths_by_id = {
        clip["phrase_id"]: clip.get("path", "")
        for clip in worker_report.get("clips", [])
        if isinstance(clip, dict) and clip.get("phrase_id")
    }
    write_quality_review_template(out_dir, phrase_bank["phrases"], clip_paths_by_id=clip_paths_by_id)

    log_path = out_dir / "worker.log"
    log_path.write_text("".join(worker_log), encoding="utf-8")

    parent_rss_after_exit_mb = current_process_rss_mb()
    measurements = {
        "worker_return_code": return_code,
        "worker_wall_seconds": worker_duration_seconds,
        "worker_peak_rss_mb": bytes_to_mb(peak_rss_bytes),
        "worker_rss_sample_count": len(rss_samples),
        "parent_rss_after_exit_mb": parent_rss_after_exit_mb,
        "model_load_seconds": worker_report.get("model_load_seconds"),
        "generation_total_seconds": worker_report.get("generation_total_seconds"),
        "generated_audio_seconds": worker_report.get("generated_audio_seconds"),
        "generation_audio_realtime_factor": worker_report.get("generation_audio_realtime_factor"),
    }

    status = worker_report.get("status", "completed") if return_code == 0 else "failed"
    report: Dict[str, Any] = {
        "schema_version": 1,
        "run_id": run_id,
        "status": status,
        "started_at": iso_timestamp(started_at),
        "completed_at": iso_timestamp(completed_at),
        "engine": args.engine,
        "device": args.device,
        "inputs": {
            "reference_audio": str(args.reference.resolve()) if args.reference else None,
            "allow_builtin_voice": args.allow_builtin_voice,
            "phrases_file": str(args.phrases.resolve()),
            "phrase_count": len(phrase_bank["phrases"]),
            "model_dir": str(args.model_dir.resolve()) if args.model_dir else None,
            "cache_dir": str(args.cache_dir.resolve()),
        },
        "parameters": generation_parameters_from_args(args),
        "environment": environment_snapshot(),
        "thresholds": THRESHOLDS,
        "measurements": measurements,
        "outputs": {
            "run_dir": str(out_dir),
            "clips_dir": str((out_dir / "clips").resolve()),
            "worker_report": str(worker_report_path.resolve()),
            "worker_log": str(log_path.resolve()),
            "quality_review": str((out_dir / "quality_review.csv").resolve()),
        },
        "monitoring": {
            "psutil_available": psutil_module is not None,
            "psutil_error": psutil_error,
            "rss_samples": rss_samples[-20:],
            "rss_samples_note": "Only the last 20 samples are kept in this report.",
        },
        "notes": [
            "If the Hugging Face cache is cold, model_load_seconds includes model download time as well as local load time.",
            "Chatterbox Nano and Turbo currently ignore cfg_weight, exaggeration, and min_p; they are recorded for cross-model comparability.",
            "Subjective voice similarity, admonishment quality, and hallucination checks must be filled in quality_review.csv after listening.",
        ],
    }
    report["gate_summary"] = build_gate_summary(report)
    write_json(out_dir / "report.json", report)

    print(f"\nWrote report: {out_dir / 'report.json'}")
    print(f"Wrote review sheet: {out_dir / 'quality_review.csv'}")
    return return_code


def generation_parameters_from_args(args: argparse.Namespace) -> Dict[str, Any]:
    return {
        "temperature": args.temperature,
        "top_p": args.top_p,
        "top_k": args.top_k,
        "repetition_penalty": args.repetition_penalty,
        "exaggeration": args.exaggeration,
        "cfg_weight": args.cfg_weight,
        "min_p": args.min_p,
        "seed": args.seed,
        "threads": args.threads,
        "skip_existing": args.skip_existing,
        "prepare_only": args.prepare_only,
    }


def model_cache_env(cache_dir: Path) -> Dict[str, str]:
    huggingface_home = cache_dir / "huggingface"
    torch_home = cache_dir / "torch"
    xdg_cache_home = cache_dir / "xdg"
    for path in (huggingface_home, torch_home, xdg_cache_home):
        path.mkdir(parents=True, exist_ok=True)
    return {
        "HF_HOME": str(huggingface_home),
        "HF_HUB_CACHE": str(huggingface_home / "hub"),
        "TORCH_HOME": str(torch_home),
        "XDG_CACHE_HOME": str(xdg_cache_home),
    }


def configure_model_cache(cache_dir: Path) -> None:
    os.environ.update(model_cache_env(cache_dir))


def environment_snapshot() -> Dict[str, Any]:
    return {
        "python_executable": sys.executable,
        "python_version": sys.version.replace("\n", " "),
        "platform": platform.platform(),
        "machine": platform.machine(),
        "processor": platform.processor(),
        "hf_home": os.environ.get("HF_HOME"),
        "hf_hub_cache": os.environ.get("HF_HUB_CACHE"),
        "torch_home": os.environ.get("TORCH_HOME"),
        "xdg_cache_home": os.environ.get("XDG_CACHE_HOME"),
    }


def validate_common_args(args: argparse.Namespace) -> None:
    args.phrases = args.phrases.resolve()
    if args.reference:
        args.reference = args.reference.expanduser().resolve()
    if args.model_dir:
        args.model_dir = args.model_dir.expanduser().resolve()
    if args.out_dir:
        args.out_dir = args.out_dir.expanduser().resolve()
    args.cache_dir = args.cache_dir.expanduser().resolve()

    if args.engine not in ENGINE_CHOICES:
        raise BakeoffError(f"Unsupported engine: {args.engine}")
    if args.reference and not args.reference.exists():
        raise BakeoffError(f"Reference audio not found: {args.reference}")
    if args.model_dir and not args.model_dir.exists():
        raise BakeoffError(f"Model directory not found: {args.model_dir}")
    if not args.prepare_only and not args.allow_builtin_voice and not args.reference:
        raise BakeoffError("A reference clip is required unless --allow-built-in-voice is set.")
    if args.threads is not None and args.threads <= 0:
        raise BakeoffError("--threads must be greater than zero")
    if args.poll_interval <= 0:
        raise BakeoffError("--poll-interval must be greater than zero")


def patch_perth_watermarker() -> bool:
    try:
        import perth
    except Exception:
        return False

    if getattr(perth, "PerthImplicitWatermarker", None) is not None:
        return False

    class NoOpWatermarker:
        def apply_watermark(self, wav: Any, sample_rate: int) -> Any:
            return wav

    perth.PerthImplicitWatermarker = NoOpWatermarker
    print("Warning: Perth watermarker unavailable; using no-op watermarker for bakeoff.", flush=True)
    return True


def load_chatterbox_model(args: argparse.Namespace) -> Tuple[Any, Any, bool]:
    configure_model_cache(args.cache_dir)
    try:
        import torch
        from chatterbox.tts_turbo import ChatterboxTurboTTS
    except Exception as exc:
        raise BakeoffError(
            "Could not import Chatterbox. Install dependencies with: "
            "pip install -r voice_bakeoff/requirements.txt"
        ) from exc

    if args.threads is not None:
        torch.set_num_threads(args.threads)
    if args.seed is not None:
        torch.manual_seed(args.seed)

    watermarker_patched = patch_perth_watermarker()
    nano = args.engine == "chatterbox-nano"
    if args.model_dir:
        model = ChatterboxTurboTTS.from_local(str(args.model_dir), device=args.device, nano=nano)
    else:
        model = ChatterboxTurboTTS.from_pretrained(device=args.device, nano=nano)

    return model, torch, watermarker_patched


def worker_generate_clip(
    args: argparse.Namespace,
    model: Any,
    torch_module: Any,
    phrase: Dict[str, str],
    output_path: Path,
) -> Dict[str, Any]:
    import torchaudio as ta

    started = time.perf_counter()
    generate_kwargs = {
        "temperature": args.temperature,
        "top_p": args.top_p,
        "top_k": args.top_k,
        "repetition_penalty": args.repetition_penalty,
        "exaggeration": args.exaggeration,
        "cfg_weight": args.cfg_weight,
        "min_p": args.min_p,
    }
    if args.reference:
        generate_kwargs["audio_prompt_path"] = str(args.reference)

    wav = model.generate(phrase["text"], **generate_kwargs)
    if hasattr(wav, "detach"):
        save_wav = wav.detach().cpu()
    else:
        save_wav = torch_module.as_tensor(wav).detach().cpu()
    if len(save_wav.shape) == 1:
        save_wav = save_wav.unsqueeze(0)

    ta.save(str(output_path), save_wav, model.sr)
    elapsed = time.perf_counter() - started
    audio_seconds = float(save_wav.shape[-1]) / float(model.sr)

    return {
        "phrase_id": phrase["id"],
        "tone": phrase["tone"],
        "text": phrase["text"],
        "path": str(output_path.resolve()),
        "generation_seconds": round(elapsed, 3),
        "audio_seconds": round(audio_seconds, 3),
        "audio_realtime_factor": round(audio_seconds / elapsed, 3) if elapsed > 0 else None,
    }


def run_worker(args: argparse.Namespace) -> int:
    started_at = utc_now()
    out_dir = args.out_dir.resolve()
    clips_dir = out_dir / "clips"
    clips_dir.mkdir(parents=True, exist_ok=True)

    phrase_bank = load_phrase_bank(args.phrases)
    phrases = phrase_bank["phrases"]

    print("Worker importing and loading model...", flush=True)
    load_started = time.perf_counter()
    model, torch_module, watermarker_patched = load_chatterbox_model(args)
    model_load_seconds = round(time.perf_counter() - load_started, 3)
    print(f"Model loaded in {model_load_seconds:.3f}s", flush=True)

    if args.prepare_only:
        report = {
            "schema_version": 1,
            "status": "prepared",
            "started_at": iso_timestamp(started_at),
            "completed_at": iso_timestamp(utc_now()),
            "engine": args.engine,
            "device": args.device,
            "model_load_seconds": model_load_seconds,
            "generation_total_seconds": 0.0,
            "generated_audio_seconds": 0.0,
            "generation_audio_realtime_factor": None,
            "sample_rate": model.sr,
            "clip_count": 0,
            "clips": [],
            "parameters": generation_parameters_from_args(args),
            "environment": environment_snapshot(),
            "workarounds": {
                "noop_watermarker": watermarker_patched,
            },
            "notes": [
                "Prepare-only mode loaded the model to download and warm local weights, then exited without generating clips."
            ],
        }
        write_json(out_dir / "worker_report.json", report)
        print(f"Worker prepared model cache: {out_dir / 'worker_report.json'}", flush=True)
        return 0

    clips: List[Dict[str, Any]] = []
    generation_started = time.perf_counter()
    for index, phrase in enumerate(phrases, start=1):
        filename = f"{index:02d}_{slugify(phrase['id'])}.wav"
        output_path = clips_dir / filename
        if output_path.exists() and args.skip_existing:
            print(f"[{index}/{len(phrases)}] Skipping existing {output_path.name}", flush=True)
            clips.append(
                {
                    "phrase_id": phrase["id"],
                    "tone": phrase["tone"],
                    "text": phrase["text"],
                    "path": str(output_path.resolve()),
                    "generation_seconds": 0.0,
                    "audio_seconds": None,
                    "audio_realtime_factor": None,
                    "skipped_existing": True,
                }
            )
            continue

        print(f"[{index}/{len(phrases)}] Generating {phrase['id']}", flush=True)
        clip = worker_generate_clip(args, model, torch_module, phrase, output_path)
        clips.append(clip)
        print(
            f"[{index}/{len(phrases)}] Wrote {output_path.name} "
            f"in {clip['generation_seconds']:.3f}s",
            flush=True,
        )

    generation_total_seconds = round(time.perf_counter() - generation_started, 3)
    generated_audio_seconds = round(
        sum(float(clip["audio_seconds"] or 0.0) for clip in clips),
        3,
    )
    realtime_factor = (
        round(generated_audio_seconds / generation_total_seconds, 3)
        if generation_total_seconds > 0
        else None
    )

    report = {
        "schema_version": 1,
        "status": "completed",
        "started_at": iso_timestamp(started_at),
        "completed_at": iso_timestamp(utc_now()),
        "engine": args.engine,
        "device": args.device,
        "model_load_seconds": model_load_seconds,
        "generation_total_seconds": generation_total_seconds,
        "generated_audio_seconds": generated_audio_seconds,
        "generation_audio_realtime_factor": realtime_factor,
        "sample_rate": model.sr,
        "clip_count": len(clips),
        "clips": clips,
        "parameters": generation_parameters_from_args(args),
        "environment": environment_snapshot(),
        "workarounds": {
            "noop_watermarker": watermarker_patched,
        },
    }
    write_json(out_dir / "worker_report.json", report)
    print(f"Worker wrote report: {out_dir / 'worker_report.json'}", flush=True)
    return 0


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate a cached Supervisor voice phrase bank and timing/RAM report.",
    )
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument(
        "--engine",
        choices=ENGINE_CHOICES,
        default="chatterbox-nano",
        help="Voice engine to run. Nano is the V1 candidate; Turbo is only for fallback comparison.",
    )
    parser.add_argument(
        "--reference",
        type=Path,
        help="Path to a 20-30 second voice reference clip you own or have permission to use.",
    )
    parser.add_argument(
        "--allow-built-in-voice",
        dest="allow_builtin_voice",
        action="store_true",
        help="Allow generation without a reference clip if the model checkpoint includes built-in conditionals.",
    )
    parser.add_argument(
        "--phrases",
        type=Path,
        default=DEFAULT_PHRASES,
        help="Phrase bank JSON file.",
    )
    parser.add_argument(
        "--out-dir",
        type=Path,
        help="Output run directory. Defaults to voice_bakeoff/runs/<UTC timestamp>.",
    )
    parser.add_argument(
        "--run-id",
        help="Optional run id used when --out-dir is not supplied.",
    )
    parser.add_argument(
        "--overwrite",
        action="store_true",
        help="Allow writing into a non-empty output directory without deleting existing files.",
    )
    parser.add_argument(
        "--device",
        default="cpu",
        help="Torch device. Use cpu for the actual V1 bakeoff; mps/cuda are useful only for comparison.",
    )
    parser.add_argument(
        "--model-dir",
        type=Path,
        help="Optional local Chatterbox model checkpoint directory, avoiding Hugging Face download.",
    )
    parser.add_argument(
        "--cache-dir",
        type=Path,
        default=DEFAULT_CACHE_DIR,
        help="Directory for Hugging Face and Torch model caches.",
    )
    parser.add_argument(
        "--threads",
        type=int,
        help="Optional torch CPU thread count.",
    )
    parser.add_argument(
        "--seed",
        type=int,
        help="Optional torch random seed.",
    )
    parser.add_argument(
        "--temperature",
        type=float,
        default=0.8,
        help="Sampling temperature.",
    )
    parser.add_argument(
        "--top-p",
        type=float,
        default=0.95,
        help="Top-p sampling value.",
    )
    parser.add_argument(
        "--top-k",
        type=int,
        default=1000,
        help="Top-k sampling value.",
    )
    parser.add_argument(
        "--repetition-penalty",
        type=float,
        default=1.2,
        help="Repetition penalty.",
    )
    parser.add_argument(
        "--exaggeration",
        type=float,
        default=0.0,
        help="Recorded for comparability; current Nano/Turbo generation ignores this value.",
    )
    parser.add_argument(
        "--cfg-weight",
        type=float,
        default=0.0,
        help="Recorded for comparability; current Nano/Turbo generation ignores this value.",
    )
    parser.add_argument(
        "--min-p",
        type=float,
        default=0.0,
        help="Recorded for comparability; current Nano/Turbo generation ignores this value.",
    )
    parser.add_argument(
        "--skip-existing",
        action="store_true",
        help="Skip clips that already exist in the output clips directory.",
    )
    parser.add_argument(
        "--prepare-only",
        action="store_true",
        help="Load the model to download/warm weights, then exit without generating clips.",
    )
    parser.add_argument(
        "--limit",
        type=int,
        help="Generate only the first N phrases. Useful for smoke testing the setup.",
    )
    parser.add_argument(
        "--poll-interval",
        type=float,
        default=0.25,
        help="Parent memory polling interval in seconds.",
    )
    return parser.parse_args(argv)


def main(argv: Optional[Sequence[str]] = None) -> int:
    try:
        args = parse_args(argv)
        validate_common_args(args)
        if args.worker:
            return run_worker(args)
        return run_orchestrator(args)
    except BakeoffError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        if exc.__cause__ is not None:
            print(f"Cause: {exc.__cause__}", file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        print("Interrupted.", file=sys.stderr)
        return 130
    except Exception:
        traceback.print_exc()
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
