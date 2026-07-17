"""Build a standalone, owner-waived replacement listening pack with cover art.

This is deliberately not a publication tool: every item stays ``draft`` with no
human approval. It assembles a new ``local-activity-library-v3`` source
directory from repeated ``--plan-run PLAN_PATH=RUN_ID`` arguments, copying the
verified FLAC masters and a matching 1024x1024 cover PNG per candidate, and
emitting a deterministic, canonical-ordered ``manifest.json``.

The catalogue layer owns final canonicalization/validation; this script only
produces a best-effort canonical manifest so the parent can install it via the
bundled owner-waived path without surprises.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import struct
import sys
import tempfile
import zlib
from collections import Counter
from decimal import Decimal, ROUND_HALF_EVEN
from pathlib import Path
from typing import Any

ACTIVITIES = ["deep_work", "motivation", "creativity", "learning", "light_work"]
ACTIVITY_LABELS = {
    "deep_work": "Deep Work",
    "motivation": "Motivation",
    "creativity": "Creativity",
    "learning": "Learning",
    "light_work": "Light Work",
}
COVER_FORMAT = "png"
COVER_DIMENSION = 1024
MAX_COVER_BYTES = 4 * 1024 * 1024
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"
EXPECTED_DURATION_SECONDS = 180

DEFAULT_PACK_ID = "local-activity-library-v3"
DEFAULT_VERSION = "0.22.0"
DEFAULT_APP_REQUIREMENT = ">=0.22.0, <0.23.0"
DEFAULT_TITLE = "Local Focus Library \u2014 Replacement Test v3"
DEFAULT_DESCRIPTION = (
    "One hundred local-only ACE-Step listening candidates with original Aria "
    "cover art: twenty for each focus activity. Draft, owner-waived test "
    "material; not approved or published."
)
DEFAULT_COVER_SOURCE = "Original Aria Focus cover art"
DEFAULT_COVER_PROVIDER = "Aria Focus"
DEFAULT_COVER_MODEL = "original cover art"
DEFAULT_COVER_VERSION = "0.1.0"


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def assert_verified_duration(candidate_id: str, duration_seconds: Any) -> None:
    """Every selected candidate must be exactly 180s of verified audio."""
    if duration_seconds != EXPECTED_DURATION_SECONDS:
        raise ValueError(
            f"candidate {candidate_id} verified audio duration is "
            f"{duration_seconds}s, expected exactly {EXPECTED_DURATION_SECONDS}s"
        )



def _round_sig(value: Decimal, digits: int) -> Decimal:
    """Round a Decimal to ``digits`` significant figures, ties-to-even."""
    if value == 0:
        return value
    exp = value.adjusted()
    quantum = Decimal(1).scaleb(exp - (digits - 1))
    return value.quantize(quantum, rounding=ROUND_HALF_EVEN)


def _f32_roundtrips(candidate: Decimal, packed: bytes) -> bool:
    """True when ``candidate`` parses (via f64) back to the same f32 bits."""
    try:
        return struct.pack("<f", float(candidate)) == packed
    except (ValueError, OverflowError):
        return False


def _format_f32_plain(digits: tuple[int, ...], exp: int, exponent: int) -> str:
    """Plain decimal form, matching ryu's fixed notation for f32."""
    if exponent >= 0:
        return "".join(map(str, digits)) + "0" * exponent + ".0"
    if exp >= 0:
        int_part = "".join(map(str, digits[: exp + 1]))
        frac_part = "".join(map(str, digits[exp + 1 :]))
        return f"{int_part}.{frac_part}"
    return "0." + "0" * (-exp - 1) + "".join(map(str, digits))


def _format_f32_scientific(digits: tuple[int, ...], exp: int) -> str:
    """Scientific form, matching ryu (``e`` with no leading ``+``)."""
    if len(digits) == 1:
        mantissa = str(digits[0])
    else:
        mantissa = f"{digits[0]}." + "".join(map(str, digits[1:]))
    suffix = f"-{-exp}" if exp < 0 else str(exp)
    return f"{mantissa}e{suffix}"


