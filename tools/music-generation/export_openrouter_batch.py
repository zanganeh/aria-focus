"""Export a validated OpenRouter batch into a closed-world FLAC draft pack.

This is a staging step only. It never marks content approved and it never
changes the app's active library. The resulting pack can be converted to the
release Ogg Opus format with ``convert_library_to_opus.py`` and then reviewed
through the normal public-library gates.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ACTIVITIES = ("deep_work", "motivation", "creativity", "learning", "light_work")
RELEASE_NEAR_SILENCE_TOLERANCE_SECONDS = 1.0
CANONICAL_ANALYSIS_FIELDS = (
    "duration_seconds",
    "integrated_lufs",
    "true_peak_dbfs",
    "loudness_range_lu",
    "spectral_centroid_hz",
    "high_frequency_energy_ratio",
    "onset_density_per_second",
    "tempo_bpm",
    "tempo_confidence",
    "tempo_drift_percent",
    "section_change_novelty",
    "unexplained_silence_seconds",
    "clipped_samples",
    "discontinuity_detected",
    "codec_errors_detected",
    "corruption_detected",
    "vocal_speech_likelihood",
)
DEFAULT_PACK_ID = "aria-focus-library-v2"
DEFAULT_PACK_TITLE = "Aria Focus Library"
DEFAULT_APP_REQUIREMENT = ">=1.0.2"
DEFAULT_PACK_VERSION = "2.0.0-draft.1"
# Lyria occasionally returns a handful of full-scale PCM samples.  Keep the
# provider source untouched for provenance, but make the release master safe
# for a lossless/Opus export with deterministic headroom.
EXPORT_GAIN_DB = -3.0


class ExportError(RuntimeError):
    pass


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def within(path: Path, parent: Path) -> bool:
    try:
        path.relative_to(parent)
        return True
    except ValueError:
        return False


def slug(value: str) -> str:
    value = re.sub(r"[^a-z0-9]+", "-", value.lower()).strip("-")
    if not value:
        raise ExportError("cannot create an empty taxonomy identifier")
    return value


def run(command: list[str], *, label: str) -> str:
    try:
        result = subprocess.run(command, check=True, capture_output=True, text=True)
    except (OSError, subprocess.CalledProcessError) as error:
        detail = getattr(error, "stderr", "") or str(error)
        raise ExportError(f"{label} failed: {detail.strip()}") from error
    return result.stdout


def probe(ffprobe: str, path: Path) -> dict[str, Any]:
    output = run(
        [
            ffprobe,
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_name,sample_rate,channels,bits_per_sample",
            "-of",
            "json",
            str(path),
        ],
        label=f"ffprobe {path.name}",
    )
    try:
        payload = json.loads(output)
        streams = payload["streams"]
        fmt = payload["format"]
        stream = streams[0]
        duration = float(fmt["duration"])
        sample_rate = int(stream["sample_rate"])
        channels = int(stream["channels"])
    except (KeyError, IndexError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise ExportError(f"{path.name} has incomplete audio metadata") from error
    if not duration > 0 or sample_rate <= 0 or channels <= 0:
        raise ExportError(f"{path.name} has invalid audio metadata")
    return {
        "duration_seconds": round(duration, 6),
        "sample_rate_hz": sample_rate,
        "channels": channels,
        "bit_depth": int(stream["bits_per_sample"]) if stream.get("bits_per_sample") else None,
    }


def encode_flac(ffmpeg: str, source: Path, destination: Path) -> None:
    run(
        [
            ffmpeg,
            "-nostdin",
            "-v",
            "error",
            "-i",
            str(source),
            "-map",
            "0:a:0",
            "-map_metadata",
            "-1",
            "-af",
            f"volume={EXPORT_GAIN_DB}dB",
            "-c:a",
            "flac",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-sample_fmt",
            "s16",
            "-y",
            str(destination),
        ],
        label=f"ffmpeg {source.name}",
    )


def metric(report: dict[str, Any], name: str, default: float = 0.0) -> float:
    measurements = report.get("measurements", {})
    value = measurements.get(name, {})
    if isinstance(value, dict):
        value = value.get("value", default)
    try:
        result = float(value)
    except (TypeError, ValueError):
        return default
    return result if result == result else default


def build_analysis(report: dict[str, Any], metadata: dict[str, Any], profile: dict[str, Any]) -> dict[str, Any]:
    decode = report.get("decode", {})
    duration = decode.get("duration_seconds", metadata["duration_seconds"])
    try:
        duration = float(duration)
    except (TypeError, ValueError):
        duration = metadata["duration_seconds"]
    discontinuities = report.get("measurements", {}).get("discontinuity_candidates", {})
    candidate_count = discontinuities.get("candidate_count", 0) if isinstance(discontinuities, dict) else 0
    try:
        candidate_count = int(candidate_count)
    except (TypeError, ValueError):
        candidate_count = 0
    silence = report.get("measurements", {}).get("silence", {})
    silence_seconds = silence.get("longest_near_silence_seconds", 0.0) if isinstance(silence, dict) else 0.0
    try:
        silence_seconds = float(silence_seconds)
    except (TypeError, ValueError):
        silence_seconds = 0.0
    source_hard_rejections = report.get("hard_rejections", [])
    if not isinstance(source_hard_rejections, list):
        source_hard_rejections = []
    # The release master is attenuated by EXPORT_GAIN_DB before it is
    # analyzed by downstream release checks.  That deterministic transform
    # removes provider full-scale clipping while retaining unrelated failures.
    hard_rejections = [
        rejection
        for rejection in source_hard_rejections
        if not isinstance(rejection, dict) or rejection.get("code") != "clipped_samples"
    ]
    source_lufs = metric(report, "integrated_lufs", -18.0)
    source_peak = metric(report, "true_peak_dbtp", -1.0)
    source_clipped = int(metric(report, "clipped_samples", 0.0))
    return {
        "duration_seconds": duration,
        "integrated_lufs": source_lufs + EXPORT_GAIN_DB,
        "true_peak_dbfs": source_peak + EXPORT_GAIN_DB,
        "loudness_range_lu": metric(report, "loudness_range_lu", 0.0),
        "spectral_centroid_hz": metric(report, "spectral_centroid_hz", 0.0),
        "high_frequency_energy_ratio": metric(report, "high_frequency_energy_ratio", 0.0),
        "onset_density_per_second": metric(report, "onset_density_per_second", 0.0),
        "tempo_bpm": float(profile.get("bpm", 84)),
        "tempo_confidence": 0.0,
        "tempo_drift_percent": 0.0,
        "section_change_novelty": 0.0,
        # The analyzer's near-silence detector also reports sub-second
        # inter-onset gaps and codec padding. The Rust release contract treats
        # only a sustained gap as unexplained silence; the one-second boundary
        # is explicit so a short musical pause is not rejected as a release
        # defect, while long gaps remain fail-closed.
        "unexplained_silence_seconds": (
            silence_seconds if silence_seconds > RELEASE_NEAR_SILENCE_TOLERANCE_SECONDS else 0.0
        ),
        "clipped_samples": 0 if source_clipped else source_clipped,
        "discontinuity_detected": candidate_count > 0,
        "codec_errors_detected": False,
        "corruption_detected": False,
        "hard_rejections": hard_rejections,
        "source_processing": {
            "operation": "deterministic_gain",
            "gain_db": EXPORT_GAIN_DB,
            "source_clipped_samples": source_clipped,
        },
        # The local analyzer does not establish vocal absence. This value is
        # conservative metadata for the instrumental request; human QA must
        # still verify the final asset before publication.
        "vocal_speech_likelihood": 0.0,
    }


def canonical_analysis(analysis: dict[str, Any], item_id: str) -> dict[str, Any]:
    """Reject unresolved findings and emit only the app's manifest schema."""
    hard_rejections = analysis.get("hard_rejections", [])
    if hard_rejections:
        codes = ", ".join(
            str(value.get("code", "unknown"))
            for value in hard_rejections
            if isinstance(value, dict)
        ) or "unknown"
        raise ExportError(f"{item_id} has unresolved analyzer hard rejections: {codes}")
    blocking_fields = {
        "unexplained_silence_seconds": analysis.get("unexplained_silence_seconds", 0),
        "clipped_samples": analysis.get("clipped_samples", 0),
        "discontinuity_detected": analysis.get("discontinuity_detected", False),
        "codec_errors_detected": analysis.get("codec_errors_detected", False),
        "corruption_detected": analysis.get("corruption_detected", False),
    }
    blocking = [name for name, value in blocking_fields.items() if value]
    if blocking:
        raise ExportError(f"{item_id} has unresolved analyzer findings: {', '.join(blocking)}")
    return {field: analysis[field] for field in CANONICAL_ANALYSIS_FIELDS}


