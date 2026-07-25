#!/usr/bin/env python3
"""Translate the model-neutral prompt pilot into a production batch plan."""
from __future__ import annotations

import argparse
import datetime as dt
import json
from pathlib import Path
from typing import Any

KEYS = ("D minor", "A minor", "C major", "E minor")
PARAMETERS = (
    ("batch_size", "1"),
    ("cfg_interval_end", "1.0"),
    ("cfg_interval_start", "0.0"),
    ("guidance_scale", "7.0"),
    ("keyscale", None),
    ("lm_cfg_scale", "2.0"),
    ("lm_temperature", "0.8"),
    ("lm_top_k", "0"),
    ("lm_top_p", "0.9"),
    ("thinking", "false"),
    ("timesignature", "4/4"),
    ("use_adg", "false"),
)


def read_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        value = json.load(handle)
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def build_plan(protocol: dict[str, Any], base: dict[str, Any]) -> dict[str, Any]:
    fixed = protocol["fixed_parameters"]
    conditions = protocol["conditions"]
    seeds = fixed["seeds"]
    if protocol.get("activity") != "motivation":
        raise ValueError("this pilot builder only supports the Motivation pilot")
    if fixed != {
        "duration_seconds": 180,
        "sample_rate_hz": 48000,
        "codec": "flac",
        "tempo_bpm": 108,
        "meter": "4/4",
        "seeds": seeds,
        "instrumental_only": True,
        "same_model_for_all_conditions": True,
    }:
        raise ValueError("the pilot fixed parameters do not match the production contract")
    if len(conditions) != 3 or len(seeds) != 4:
        raise ValueError("the pilot must contain exactly 3 conditions and 4 seeds")
    if len({condition["id"] for condition in conditions}) != len(conditions):
        raise ValueError("pilot condition IDs must be unique")

    candidates: list[dict[str, Any]] = []
    for condition_index, condition in enumerate(conditions):
        for index, seed_family in enumerate(seeds):
            key = KEYS[index]
            # candidate-ledger deliberately rejects duplicate seeds within one
            # batch. Keep the four experimental seed families while deriving a
            # stable, unique production seed for each condition.
            seed = seed_family + condition_index * 1000
            candidate_id = f"motivation-{condition['id']}-{seed_family}"
            positive = condition["positive_prompt"].strip()
            if not positive.startswith("[Instrumental]"):
                positive = f"[Instrumental] {positive}"
            candidates.append(
                {
                    "id": candidate_id,
                    "seed": seed,
                    "activity": "motivation",
                    "genre_ids": ["soft-electronic"],
                    "mood_ids": ["energized", "optimistic", "steady"],
                    "duration_seconds": 180,
                    "bpm": 108,
                    "contains_lyrics": False,
                    "contains_speech": False,
                    "prompts": {
                        "positive": positive,
                        "negative": condition["negative_prompt"],
                    },
                    "inference": {
                        "codec": "flac",
                        "sample_rate_hz": 48000,
                        "steps": 8,
                        "shift": 3,
                        "solver": "ode",
                        "use_random_seed": False,
                        "parameters": [
                            {"name": name, "value": value if value is not None else key}
                            for name, value in PARAMETERS
                        ],
                    },
                }
            )

    base_batch = base["batch"]
    return {
        "schema": "adhd-music.candidate-ledger.planned",
        "schema_version": 1,
        "batch": {
            "id": "motivation-prompt-pilot-v1",
            "created_at": "2026-07-22T00:00:00Z",
            "notes": (
                "Twelve quarantined Motivation candidates for prompt-structure "
                "research: three prompt conditions and four fixed seeds. "
                "None is approved or published content."
            ),
            "generator_pin": base_batch["generator_pin"],
            "terms_evidence": base_batch["terms_evidence"],
        },
        "taxonomy": {
            "activities": ["motivation"],
            "genres": [{"id": "soft-electronic", "label": "Soft Electronic"}],
            "moods": [
                {"id": "energized", "label": "Energized"},
                {"id": "optimistic", "label": "Optimistic"},
                {"id": "steady", "label": "Steady"},
            ],
        },
        "candidates": candidates,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--protocol", type=Path, required=True)
    parser.add_argument("--base-plan", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    if args.output.exists():
        raise SystemExit(f"refusing to overwrite existing output: {args.output}")
    plan = build_plan(read_json(args.protocol), read_json(args.base_plan))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x", encoding="utf-8") as handle:
        json.dump(plan, handle, indent=2)
        handle.write("\n")
    print(f"wrote {len(plan['candidates'])} candidates to {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