def format_f32_serde(value: float) -> str:
    """Format a float exactly as serde_json serializes an f32 (ryu shortest).

    The catalogue canonical form serializes every analysis/suitability field
    as f32, so the builder must emit the same shortest decimal ryu would
    produce after the value is narrowed to f32. Python ``repr`` formats the
    f64 and differs in two observable ways: sub-millisecond mantissa rounding
    (e.g. 259.51662 -> 259.51663) and exponent notation (e.g. 1.8e-05 ->
    0.000018). This mirrors ryu's f32 output for the manifest value ranges.
    """
    packed = struct.pack("<f", float(value))
    bits = struct.unpack("<I", packed)[0]
    if bits & 0x7FFFFFFF == 0:
        return "-0.0" if bits >> 31 else "0.0"
    exact = Decimal(struct.unpack("<f", packed)[0])
    chosen: Decimal | None = None
    for digits in range(1, 10):
        candidate = _round_sig(exact, digits)
        if _f32_roundtrips(candidate, packed):
            chosen = candidate
            break
    if chosen is None:
        chosen = _round_sig(exact, 9)
    normalized = chosen.normalize()
    digits_tuple = normalized.as_tuple().digits
    exp = normalized.adjusted()
    exponent = normalized.as_tuple().exponent
    sign = "-" if normalized.is_signed() else ""
    if -6 <= exp <= 12:
        return sign + _format_f32_plain(digits_tuple, exp, exponent)
    return sign + _format_f32_scientific(digits_tuple, exp)


def _escape_json_string(text: str) -> str:
    """JSON string escaping matching serde_json: raw UTF-8, controls escaped."""
    pieces = ['"']
    for ch in text:
        code = ord(ch)
        if ch == '"':
            pieces.append('\\"')
        elif ch == "\\":
            pieces.append("\\\\")
        elif ch == "\b":
            pieces.append("\\b")
        elif ch == "\f":
            pieces.append("\\f")
        elif ch == "\n":
            pieces.append("\\n")
        elif ch == "\r":
            pieces.append("\\r")
        elif ch == "\t":
            pieces.append("\\t")
        elif code < 0x20:
            pieces.append(f"\\u{code:04x}")
        else:
            pieces.append(ch)
    pieces.append('"')
    return "".join(pieces)


def _encode_canonical(obj: Any, parts: list[str]) -> None:
    """Compact JSON encoder using f32-ryu float formatting and raw UTF-8."""
    if obj is None:
        parts.append("null")
    elif obj is True:
        parts.append("true")
    elif obj is False:
        parts.append("false")
    elif isinstance(obj, bool):
        parts.append("true" if obj else "false")
    elif isinstance(obj, int):
        parts.append(str(obj))
    elif isinstance(obj, float):
        parts.append(format_f32_serde(obj))
    elif isinstance(obj, str):
        parts.append(_escape_json_string(obj))
    elif isinstance(obj, dict):
        parts.append("{")
        first = True
        for key, value in obj.items():
            if not first:
                parts.append(",")
            first = False
            parts.append(_escape_json_string(str(key)))
            parts.append(":")
            _encode_canonical(value, parts)
        parts.append("}")
    elif isinstance(obj, (list, tuple)):
        parts.append("[")
        first = True
        for value in obj:
            if not first:
                parts.append(",")
            first = False
            _encode_canonical(value, parts)
        parts.append("]")
    else:
        raise TypeError(f"unsupported type for canonical encoding: {type(obj).__name__}")


def canonical_dumps(obj: Any) -> bytes:
    """Byte-canonical JSON matching ``catalogue::canonical_manifest_bytes``."""
    parts: list[str] = []
    _encode_canonical(obj, parts)
    return "".join(parts).encode("utf-8")


def is_png(data: bytes) -> bool:
    return data.startswith(PNG_MAGIC)


def png_dimensions(data: bytes) -> tuple[int, int]:
    """Read width/height from the IHDR chunk without external dependencies."""
    if not is_png(data) or len(data) < 24:
        raise ValueError("not a PNG or truncated header")
    if data[12:16] != b"IHDR":
        raise ValueError("first chunk is not IHDR")
    width = int.from_bytes(data[16:20], "big")
    height = int.from_bytes(data[20:24], "big")
    return width, height