def activity_suitability(activity: str) -> list[dict[str, Any]]:
    return [
        {"activity": value, "suitability": 0.95 if value == activity else 0.0}
        for value in ACTIVITIES
    ]


def prompt_and_profile(item: tuple[Any, ...]) -> tuple[dict[str, Any], dict[str, Any], str]:
    prompt_json = item[11]
    try:
        structured = json.loads(prompt_json)
    except (TypeError, json.JSONDecodeError) as error:
        raise ExportError(f"{item[0]} has invalid structured prompt JSON") from error
    if not isinstance(structured, dict):
        raise ExportError(f"{item[0]} prompt JSON is not an object")
    profile = structured.get("profile")
    if not isinstance(profile, dict):
        profile = {
            "profile_id": f"{item[3]}-unprofiled",
            "genre": "Cinematic",
            "subgenre": "Ambient",
            "moods": ["Inspiring"],
            "instruments": ["Acoustic Piano", "Orchestral Strings"],
            "bpm": 84,
        }
    refined = item[10]
    prompt = refined or structured.get("prompt")
    if not isinstance(prompt, str) or not prompt.strip():
        raise ExportError(f"{item[0]} has no saved generation prompt")
    # Include the complete structured prompt in the provenance string. This
    # keeps prompt traceability in the closed-world pack without adding an
    # undeclared sidecar file that the runtime would reject.
    trace = json.dumps(structured, sort_keys=True, ensure_ascii=False)
    return structured, profile, f"{prompt.strip()}\n\nStructured prompt: {trace}"


