#!/usr/bin/env python3
"""Supervisor Chatterbox Nano phrase generator.

CLI one-shot still works. `--serve` keeps the model loaded and reads one JSON
request per stdin line until stdin closes. The desktop app owns the process.
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
from typing import Any, Dict, List, Optional, Sequence, TextIO, Tuple


WORKER_API_VERSION = 1


class WorkerError(Exception):
    """Expected input/setup failure."""


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def slugify(value: str) -> str:
    slug = re.sub(r"[^a-zA-Z0-9]+", "_", value.strip().lower()).strip("_")
    return slug or "phrase"


def same_reference(current: Optional[Path], incoming: Path) -> bool:
    if current is None:
        return False
    try:
        return current.resolve() == incoming.resolve()
    except OSError:
        return current == incoming


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
    return normalize_phrases(data.get("phrases"))


def normalize_phrases(phrases: Any) -> List[Dict[str, str]]:
    if not isinstance(phrases, list) or not phrases:
        raise WorkerError("request must contain a non-empty 'phrases' list")

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


def load_model(args: argparse.Namespace) -> tuple[Any, Any, bool, Dict[str, int]]:
    import_started = time.perf_counter()
    try:
        import torch
        from chatterbox.tts_turbo import ChatterboxTurboTTS
    except Exception as exc:
        raise WorkerError(f"Could not import the Chatterbox Nano runtime: {exc}") from exc
    imports_ms = ms_since(import_started)

    if args.threads is not None:
        torch.set_num_threads(args.threads)
    if args.seed is not None:
        torch.manual_seed(args.seed)

    patched = patch_perth_watermarker()
    load_started = time.perf_counter()
    model = ChatterboxTurboTTS.from_local(str(args.model_dir), device=args.device, nano=True)
    return model, torch, patched, {
        "imports": imports_ms,
        "model_load": ms_since(load_started),
    }


def configure_runtime_environment(cache_dir: Path) -> None:
    cache_dir.mkdir(parents=True, exist_ok=True)
    hf_home = cache_dir / ".hf_cache"
    os.environ.setdefault("HF_HOME", str(hf_home))
    os.environ.setdefault("HF_HUB_CACHE", str(hf_home / "hub"))
    os.environ.setdefault("HUGGINGFACE_HUB_CACHE", str(hf_home / "hub"))
    os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
    os.environ.setdefault("NUMBA_CACHE_DIR", str(cache_dir / ".numba_cache"))


def ms_since(started: float) -> int:
    return max(0, round((time.perf_counter() - started) * 1000))


def prepare_voice(model: Any, reference: Path) -> int:
    started = time.perf_counter()
    model.prepare_conditionals(str(reference))
    return ms_since(started)


def generate_clip(
    args: argparse.Namespace,
    model: Any,
    torch_module: Any,
    phrase: Dict[str, str],
    output_path: Path,
) -> Tuple[Dict[str, Any], int, int]:
    import torchaudio as ta

    synth_started = time.perf_counter()
    wav = model.generate(
        phrase["text"],
        temperature=args.temperature,
        top_p=args.top_p,
        top_k=args.top_k,
        repetition_penalty=args.repetition_penalty,
    )
    synthesis_ms = ms_since(synth_started)
    write_started = time.perf_counter()
    save_wav = wav.detach().cpu() if hasattr(wav, "detach") else torch_module.as_tensor(wav).detach().cpu()
    if len(save_wav.shape) == 1:
        save_wav = save_wav.unsqueeze(0)

    ta.save(str(output_path), save_wav, model.sr, encoding="PCM_S", bits_per_sample=16)
    audio_seconds = float(save_wav.shape[-1]) / float(model.sr)
    return (
        {
            "phrase_id": phrase["id"],
            "tone": phrase["tone"],
            "text": phrase["text"],
            "path": str(output_path.resolve()),
            "generation_seconds": round((synthesis_ms + ms_since(write_started)) / 1000, 3),
            "audio_seconds": round(audio_seconds, 3),
        },
        synthesis_ms,
        ms_since(write_started),
    )


def generate_phrases(
    args: argparse.Namespace,
    model: Any,
    torch_module: Any,
    phrases: List[Dict[str, str]],
    out_dir: Path,
    reference: Path,
    current_reference: Optional[Path],
    overwrite: bool,
) -> Tuple[List[Dict[str, Any]], Path, Dict[str, int], bool]:
    if not reference.exists():
        raise WorkerError(f"Reference audio not found: {reference}")
    if out_dir.exists() and any(out_dir.iterdir()) and not overwrite:
        raise WorkerError(f"Output directory is not empty: {out_dir}")

    clips_dir = out_dir / "clips"
    clips_dir.mkdir(parents=True, exist_ok=True)

    resolved = reference.resolve()
    reused_voice = same_reference(current_reference, resolved)
    condition_ms = 0 if reused_voice else prepare_voice(model, resolved)

    clips: List[Dict[str, Any]] = []
    synthesis_ms = 0
    write_ms = 0
    for index, phrase in enumerate(phrases, start=1):
        output_path = clips_dir / f"{index:02d}_{slugify(phrase['id'])}.wav"
        if output_path.exists() and not overwrite:
            raise WorkerError(f"Clip already exists: {output_path}")
        clip, clip_synth, clip_write = generate_clip(args, model, torch_module, phrase, output_path)
        clips.append(clip)
        synthesis_ms += clip_synth
        write_ms += clip_write

    return clips, resolved, {
        "condition": condition_ms,
        "synthesis": synthesis_ms,
        "wav_write": write_ms,
    }, reused_voice


def write_report(
    out_dir: Path,
    args: argparse.Namespace,
    model: Any,
    watermarker_patched: bool,
    clips: List[Dict[str, Any]],
    phases_ms: Dict[str, int],
    started_at: str,
) -> None:
    report = {
        "schema_version": 1,
        "worker_api": WORKER_API_VERSION,
        "status": "completed",
        "started_at": started_at,
        "completed_at": utc_now(),
        "engine": "chatterbox-nano",
        "device": args.device,
        "model_load_seconds": round(phases_ms.get("model_load", 0) / 1000, 3),
        "generation_total_seconds": round(
            (phases_ms.get("condition", 0) + phases_ms.get("synthesis", 0) + phases_ms.get("wav_write", 0))
            / 1000,
            3,
        ),
        "phases_ms": phases_ms,
        "sample_rate": model.sr,
        "clip_count": len(clips),
        "clips": clips,
        "workarounds": {
            "noop_watermarker": watermarker_patched,
        },
    }
    write_json(out_dir / "worker_report.json", report)


def run(args: argparse.Namespace) -> int:
    started_at = utc_now()
    if not args.model_dir.exists():
        raise WorkerError(f"Model directory not found: {args.model_dir}")
    phrases = load_phrases(args.phrases)
    configure_runtime_environment(args.cache_dir or args.out_dir)

    model, torch_module, watermarker_patched, load_phases = load_model(args)
    clips, _, gen_phases, _ = generate_phrases(
        args,
        model,
        torch_module,
        phrases,
        args.out_dir,
        args.reference,
        None,
        args.overwrite,
    )
    phases_ms = {**load_phases, **gen_phases}
    write_report(args.out_dir, args, model, watermarker_patched, clips, phases_ms, started_at)
    return 0


def reply(stream: TextIO, payload: Dict[str, Any]) -> None:
    stream.write(json.dumps(payload, separators=(",", ":")) + "\n")
    stream.flush()


def serve(args: argparse.Namespace) -> int:
    if not args.model_dir.exists():
        raise WorkerError(f"Model directory not found: {args.model_dir}")
    configure_runtime_environment(args.cache_dir or args.model_dir)

    # Libraries print to stdout. Keep JSON replies on the original pipe.
    replies = sys.stdout
    sys.stdout = sys.stderr

    model = None
    torch_module = None
    watermarker_patched = False
    load_phases = {"imports": 0, "model_load": 0}
    current_reference: Optional[Path] = None

    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
        except json.JSONDecodeError as exc:
            reply(replies, {"ok": False, "id": None, "error": f"invalid json: {exc}"})
            continue
        if not isinstance(request, dict):
            reply(replies, {"ok": False, "id": None, "error": "request must be an object"})
            continue

        request_id = request.get("id")
        op = str(request.get("op") or "generate")
        if op == "shutdown":
            reply(replies, {"ok": True, "id": request_id, "op": "shutdown"})
            return 0
        if op != "generate":
            reply(replies, {"ok": False, "id": request_id, "error": f"unknown op: {op}"})
            continue

        try:
            started_at = utc_now()
            phrases = normalize_phrases(request.get("phrases"))
            reference = Path(str(request.get("reference") or "")).expanduser()
            out_dir = Path(str(request.get("out_dir") or "")).expanduser()
            if not str(reference) or not str(out_dir):
                raise WorkerError("generate requires reference and out_dir")
            overwrite = bool(request.get("overwrite", True))

            reused_model = model is not None
            if model is None:
                model, torch_module, watermarker_patched, load_phases = load_model(args)

            clips, current_reference, gen_phases, reused_voice = generate_phrases(
                args,
                model,
                torch_module,
                phrases,
                out_dir,
                reference.resolve(),
                current_reference,
                overwrite,
            )
            phases_ms = {
                "imports": 0 if reused_model else load_phases["imports"],
                "model_load": 0 if reused_model else load_phases["model_load"],
                **gen_phases,
            }
            write_report(out_dir, args, model, watermarker_patched, clips, phases_ms, started_at)
            reply(
                replies,
                {
                    "ok": True,
                    "id": request_id,
                    "loaded": True,
                    "reused_model": reused_model,
                    "reused_voice": reused_voice,
                    "phases_ms": phases_ms,
                },
            )
        except WorkerError as exc:
            reply(
                replies,
                {
                    "ok": False,
                    "id": request_id,
                    "error": str(exc),
                    "loaded": model is not None,
                },
            )
        except Exception as exc:
            traceback.print_exc(file=sys.stderr)
            reply(
                replies,
                {
                    "ok": False,
                    "id": request_id,
                    "error": str(exc),
                    "loaded": model is not None,
                },
            )
            return 1
    return 0


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate Supervisor voice clips with Chatterbox Nano.")
    parser.add_argument("--serve", action="store_true")
    parser.add_argument("--model-dir", type=Path, required=True)
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--phrases", type=Path)
    parser.add_argument("--out-dir", type=Path)
    parser.add_argument("--cache-dir", type=Path)
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
    if args.cache_dir:
        args.cache_dir = args.cache_dir.expanduser().resolve()
    if args.threads is not None and args.threads <= 0:
        raise WorkerError("--threads must be greater than zero")
    if args.serve:
        return args
    if not args.reference or not args.phrases or not args.out_dir:
        raise WorkerError("--reference, --phrases, and --out-dir are required unless --serve")
    args.reference = args.reference.expanduser().resolve()
    args.phrases = args.phrases.expanduser().resolve()
    args.out_dir = args.out_dir.expanduser().resolve()
    return args


def self_check() -> int:
    assert same_reference(None, Path("a.wav")) is False
    assert same_reference(Path("a.wav"), Path("a.wav")) is True
    phrases = normalize_phrases(
        [{"id": "hi", "tone": "gentle", "text": "Back to the task."}]
    )
    assert phrases[0]["id"] == "hi"
    try:
        normalize_phrases([])
        raise AssertionError("empty phrases should fail")
    except WorkerError:
        pass
    request = json.loads(
        '{"op":"generate","id":"1","reference":"ref.wav","out_dir":"out","phrases":[{"id":"a","tone":"gentle","text":"Hi."}]}'
    )
    assert request["op"] == "generate" and len(normalize_phrases(request["phrases"])) == 1
    print("self-check ok")
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    multiprocessing.freeze_support()
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv == ["--self-check"]:
        return self_check()
    try:
        args = parse_args(argv)
        return serve(args) if args.serve else run(args)
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