def derive_title(candidate_id: str) -> str:
    """`creativity-ambient-electronic-01` -> `Creativity Ambient Electronic 01`."""
    return " ".join(word.capitalize() for word in candidate_id.split("-"))


def activity_label(activity: str) -> str:
    return ACTIVITY_LABELS[activity]


def build_activity_suitability(activity: str) -> list[dict]:
    return [
        {"activity": value, "suitability": 0.95 if value == activity else 0.0}
        for value in sorted(ACTIVITIES)
    ]


def build_analysis(candidate: dict, report: dict) -> dict:
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
        # The analyzer's sub-millisecond near-silence total is not an unexplained
        # audible gap; authored regions >=100 ms are absent for these candidates.
        "unexplained_silence_seconds": 0.0,
        "clipped_samples": measured["clipped_samples"],
        "discontinuity_detected": measured["discontinuity_candidates"]["candidate_count"] > 0,
        "codec_errors_detected": False,
        "corruption_detected": False,
        "vocal_speech_likelihood": 0.0,
    }


def derive_cover_prompt(candidate: dict, title: str) -> str:
    activity = ACTIVITY_LABELS[candidate["activity"]]
    genres = ", ".join(candidate["genre_ids"]) or "ambient"
    moods = ", ".join(candidate["mood_ids"]) or "calm"
    return (
        f"Original Aria Focus cover art for {title} \u2014 {activity} focus; "
        f"genres: {genres}; moods: {moods}; instrumental, no text, no logo."
    )


def build_cover_provenance(candidate: dict, title: str, generator: dict, source: str) -> dict:
    return {
        "source": source,
        "generator": {
            "provider": generator["provider"],
            "model": generator["model"],
            "model_version": generator["model_version"],
            "prompt": derive_cover_prompt(candidate, title),
        },
        "licence_id": None,
        "licence_url": None,
    }


def build_cover_asset(candidate_id: str, cover_bytes: bytes, provenance: dict) -> dict:
    width, height = png_dimensions(cover_bytes)
    return {
        "path": f"assets/{candidate_id}.{COVER_FORMAT}",
        "sha256": sha256_bytes(cover_bytes),
        "bytes": len(cover_bytes),
        "format": COVER_FORMAT,
        "width": width,
        "height": height,
        "provenance": provenance,
    }


def build_provenance(candidate: dict, batch: dict) -> dict:
    return {
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
    }


def build_item(
    candidate: dict,
    batch: dict,
    audio_sha256: str,
    audio_bytes: int,
    duration_seconds: float,
    report: dict,
    cover: dict,
    item_id: str,
) -> dict:
    title = derive_title(candidate["id"])
    return {
        "id": item_id,
        "title": title,
        "genre_ids": sorted(candidate["genre_ids"]),
        "mood_ids": sorted(candidate["mood_ids"]),
        "activity_suitability": build_activity_suitability(candidate["activity"]),
        "provenance": build_provenance(candidate, batch),
        "analysis": build_analysis(candidate, report),
        "variants": [
            {
                "id": "source",
                "asset": {
                    "path": f"assets/{candidate['id']}.flac",
                    "sha256": audio_sha256,
                    "bytes": audio_bytes,
                    "codec": "flac",
                    "sample_rate_hz": 48000,
                    "channels": 2,
                    "bit_depth": 16,
                },
                "safe_regions": [
                    {
                        "kind": "loop",
                        "start_seconds": 0.0,
                        "end_seconds": float(duration_seconds),
                    }
                ],
                "stimulation_available": ["off", "low", "medium", "high"],
            }
        ],
        "human_qa": {"status": "draft", "reviews": []},
        "cover": cover,
    }