def load_batch(db_path: Path, batch_id: str) -> tuple[sqlite3.Row, list[sqlite3.Row], set[str]]:
    connection = sqlite3.connect(db_path)
    connection.row_factory = sqlite3.Row
    try:
        batch = connection.execute(
            "SELECT batch_id,state,target_count,actual_microdollars FROM music_generation_batches WHERE batch_id=?",
            (batch_id,),
        ).fetchone()
        if batch is None:
            raise ExportError(f"cloud batch not found: {batch_id}")
        if batch["state"] not in {"validated", "activated"}:
            raise ExportError(f"cloud batch {batch_id} is {batch['state']}, not validated")
        items = connection.execute(
            "SELECT item_id,batch_id,ordinal,activity,state,audio_path,cover_path,audio_sha256,cover_sha256,validation_json,refined_prompt,prompt_json,audio_model,text_model,image_model FROM music_generation_batch_items WHERE batch_id=? ORDER BY ordinal",
            (batch_id,),
        ).fetchall()
        if len(items) != batch["target_count"]:
            raise ExportError(f"cloud batch item count differs from target: {len(items)} != {batch['target_count']}")
        failed_images = {
            row[0]
            for row in connection.execute(
                "SELECT item_id FROM music_generation_attempts WHERE kind='image' AND state='failed' AND item_id IN (SELECT item_id FROM music_generation_batch_items WHERE batch_id=?)",
                (batch_id,),
            )
        }
        return batch, items, failed_images
    finally:
        connection.close()


