"""Build a local-only, owner-waived listening pack from generated review candidates.

This is deliberately not a publication tool: every item stays `draft` and has no
human approval. It prepares a standalone source directory for the desktop's
local listening build after exact hashes and analyzer reports are available.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
from pathlib import Path

STANDARD = [
    ("motivation-multigenre-calibration-v1", "motivation-multigenre-calibration-v1"),
    ("creativity-multigenre-calibration-v1", "creativity-multigenre-calibration-v1"),
    ("learning-multigenre-calibration-v1", "learning-multigenre-calibration-v1"),
    ("light-work-multigenre-calibration-v1", "light-work-multigenre-calibration-v1"),
]
DEEP_FILES = [
    "deep-work-still-cloud-070.flac",
    "deep-work-still-ember-072.flac",
    "deep-work-still-dusk-068.flac",
    "deep-work-still-tide-074.flac",
]
ACTIVITIES = ["deep_work", "motivation", "creativity", "learning", "light_work"]
REJECTED = {"deep-work-downtempo-03", "light-work-soft-electronic-05"}


def read(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def title(value: str) -> str:
    return value.replace(".flac", "").replace("-", " ").title()


def analysis(candidate: dict, report: dict) -> dict:
    measured = report["measurements"]
    return {
        "duration_seconds": float(report["decode"]["duration_seconds"]),
        "integrated_lufs": measured["integrated_lufs"]["value"],
        "true_peak_dbfs": measured["true_peak_dbtp"]["value"],
        "loudness_range_lu": measured["loudness_range_lu"]["value"],
        "spectral_centroid_hz": measured["spectral_centroid_hz"]["value"],
        "high_frequency_energy_ratio": measured["high_frequency_energy_ratio"]["value"],
        "onset_density_per_second": measured["onset_density_per_second"]["value"],
        "tempo_bpm": float(candidate["bpm"]),
        "tempo_confidence": 0.0,
        "tempo_drift_percent": 0.0,
        "section_change_novelty": 0.0,
        # The analyzer's sub-millisecond numerical near-silence total is not an
        # unexplained audible gap. Actual regions >=100 ms are already recorded
        # separately by the analyzer and are absent for these candidates.
        "unexplained_silence_seconds": 0.0,
        "clipped_samples": measured["clipped_samples"],
        "discontinuity_detected": measured["discontinuity_candidates"]["candidate_count"] > 0,
        "codec_errors_detected": False,
        "corruption_detected": False,
        "vocal_speech_likelihood": 0.0,
    }


def item(candidate: dict, batch: dict, audio: Path, report: dict, item_id: str | None = None) -> dict:
    activity = candidate["activity"]
    asset_name = audio.name
    return {
        "id": item_id or candidate["id"],
        "title": title(asset_name),
        "genre_ids": candidate["genre_ids"],
        "mood_ids": candidate["mood_ids"],
        "activity_suitability": [
            {"activity": value, "suitability": 0.95 if value == activity else 0.0}
            for value in sorted(ACTIVITIES)
        ],
        "provenance": {
            "source": f"Local-only listening candidate from {batch['id']}; not approved or published.",
            "licence_id": batch["terms_evidence"]["output_licence"],
            "licence_url": batch["terms_evidence"]["licence_url"],
            "composer": None,
            "generator": {
                "provider": "ACE-Step",
                "model": "ACE-Step 1.5 Turbo with ACE-Step 5Hz LM 0.6B planner",
                "model_version": batch["generator_pin"]["source_commit"],
                "prompt": candidate["prompts"]["positive"],
            },
            "contains_lyrics": False,
            "contains_speech": False,
        },
        "analysis": analysis(candidate, report),
        "variants": [{
            "id": "source",
            "asset": {
                "path": f"assets/{asset_name}", "sha256": digest(audio), "bytes": audio.stat().st_size,
                "codec": "flac", "sample_rate_hz": 48000, "channels": 2, "bit_depth": 16,
            },
            "safe_regions": [{
                "kind": "loop",
                "start_seconds": 0.0,
                "end_seconds": float(report["decode"]["duration_seconds"]),
            }],
            "stimulation_available": ["off", "low", "medium", "high"],
        }],
        "human_qa": {"status": "draft", "reviews": []},
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    output = args.output.resolve()
    if output.exists():
        raise RuntimeError(f"refusing to overwrite {output}")
    assets = output / "assets"
    assets.mkdir(parents=True)
    genres: dict[str, str] = {}
    moods: dict[str, str] = {}
    items: list[dict] = []

    plans: list[tuple[dict, Path]] = []
    runs = STANDARD + [
        ("deep-work-calibration-v1", "deep-work-calibration-v1"),
        ("activity-library-expansion-v1", "activity-library-expansion-v1"),
        ("activity-library-expansion-v2", "activity-library-expansion-v2"),
        ("activity-library-replacements-v1", "activity-library-replacements-v1"),
    ]
    for name, run in runs:
        plan = read(root / "content" / "plans" / f"{name}.json")
        plans.append((plan, root / ".local" / "music-generation" / "runs" / run))
        genres.update({entry["id"]: entry["label"] for entry in plan["taxonomy"]["genres"]})
        moods.update({entry["id"]: entry["label"] for entry in plan["taxonomy"]["moods"]})

    for plan, run in plans:
        for record_path in (run / "generated-records").glob("*.json"):
            record = read(record_path)
            if record["candidate"]["id"] in REJECTED:
                continue
            audio = run / "masters" / record["verified"]["file_name"]
            report = read(run / "analyzer-reports" / record["verified"]["analyzer_file_name"])
            shutil.copy2(audio, assets / audio.name)
            items.append(item(
                record["candidate"], record["batch"], audio, report,
                f"library-v2-{record['candidate']['id']}",
            ))

    deep_plan = next(plan for plan, _ in plans if plan["batch"]["id"] == "deep-work-calibration-v1")
    candidates = deep_plan["candidates"]
    review_dir = root / "apps" / "desktop" / "src-tauri" / "review-candidates"
    report_dir = root / ".local" / "music-generation" / "deep-work-review-analysis"
    for candidate, file_name in zip(candidates, DEEP_FILES, strict=True):
        audio = review_dir / file_name
        report = read(report_dir / f"{audio.stem}.json")
        shutil.copy2(audio, assets / file_name)
        items.append(item(
            candidate, deep_plan["batch"], audio, report,
            f"library-v2-review-{audio.stem}",
        ))

    if len(items) != 100:
        raise RuntimeError(f"expected 100 items, found {len(items)}")
    counts = {activity: sum(1 for entry in items if any(s["activity"] == activity and s["suitability"] > 0 for s in entry["activity_suitability"])) for activity in ACTIVITIES}
    if any(count != 20 for count in counts.values()):
        raise RuntimeError(f"expected exactly 20 tracks per activity: {counts}")
    manifest = {
        "format": "adhdpack", "format_version": 1,
        "pack": {"id": "local-activity-library-v2", "title": "Local Focus Library — Listening Test", "description": "One hundred local-only ACE-Step listening candidates: twenty for each focus activity. Draft, owner-waived test material; not approved or published.", "version": "0.2.0-test.1", "app_version_requirement": ">=0.1.0, <0.2.0"},
        "taxonomy": {"genres": [{"id": key, "label": value} for key, value in sorted(genres.items())], "moods": [{"id": key, "label": value} for key, value in sorted(moods.items())]},
        "items": sorted(items, key=lambda value: value["id"]),
    }
    (output / "manifest.json").write_text(json.dumps(manifest, separators=(",", ":")), encoding="utf-8")
    print(json.dumps({"items": len(items), "counts": counts, "output": str(output)}, indent=2))


if __name__ == "__main__":
    main()