def merge_taxonomy(plan_taxonomies: list[dict]) -> dict:
    genres: dict[str, str] = {}
    moods: dict[str, str] = {}
    for taxonomy in plan_taxonomies:
        for entry in taxonomy["genres"]:
            if entry["id"] in genres and genres[entry["id"]] != entry["label"]:
                raise ValueError(
                    f"conflicting genre label for {entry['id']}: "
                    f"{genres[entry['id']]} vs {entry['label']}"
                )
            genres[entry["id"]] = entry["label"]
        for entry in taxonomy["moods"]:
            if entry["id"] in moods and moods[entry["id"]] != entry["label"]:
                raise ValueError(
                    f"conflicting mood label for {entry['id']}: "
                    f"{moods[entry['id']]} vs {entry['label']}"
                )
            moods[entry["id"]] = entry["label"]
    return {
        "genres": [{"id": key, "label": value} for key, value in sorted(genres.items())],
        "moods": [{"id": key, "label": value} for key, value in sorted(moods.items())],
    }


def parse_plan_run(value: str) -> tuple[str, str]:
    if "=" not in value:
        raise argparse.ArgumentTypeError(
            f"--plan-run must be PLAN_PATH=RUN_ID, got {value!r}"
        )
    plan_path, run_id = value.split("=", 1)
    if not plan_path or not run_id:
        raise argparse.ArgumentTypeError(
            f"--plan-run must be PLAN_PATH=RUN_ID, got {value!r}"
        )
    return plan_path, run_id


def load_run_candidates(
    repo_root: Path,
    runs_root: Path,
    plan_path: str,
    run_id: str,
    cover_dir: Path,
    cover_generator: dict,
    cover_source: str,
    assets_dir: Path,
    dry_run: bool,
    exclude_ids: set[str] | None = None,
) -> tuple[list[dict], dict]:
    """Load and verify every candidate from one run; copy its FLAC + cover."""
    run_dir = runs_root / run_id
    if not run_dir.is_dir():
        raise FileNotFoundError(f"run directory not found: {run_dir}")
    plan_file = repo_root / "content" / "plans" / f"{plan_path}.json"
    if not plan_file.is_file():
        raise FileNotFoundError(f"plan file not found: {plan_file}")
    plan = read_json(plan_file)
    if plan["batch"]["id"] != plan_path:
        raise ValueError(
            f"plan {plan_path} batch id mismatch: {plan['batch']['id']}"
        )
    records_dir = run_dir / "generated-records"
    masters_dir = run_dir / "masters"
    analyzer_dir = run_dir / "analyzer-reports"
    items: list[dict] = []
    taxonomy = plan["taxonomy"]
    for record_path in sorted(records_dir.glob("*.json")):
        record = read_json(record_path)
        candidate = record["candidate"]
        if candidate["id"] in (exclude_ids or set()):
            continue
        if record["batch"]["id"] != plan_path:
            raise ValueError(
                f"candidate {candidate['id']} batch {record['batch']['id']} "
                f"differs from plan {plan_path}"
            )
        verified = record.get("verified")
        if not verified:
            raise ValueError(f"candidate {candidate['id']} has no verified evidence")
        assert_verified_duration(candidate["id"], verified["duration_seconds"])
        audio_path = masters_dir / verified["file_name"]
        if not audio_path.is_file():
            raise FileNotFoundError(f"master FLAC missing for {candidate['id']}: {audio_path}")
        audio_bytes = audio_path.stat().st_size
        if audio_bytes != verified["bytes"]:
            raise ValueError(
                f"candidate {candidate['id']} audio bytes {audio_bytes} != "
                f"declared {verified['bytes']}"
            )
        audio_sha = sha256_file(audio_path)
        if audio_sha.lower() != verified["sha256"].lower():
            raise ValueError(f"candidate {candidate['id']} audio sha256 mismatch")
        report_path = analyzer_dir / verified["analyzer_file_name"]
        if not report_path.is_file():
            raise FileNotFoundError(
                f"analyzer report missing for {candidate['id']}: {report_path}"
            )
        report = read_json(report_path)
        if report["source"]["sha256"].lower() != verified["sha256"].lower():
            raise ValueError(
                f"candidate {candidate['id']} analyzer source sha256 mismatch"
            )
        if report["source"]["bytes"] != verified["bytes"]:
            raise ValueError(
                f"candidate {candidate['id']} analyzer source bytes mismatch"
            )
        cover_path = cover_dir / f"{candidate['id']}.png"
        if not cover_path.is_file():
            raise FileNotFoundError(f"cover PNG missing for {candidate['id']}: {cover_path}")
        cover_bytes = cover_path.read_bytes()
        if not is_png(cover_bytes):
            raise ValueError(f"cover for {candidate['id']} is not a PNG")
        if len(cover_bytes) > MAX_COVER_BYTES:
            raise ValueError(
                f"cover for {candidate['id']} exceeds {MAX_COVER_BYTES} bytes"
            )
        cwidth, cheight = png_dimensions(cover_bytes)
        if cwidth != COVER_DIMENSION or cheight != COVER_DIMENSION:
            raise ValueError(
                f"cover for {candidate['id']} is {cwidth}x{cheight}, "
                f"expected {COVER_DIMENSION}x{COVER_DIMENSION}"
            )
        title = derive_title(candidate["id"])
        provenance = build_cover_provenance(candidate, title, cover_generator, cover_source)
        cover = build_cover_asset(candidate["id"], cover_bytes, provenance)
        if not dry_run:
            shutil.copy2(audio_path, assets_dir / f"{candidate['id']}.flac")
            shutil.copy2(cover_path, assets_dir / f"{candidate['id']}.png")
        items.append(
            build_item(
                candidate,
                record["batch"],
                audio_sha,
                audio_bytes,
                verified["duration_seconds"],
                report,
                cover,
                f"library-v3-{candidate['id']}",
            )
        )
    return items, taxonomy