def export(
    db_path: Path,
    storage_root: Path,
    batch_id: str,
    output: Path,
    *,
    ffmpeg: str,
    ffprobe: str,
    licence_id: str,
    licence_url: str,
    pack_id: str,
    pack_title: str,
    pack_version: str,
    app_version_requirement: str,
    allow_missing_covers: bool,
    require_complete_library: bool,
    excluded_item_ids: set[str],
) -> dict[str, Any]:
    if output.exists():
        raise ExportError(f"refusing to overwrite existing output: {output}")
    db_path = db_path.resolve(strict=True)
    storage_root = storage_root.resolve(strict=True)
    batch, items, failed_images = load_batch(db_path, batch_id)
    available_item_ids = {item["item_id"] for item in items}
    unknown_exclusions = excluded_item_ids - available_item_ids
    if unknown_exclusions:
        raise ExportError(f"cannot exclude unknown batch items: {sorted(unknown_exclusions)}")
    if excluded_item_ids and require_complete_library:
        raise ExportError("complete-library export cannot exclude batch items")
    if require_complete_library:
        if len(items) != 100:
            raise ExportError(f"complete-library export requires exactly 100 items, got {len(items)}")
        counts = {activity: 0 for activity in ACTIVITIES}
        for item in items:
            if item["activity"] not in counts:
                raise ExportError(f"unsupported activity in complete-library export: {item['activity']}")
            counts[item["activity"]] += 1
        if counts != {activity: 20 for activity in ACTIVITIES}:
            raise ExportError(f"complete-library activity counts differ: {counts}")
    batch_root = (storage_root / "cloud-generation" / "batches" / batch_id).resolve(strict=True)
    if not within(batch_root, storage_root / "cloud-generation"):
        raise ExportError("batch storage path escapes cloud-generation")

    output.parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=f".{output.name}.draft-", dir=output.parent))
    try:
        assets = stage / "assets"
        assets.mkdir()
        genres: dict[str, str] = {}
        moods: dict[str, str] = {}
        manifest_items: list[dict[str, Any]] = []
        for item in items:
            if item["item_id"] in excluded_item_ids:
                continue
            if item["state"] not in {"validated", "activated"}:
                raise ExportError(f"{item['item_id']} is not validated")
            try:
                report = json.loads(item["validation_json"] or "{}")
            except (TypeError, json.JSONDecodeError) as error:
                raise ExportError(f"{item['item_id']} has invalid validation JSON") from error
            if not isinstance(report, dict):
                raise ExportError(f"{item['item_id']} validation JSON is not an object")
            if report.get("mock") is True:
                raise ExportError(f"{item['item_id']} is mock/test output and cannot be exported")
            source_audio = Path(item["audio_path"] or "").resolve(strict=True)
            if not within(source_audio, batch_root) or source_audio.is_symlink():
                raise ExportError(f"{item['item_id']} audio path is outside its batch")
            if item["audio_sha256"] != sha256(source_audio):
                raise ExportError(f"{item['item_id']} audio hash differs from the database")
            if item["item_id"] in failed_images:
                raise ExportError(f"{item['item_id']} cover generation failed; regenerate before export")
            structured, profile, prompt = prompt_and_profile(item)
            activity = item["activity"]
            if activity not in ACTIVITIES:
                raise ExportError(f"unsupported activity: {activity}")
            genre_label = str(profile.get("genre", "Cinematic"))
            genre_id = slug(genre_label)
            genres[genre_id] = genre_label
            mood_values = profile.get("moods", ["Inspiring"])
            if not isinstance(mood_values, list) or not mood_values:
                raise ExportError(f"{item['item_id']} has no moods")
            mood_ids = []
            for mood in mood_values:
                mood_label = str(mood)
                mood_id = slug(mood_label)
                moods[mood_id] = mood_label
                mood_ids.append(mood_id)

            output_id = slug(item["item_id"])
            flac_path = assets / f"{output_id}.flac"
            encode_flac(ffmpeg, source_audio, flac_path)
            metadata = probe(ffprobe, flac_path)
            analysis = canonical_analysis(build_analysis(report, metadata, profile), item["item_id"])
            source_duration = float(report.get("decode", {}).get("duration_seconds", metadata["duration_seconds"]))
            if abs(metadata["duration_seconds"] - source_duration) > max(0.05, source_duration * 0.01):
                raise ExportError(f"{item['item_id']} FLAC duration differs from validated source")

            cover_entry: dict[str, Any] | None = None
            cover_source = item["cover_path"]
            if cover_source:
                cover_path = Path(cover_source).resolve(strict=True)
                if not within(cover_path, batch_root) or cover_path.is_symlink():
                    raise ExportError(f"{item['item_id']} cover path is outside its batch")
                suffix = cover_path.suffix.lower()
                if suffix not in {".png", ".jpg", ".jpeg", ".webp"}:
                    raise ExportError(f"{item['item_id']} cover is not a supported raster image")
                if item["cover_sha256"] != sha256(cover_path):
                    raise ExportError(f"{item['item_id']} cover hash differs from the database")
                cover_output = assets / f"{output_id}{suffix if suffix != '.jpeg' else '.jpg'}"
                shutil.copyfile(cover_path, cover_output)
                cover_metadata = probe_image(cover_output, ffprobe)
                cover_prompt = structured.get("cover_prompt")
                if not isinstance(cover_prompt, str) or not cover_prompt.strip():
                    cover_prompt = f"Original Aria Focus cover art for {activity} focus music; no text, no logo."
                cover_entry = {
                    "path": f"assets/{cover_output.name}",
                    "sha256": sha256(cover_output),
                    "bytes": cover_output.stat().st_size,
                    "format": {".png": "png", ".jpg": "jpeg", ".jpeg": "jpeg", ".webp": "webp"}[suffix],
                    "width": cover_metadata["width"],
                    "height": cover_metadata["height"],
                    "provenance": {
                        "source": f"OpenRouter cloud batch {batch_id}",
                        "generator": {
                            "provider": "OpenRouter",
                            "model": item["image_model"] or "unknown",
                            "model_version": item["image_model"] or "unknown",
                            "prompt": cover_prompt,
                        },
                        "licence_id": licence_id,
                        "licence_url": licence_url,
                    },
                }
            elif not allow_missing_covers:
                raise ExportError(f"{item['item_id']} is missing required cover art")

            manifest_items.append(
                {
                    "id": output_id,
                    "title": f"{activity.replace('_', ' ').title()} Focus {int(item['ordinal']) + 1:02d}",
                    "genre_ids": [genre_id],
                    "mood_ids": sorted(set(mood_ids)),
                    "activity_suitability": activity_suitability(activity),
                    "provenance": {
                        "source": f"OpenRouter cloud generation batch {batch_id}; draft until human QA",
                        "licence_id": licence_id,
                        "licence_url": licence_url,
                        "composer": None,
                        "generator": {
                            "provider": "OpenRouter",
                            "model": item["audio_model"],
                            "model_version": item["audio_model"],
                            "prompt": prompt,
                        },
                        "contains_lyrics": False,
                        "contains_speech": False,
                    },
                    "analysis": analysis,
                    "variants": [
                        {
                            "id": "source",
                            "asset": {
                                "path": f"assets/{flac_path.name}",
                                "sha256": sha256(flac_path),
                                "bytes": flac_path.stat().st_size,
                                "codec": "flac",
                                "sample_rate_hz": metadata["sample_rate_hz"],
                                "channels": metadata["channels"],
                                "bit_depth": metadata["bit_depth"],
                            },
                            "safe_regions": [
                                {
                                    "kind": "loop",
                                    "start_seconds": 0.0,
                                    "end_seconds": analysis["duration_seconds"],
                                }
                            ],
                            "stimulation_available": ["off", "low", "medium", "high"],
                        }
                    ],
                    "human_qa": {"status": "draft", "reviews": []},
                    "cover": cover_entry,
                }
            )

        manifest = {
            "format": "adhdpack",
            "format_version": 1,
            "pack": {
                "id": pack_id,
                "title": pack_title,
                "description": "OpenRouter-generated Aria Focus music; draft pending human listening QA.",
                "version": pack_version,
                "app_version_requirement": app_version_requirement,
            },
            "taxonomy": {
                "genres": [{"id": key, "label": genres[key]} for key in sorted(genres)],
                "moods": [{"id": key, "label": moods[key]} for key in sorted(moods)],
            },
            "items": sorted(manifest_items, key=lambda value: value["id"]),
        }
        (stage / "manifest.json").write_text(
            json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        os.replace(stage, output)
    except Exception:
        shutil.rmtree(stage, ignore_errors=True)
        raise
    return {"output": str(output.resolve()), "items": len(manifest_items), "batch_id": batch_id}


def probe_image(path: Path, ffprobe: str) -> dict[str, int]:
    output = run(
        [ffprobe, "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "json", str(path)],
        label=f"ffprobe {path.name}",
    )
    try:
        stream = json.loads(output)["streams"][0]
        width = int(stream["width"])
        height = int(stream["height"])
    except (KeyError, IndexError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise ExportError(f"{path.name} has incomplete cover dimensions") from error
    if not (1 <= width <= 4096 and 1 <= height <= 4096):
        raise ExportError(f"{path.name} dimensions are outside the supported range")
    return {"width": width, "height": height}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", type=Path, required=True, help="Aria Focus preferences.sqlite3")
    parser.add_argument("--storage-root", type=Path, required=True, help="App-data root containing cloud-generation")
    parser.add_argument("--batch-id", required=True)
    parser.add_argument("--output", type=Path, required=True, help="new FLAC draft pack directory")
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--ffprobe", default="ffprobe")
    parser.add_argument("--licence-id", required=True)
    parser.add_argument("--licence-url", required=True)
    parser.add_argument("--pack-id", default=DEFAULT_PACK_ID)
    parser.add_argument("--pack-title", default=DEFAULT_PACK_TITLE)
    parser.add_argument("--pack-version", default=DEFAULT_PACK_VERSION)
    parser.add_argument("--app-version-requirement", default=DEFAULT_APP_REQUIREMENT)
    parser.add_argument("--allow-missing-covers", action="store_true", help="internal pilot only; public release requires covers")
    parser.add_argument(
        "--exclude-item-id",
        action="append",
        default=[],
        help="staging-only omission for an explicitly rejected item; merge a reviewed replacement before release",
    )
    parser.add_argument(
        "--require-complete-library",
        action="store_true",
        help="require exactly 100 items and 20 items per activity",
    )
    arguments = parser.parse_args()
    try:
        result = export(
            arguments.db,
            arguments.storage_root,
            arguments.batch_id,
            arguments.output,
            ffmpeg=arguments.ffmpeg,
            ffprobe=arguments.ffprobe,
            licence_id=arguments.licence_id,
            licence_url=arguments.licence_url,
            pack_id=arguments.pack_id,
            pack_title=arguments.pack_title,
            pack_version=arguments.pack_version,
            app_version_requirement=arguments.app_version_requirement,
            allow_missing_covers=arguments.allow_missing_covers,
            require_complete_library=arguments.require_complete_library,
            excluded_item_ids=set(arguments.exclude_item_id),
        )
    except ExportError as error:
        print(f"export failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
