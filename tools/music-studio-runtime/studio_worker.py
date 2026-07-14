#!/usr/bin/env python3
"""Hermetic one-shot runner for the packaged ACE-Step runtime."""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

MAX_PROMPT_CHARS = 4096
MAX_REQUEST_BYTES = 16 * 1024
GENERATION_TIMEOUT_SECONDS = 3600


def absolute_path(value: str) -> Path:
    path = Path(value).resolve(strict=False)
    if not path.is_absolute() or path == Path(path.anchor):
        raise argparse.ArgumentTypeError("an absolute non-root path is required")
    return path


def emit(ok: bool, code: str | None = None, output: str | None = None) -> int:
    value: dict[str, object] = {"ok": ok}
    if code is not None:
        value["code"] = code
    if output is not None:
        value["output"] = output
    # All values are fixed and this result is always far below the desktop cap.
    print(json.dumps(value, separators=(",", ":")))
    return 0 if ok else 1


def regular_file(path: Path) -> bool:
    try:
        return path.is_file() and not path.is_symlink()
    except OSError:
        return False


def generation_environment(root: Path, cache: Path) -> dict[str, str]:
    modules = cache / "modules"
    modules.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment.update({
        "ACESTEP_CHECKPOINTS_DIR": str(root / "snapshots" / "turbo-vae"),
        "HF_HUB_OFFLINE": "1",
        "TRANSFORMERS_OFFLINE": "1",
        "HF_HOME": str(cache),
        "HF_MODULES_CACHE": str(modules),
        "PYTHONUTF8": "1",
        "PYTHONIOENCODING": "utf-8",
    })
    return environment


def toml_value(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return str(value)
    return json.dumps(value)


def flat_config(root: Path, output_dir: Path, request: dict[str, object]) -> str:
    values = {
        "project_root": str(root / "ace-step-source"),
        "checkpoint_dir": str(root / "snapshots" / "turbo-vae"),
        "lm_model_path": str(root / "snapshots" / "planner-0.6b"),
        "backend": "pt",
        "device": "cuda",
        "save_dir": str(output_dir),
        "audio_format": "flac",
        "caption": request["positive_prompt"],
        "lyrics": "[Instrumental]",
        "instrumental": True,
        "duration": request["duration_seconds"],
        "inference_steps": 8,
        "seed": request["seed"],
        "shift": 3,
        "infer_method": "ode",
        "use_random_seed": False,
        "lm_negative_prompt": request["negative_prompt"],
        "batch_size": 1,
        # Fixed, previously validated ACE-Step inference parameters.
        "cfg_interval_end": 1.0,
        "cfg_interval_start": 0.0,
        "guidance_scale": 7.0,
        "lm_cfg_scale": 2.0,
        "lm_temperature": 0.8,
        "lm_top_k": 0,
        "lm_top_p": 0.9,
        "thinking": False,
        "timesignature": "4/4",
        "use_adg": False,
    }
    return "".join(f"{key} = {toml_value(value)}\n" for key, value in values.items())


def read_request(path: Path) -> dict[str, object] | None:
    if not regular_file(path):
        return None
    try:
        raw = path.read_bytes()
        if len(raw) > MAX_REQUEST_BYTES:
            return None
        value = json.loads(raw)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        return None
    if not isinstance(value, dict) or set(value) != {
        "positive_prompt", "negative_prompt", "duration_seconds", "seed"
    }:
        return None
    positive, negative = value["positive_prompt"], value["negative_prompt"]
    duration, seed = value["duration_seconds"], value["seed"]
    if (
        not isinstance(positive, str) or not positive or len(positive) > MAX_PROMPT_CHARS
        or not isinstance(negative, str) or len(negative) > MAX_PROMPT_CHARS
        or duration not in (90, 180) or isinstance(duration, bool)
        or not isinstance(seed, int) or isinstance(seed, bool) or not 0 <= seed <= 2**64 - 1
    ):
        return None
    return value


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--request", required=True, type=absolute_path)
    parser.add_argument("--output-dir", required=True, type=absolute_path)
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parent
    request = read_request(args.request)
    cli = root / "ace-step-source" / "cli.py"
    checkpoint = root / "snapshots" / "turbo-vae"
    planner = root / "snapshots" / "planner-0.6b"
    if request is None:
        return emit(False, "invalid_request")
    if args.output_dir.exists() or not all((regular_file(cli), checkpoint.is_dir(), planner.is_dir())):
        return emit(False, "invalid_paths" if args.output_dir.exists() else "runtime_invalid")
    config_path: Path | None = None
    try:
        args.output_dir.mkdir(mode=0o700)
        fd, name = tempfile.mkstemp(prefix="studio-owned-", suffix=".toml", dir=args.output_dir)
        config_path = Path(name)
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(flat_config(root, args.output_dir, request))
        with tempfile.TemporaryFile() as diagnostic, \
             tempfile.TemporaryDirectory(prefix="adhd-music-studio-cache-") as cache_name:
            completed = subprocess.run(
                [sys.executable, str(cli), "--backend", "pt", "--config", str(config_path)],
                cwd=root,
                check=False,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=diagnostic,
                env=generation_environment(root, Path(cache_name)),
                timeout=GENERATION_TIMEOUT_SECONDS,
            )
            diagnostic.seek(0)
            bounded_diagnostic = diagnostic.read(64 * 1024).lower()
        config_path.unlink()
        config_path = None
        if completed.returncode != 0:
            if b"out of memory" in bounded_diagnostic or b"cuda oom" in bounded_diagnostic:
                return emit(False, "gpu_oom")
            return emit(False, "unexpected_exit")
        outputs = [
            item for item in args.output_dir.iterdir()
            if item.suffix.lower() == ".flac" and regular_file(item)
        ]
        if len(outputs) != 1:
            return emit(False, "invalid_output")
        target = args.output_dir / "draft.flac"
        if outputs[0] != target:
            os.replace(outputs[0], target)
        return emit(True, output="draft.flac")
    except subprocess.TimeoutExpired:
        return emit(False, "timeout")
    except OSError:
        return emit(False, "generation_failed")
    finally:
        if config_path is not None:
            config_path.unlink(missing_ok=True)


if __name__ == "__main__":
    raise SystemExit(main())