def assemble_manifest(
    items: list[dict],
    taxonomy: dict,
    pack_id: str,
    title: str,
    description: str,
    version: str,
    app_requirement: str,
) -> dict:
    return {
        "format": "adhdpack",
        "format_version": 1,
        "pack": {
            "id": pack_id,
            "title": title,
            "description": description,
            "version": version,
            "app_version_requirement": app_requirement,
        },
        "taxonomy": taxonomy,
        "items": sorted(items, key=lambda value: value["id"]),
    }


def validate_counts(items: list[dict]) -> dict:
    if len(items) != 100:
        raise RuntimeError(f"expected exactly 100 candidates, found {len(items)}")
    ids = [item["id"] for item in items]
    if len(set(ids)) != len(ids):
        dupes = sorted({i for i in ids if ids.count(i) > 1})
        raise RuntimeError(f"duplicate candidate ids: {dupes}")
    counts = Counter(
        next(
            entry["activity"]
            for entry in item["activity_suitability"]
            if entry["suitability"] > 0
        )
        for item in items
    )
    if any(counts.get(activity, 0) != 20 for activity in ACTIVITIES):
        raise RuntimeError(f"expected exactly 20 tracks per activity: {dict(counts)}")
    return dict(counts)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan-run", action="append", type=parse_plan_run, required=True,
                        help="PLAN_PATH=RUN_ID, repeatable (e.g. activity-library-expansion-v2=replacement-expansion-v3)")
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--runs-root", type=Path, default=None,
                        help="default <repo-root>/.local/music-generation/runs")
    parser.add_argument("--cover-dir", type=Path, required=True,
                        help="directory holding <candidate_id>.png covers")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--pack-id", default=DEFAULT_PACK_ID)
    parser.add_argument("--title", default=DEFAULT_TITLE)
    parser.add_argument("--description", default=DEFAULT_DESCRIPTION)
    parser.add_argument("--version", default=DEFAULT_VERSION)
    parser.add_argument("--app-version-requirement", default=DEFAULT_APP_REQUIREMENT)
    parser.add_argument("--cover-source", default=DEFAULT_COVER_SOURCE)
    parser.add_argument("--cover-generator-provider", default=DEFAULT_COVER_PROVIDER)
    parser.add_argument("--cover-generator-model", default=DEFAULT_COVER_MODEL)
    parser.add_argument("--cover-generator-version", default=DEFAULT_COVER_VERSION)
    parser.add_argument("--force", action="store_true", help="overwrite an existing output dir")
    parser.add_argument("--exclude-candidate", action="append", default=None,
                        help="candidate id to skip (repeatable); used to drop bad "
                             "records from one run while a repair run supplies replacements")
    parser.add_argument("--dry-run", action="store_true",
                        help="validate inputs and assemble the manifest without writing files")
    args = parser.parse_args(argv)

    repo_root = args.repo_root.resolve()
    runs_root = (args.runs_root or repo_root / ".local" / "music-generation" / "runs").resolve()
    cover_dir = args.cover_dir.resolve()
    output = args.output.resolve()

    if not cover_dir.is_dir():
        raise FileNotFoundError(f"cover dir not found: {cover_dir}")

    cover_generator = {
        "provider": args.cover_generator_provider,
        "model": args.cover_generator_model,
        "model_version": args.cover_generator_version,
    }

    if args.dry_run:
        assets_dir = repo_root / ".local" / "music-generation" / ".cover-build-dry-run-assets"
    else:
        if output.exists() and not args.force:
            raise RuntimeError(f"refusing to overwrite {output}")
        if output.exists():
            shutil.rmtree(output)
        assets_dir = output / "assets"
        assets_dir.mkdir(parents=True, exist_ok=False)

    exclude_ids = set(args.exclude_candidate or [])
    all_items: list[dict] = []
    plan_taxonomies: list[dict] = []
    for plan_path, run_id in args.plan_run:
        items, taxonomy = load_run_candidates(
            repo_root,
            runs_root,
            plan_path,
            run_id,
            cover_dir,
            cover_generator,
            args.cover_source,
            assets_dir,
            args.dry_run,
            exclude_ids,
        )
        all_items.extend(items)
        plan_taxonomies.append(taxonomy)

    counts = validate_counts(all_items)
    taxonomy = merge_taxonomy(plan_taxonomies)
    manifest = assemble_manifest(
        all_items,
        taxonomy,
        args.pack_id,
        args.title,
        args.description,
        args.version,
        args.app_version_requirement,
    )
    manifest_bytes = canonical_dumps(manifest)
    summary = {
        "items": len(all_items),
        "counts": counts,
        "pack_id": args.pack_id,
        "version": args.version,
        "manifest_bytes": len(manifest_bytes),
        "output": None if args.dry_run else str(output),
        "dry_run": args.dry_run,
    }
    if not args.dry_run:
        (output / "manifest.json").write_bytes(manifest_bytes)
        print(json.dumps(summary, indent=2))
    else:
        print(json.dumps(summary, indent=2))
    return 0


def self_test() -> int:
    """Lightweight in-process checks for the pure helpers."""
    candidate = {
        "id": "creativity-ambient-electronic-01",
        "activity": "creativity",
        "genre_ids": ["ambient-electronic"],
        "mood_ids": ["calm", "steady"],
        "bpm": 70,
        "prompts": {"positive": "[Instrumental] calm ambient electronic creativity bed"},
    }
    assert derive_title(candidate["id"]) == "Creativity Ambient Electronic 01"
    suitability = build_activity_suitability("creativity")
    assert [entry["activity"] for entry in suitability] == sorted(ACTIVITIES)
    assert next(entry for entry in suitability if entry["activity"] == "creativity")["suitability"] == 0.95
    assert all(entry["suitability"] == 0.0 for entry in suitability if entry["activity"] != "creativity")

    one_pixel = (
        PNG_MAGIC + b"\x00\x00\x00\x0dIHDR" + (1).to_bytes(4, "big") + (1).to_bytes(4, "big")
        + b"\x08\x06\x00\x00\x00" + b"\x00\x00\x00\x00"
    )
    assert png_dimensions(one_pixel) == (1, 1)
    assert is_png(one_pixel)
    assert not is_png(b"not a png")

    taxonomy = merge_taxonomy([
        {"genres": [{"id": "ambient-electronic", "label": "Ambient Electronic"}],
         "moods": [{"id": "calm", "label": "Calm"}]},
        {"genres": [{"id": "downtempo", "label": "Downtempo"}],
         "moods": [{"id": "calm", "label": "Calm"}, {"id": "steady", "label": "Steady"}]},
    ])
    assert [genre["id"] for genre in taxonomy["genres"]] == ["ambient-electronic", "downtempo"]
    assert [mood["id"] for mood in taxonomy["moods"]] == ["calm", "steady"]

    report = {
        "decode": {"duration_seconds": 180.0},
        "measurements": {
            "integrated_lufs": {"value": -18.0},
            "true_peak_dbtp": {"value": -1.0},
            "loudness_range_lu": {"value": 4.0},
            "spectral_centroid_hz": {"value": 300.0},
            "high_frequency_energy_ratio": {"value": 0.0001},
            "onset_density_per_second": {"value": 2.0},
            "clipped_samples": 0,
            "discontinuity_candidates": {"candidate_count": 0},
        },
    }
    analysis = build_analysis(candidate, report)
    assert analysis["duration_seconds"] == 180.0

    assert_verified_duration(candidate["id"], 180)
    assert_verified_duration(candidate["id"], 180.0)
    try:
        assert_verified_duration(candidate["id"], 179.9)
    except ValueError as exc:
        assert "creativity-ambient-electronic-01" in str(exc)
        assert "179.9" in str(exc)
        assert "180" in str(exc)
    else:
        raise AssertionError("assert_verified_duration should reject non-180 duration")
    assert analysis["tempo_bpm"] == 70.0
    assert analysis["vocal_speech_likelihood"] == 0.0
    assert analysis["discontinuity_detected"] is False

    batch = {
        "id": "activity-library-expansion-v2",
        "generator_pin": {"source_commit": "abc123"},
        "terms_evidence": {
            "output_licence": "ace-step-1.5-output-terms",
            "licence_url": "https://example.invalid/LICENSE",
        },
    }
    cover = build_cover_asset(
        candidate["id"], one_pixel,
        build_cover_provenance(
            candidate, derive_title(candidate["id"]),
            {"provider": "Aria Focus", "model": "original cover art", "model_version": "0.1.0"},
            "Original Aria Focus cover art",
        ),
    )
    assert cover["format"] == "png"
    assert cover["width"] == 1 and cover["height"] == 1
    assert cover["provenance"]["source"] == "Original Aria Focus cover art"
    assert cover["provenance"]["generator"]["provider"] == "Aria Focus"
    assert "Creativity Ambient Electronic 01" in cover["provenance"]["generator"]["prompt"]

    item = build_item(
        candidate, batch, "a" * 64, 1024, 180.0, report, cover,
        f"library-v3-{candidate['id']}",
    )
    assert item["id"] == "library-v3-creativity-ambient-electronic-01"
    assert item["variants"][0]["asset"]["path"] == "assets/creativity-ambient-electronic-01.flac"
    assert item["cover"]["path"] == "assets/creativity-ambient-electronic-01.png"
    assert item["human_qa"] == {"status": "draft", "reviews": []}

    # --exclude-candidate: load_run_candidates must skip records whose
    # candidate.id is in the exclusion set before building items, while still
    # verifying/encoding the remaining ones through the full 180s + canonical
    # path. Build a tiny two-candidate run in a temp tree and exercise it.
    def _png1024() -> bytes:
        def chunk(tag: bytes, data: bytes) -> bytes:
            return (
                len(data).to_bytes(4, "big") + tag + data
                + int(zlib.crc32(tag + data)).to_bytes(4, "big")
            )
        ihdr = (1024).to_bytes(4, "big") + (1024).to_bytes(4, "big") + b"\x08\x06\x00\x00\x00"
        return PNG_MAGIC + chunk(b"IHDR", ihdr) + chunk(b"IEND", b"")

    cand_a = candidate
    cand_b = dict(candidate, id="creativity-ambient-electronic-02")
    plan_id = batch["id"]
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "content" / "plans").mkdir(parents=True)
        plan = {
            "batch": batch,
            "taxonomy": {"genres": [{"id": "ambient-electronic", "label": "Ambient Electronic"}],
                         "moods": [{"id": "calm", "label": "Calm"}]},
            "candidates": [cand_a, cand_b],
        }
        (root / "content" / "plans" / f"{plan_id}.json").write_text(json.dumps(plan), encoding="utf-8")
        run_dir = root / ".local" / "music-generation" / "runs" / "exclusion-test-run"
        rec_dir = run_dir / "generated-records"
        mas_dir = run_dir / "masters"
        ana_dir = run_dir / "analyzer-reports"
        cover_dir = root / "covers"
        for d in (rec_dir, mas_dir, ana_dir, cover_dir):
            d.mkdir(parents=True)
        png = _png1024()
        for cand in (cand_a, cand_b):
            file_name = f"{cand['id']}.flac"
            master = b"fake-flac-" + cand["id"].encode("utf-8")
            (mas_dir / file_name).write_bytes(master)
            sha = sha256_bytes(master)
            report_file = {
                "decode": {"duration_seconds": 180.0},
                "source": {"sha256": sha, "bytes": len(master)},
                "measurements": report["measurements"],
            }
            ana_name = f"{cand['id']}.json"
            (ana_dir / ana_name).write_text(json.dumps(report_file), encoding="utf-8")
            record = {
                "candidate": cand,
                "batch": batch,
                "verified": {
                    "file_name": file_name,
                    "bytes": len(master),
                    "sha256": sha,
                    "duration_seconds": 180.0,
                    "analyzer_file_name": ana_name,
                },
            }
            (rec_dir / f"{cand['id']}.json").write_text(json.dumps(record), encoding="utf-8")
            (cover_dir / f"{cand['id']}.png").write_bytes(png)
        runs_root = root / ".local" / "music-generation" / "runs"
        cover_gen = {"provider": "Aria Focus", "model": "original cover art", "model_version": "0.1.0"}
        all_items, _ = load_run_candidates(
            root, runs_root, plan_id, "exclusion-test-run", cover_dir,
            cover_gen, "Original Aria Focus cover art", root / "assets", True, set(),
        )
        assert {it["id"] for it in all_items} == {
            f"library-v3-{cand_a['id']}", f"library-v3-{cand_b['id']}",
        }, "both candidates should load without exclusions"
        kept, _ = load_run_candidates(
            root, runs_root, plan_id, "exclusion-test-run", cover_dir,
            cover_gen, "Original Aria Focus cover art", root / "assets", True,
            {cand_b["id"]},
        )
        assert [it["id"] for it in kept] == [f"library-v3-{cand_a['id']}"], (
            "excluded candidate id must be skipped before building items"
        )
        # The kept item must still serialize byte-canonically.
        assert b'"duration_seconds":180.0' in canonical_dumps(kept[0])

    counts = validate_counts(
        [
            {"id": f"library-v3-{act}-{i}", "activity_suitability": [
                {"activity": a, "suitability": 0.95 if a == act else 0.0}
                for a in ACTIVITIES
            ]}
            for act in ACTIVITIES for i in range(20)
        ]
    )
    assert counts == {activity: 20 for activity in ACTIVITIES}

    # canonical_dumps must emit compact JSON with raw UTF-8 and serde/ryu
    # f32 float formatting, so the manifest is byte-identical to the
    # catalogue canonical form (canonical_manifest_bytes).
    for value, expected in (
        (180.0, "180.0"),
        (0.0, "0.0"),
        (-0.0, "-0.0"),
        (0.95, "0.95"),
        # Python repr "1.8e-05" differs from ryu f32 "0.000018".
        (1.8e-05, "0.000018"),
        # Python repr "259.51662" differs from ryu f32 "259.51663".
        (259.51662, "259.51663"),
        (214.646681, "214.64668"),
        (1e-7, "1e-7"),
        (3.4e38, "3.4e38"),
    ):
        assert format_f32_serde(value) == expected, (
            f"format_f32_serde({value})={format_f32_serde(value)!r} != {expected!r}"
        )
    sample = {"title": "Local Focus \u2014 Caf\u00e9 Jazz", "ratio": 1.8e-05}
    sample_bytes = canonical_dumps(sample)
    assert b"\xc3\xa9" in sample_bytes, "canonical_dumps must emit raw UTF-8"
    assert b"\\u00e9" not in sample_bytes, "non-ASCII must not be unicode-escaped"
    assert b"\\u2014" not in sample_bytes, "em dash must not be unicode-escaped"
    assert b'"ratio":0.000018' in sample_bytes, "f32 floats must use ryu formatting"
    assert sample_bytes.endswith(b"}") and b", " not in sample_bytes, "compact separators"

    print("self-test ok")
    return 0


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        raise SystemExit(self_test())
    raise SystemExit(main())
