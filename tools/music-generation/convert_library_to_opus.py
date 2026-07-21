"""Create a separate, reproducible Ogg Opus distribution candidate from FLAC masters.

This is deliberately a *staging* tool.  It never writes inside its source pack,
never overwrites an existing destination, and never changes review status.  The
result remains a candidate until the normal library review/release gate approves
it.

It uses ffmpeg/libopus at a fixed 112 kbps VBR profile and ffprobe to validate
the emitted Ogg Opus stream.  ffmpeg and ffprobe must be from the same trusted
local installation.  The exact tool versions are recorded in conversion-report.json
so a release can be reproduced and audited.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, Callable


BITRATE_KBPS = 112
SAMPLE_RATE_HZ = 48_000
CHANNELS = 2
DEFAULT_MAX_TOTAL_BYTES = 300_000_000
SAFE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
PRIVATE_BETA_RELATIVE = Path("apps/desktop/src-tauri/private-beta-pack")


class ConversionError(RuntimeError):
    """A source, encoder, or candidate validation error."""


def strict_json(path: Path) -> dict[str, Any]:
    def pairs(values: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in values:
            if key in result:
                raise ConversionError(f"duplicate JSON key in {path}: {key}")
            result[key] = value
        return result

    try:
        data = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=pairs)
    except (OSError, json.JSONDecodeError) as error:
        raise ConversionError(f"cannot read manifest {path}: {error}") from error
    if not isinstance(data, dict):
        raise ConversionError("manifest root must be an object")
    return data


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def canonical_json(value: Any) -> bytes:
    # The source pack is already the Rust application's canonical manifest.
    # Preserve its schema field order while changing only values/asset entries;
    # sorting JSON keys alphabetically would produce bytes the app correctly
    # rejects as non-canonical.
    return (json.dumps(value, separators=(",", ":"), ensure_ascii=False) + "\n").encode("utf-8")


def canonicalize_manifest(path: Path) -> None:
    """Use the application's Rust serializer as the single manifest authority."""
    root = Path(__file__).resolve().parents[2]
    command = [
        "cargo",
        "run",
        "--quiet",
        "-p",
        "content-ingest",
        "--bin",
        "canonicalize-private-beta",
        "--",
        str(path),
    ]
    try:
        subprocess.run(command, cwd=root, check=True, capture_output=True, text=True)
    except (OSError, subprocess.CalledProcessError) as error:
        detail = getattr(error, "stderr", "") or str(error)
        raise ConversionError(f"could not canonicalize candidate manifest: {detail.strip()}") from error


def within(child: Path, parent: Path) -> bool:
    try:
        child.relative_to(parent)
        return True
    except ValueError:
        return False


def safe_relative(value: object, label: str) -> Path:
    if not isinstance(value, str) or not value:
        raise ConversionError(f"{label} must be a non-empty path")
    path = Path(value)
    if path.is_absolute() or ".." in path.parts or path.as_posix() != value.replace("\\", "/"):
        raise ConversionError(f"{label} is not a canonical relative path: {value!r}")
    return path


def source_assets(source: Path, manifest: dict[str, Any]) -> list[tuple[dict[str, Any], Path]]:
    items = manifest.get("items")
    if not isinstance(items, list) or not items:
        raise ConversionError("manifest must contain at least one item")
    found: list[tuple[dict[str, Any], Path]] = []
    ids: set[str] = set()
    expected = {"manifest.json"}
    for item in items:
        if not isinstance(item, dict):
            raise ConversionError("manifest item must be an object")
        item_id = item.get("id")
        if not isinstance(item_id, str) or not SAFE_ID.fullmatch(item_id) or item_id in ids:
            raise ConversionError(f"invalid or duplicate item id: {item_id!r}")
        ids.add(item_id)
        variants = item.get("variants")
        if not isinstance(variants, list) or len(variants) != 1 or not isinstance(variants[0], dict):
            raise ConversionError(f"{item_id} must contain exactly one source variant")
        asset = variants[0].get("asset")
        if not isinstance(asset, dict):
            raise ConversionError(f"{item_id} has no source asset")
        relative = safe_relative(asset.get("path"), f"{item_id} source asset")
        if relative.suffix.lower() != ".flac" or asset.get("codec") != "flac":
            raise ConversionError(f"{item_id} must point to a FLAC master")
        asset_path = (source / relative).resolve(strict=True)
        if not within(asset_path, source) or not asset_path.is_file() or asset_path.is_symlink():
            raise ConversionError(f"{item_id} source asset is missing, linked, or outside the pack")
        if asset.get("bytes") != asset_path.stat().st_size or asset.get("sha256") != sha256(asset_path):
            raise ConversionError(f"{item_id} source asset does not match its manifest hash or size")
        expected.add(relative.as_posix())
        cover = item.get("cover")
        if cover is not None:
            if not isinstance(cover, dict):
                raise ConversionError(f"{item_id} cover must be an object")
            cover_relative = safe_relative(cover.get("path"), f"{item_id} cover asset")
            cover_path = (source / cover_relative).resolve(strict=True)
            if not within(cover_path, source) or not cover_path.is_file() or cover_path.is_symlink():
                raise ConversionError(f"{item_id} cover asset is missing, linked, or outside the pack")
            if cover.get("bytes") != cover_path.stat().st_size or cover.get("sha256") != sha256(cover_path):
                raise ConversionError(f"{item_id} cover asset does not match its manifest hash or size")
            expected.add(cover_relative.as_posix())
        found.append((item, asset_path))
    actual = {path.relative_to(source).as_posix() for path in source.rglob("*") if path.is_file()}
    if actual != expected:
        raise ConversionError("source pack is not closed-world or contains non-manifest files")
    return found


def tool_version(executable: str) -> str:
    try:
        result = subprocess.run([executable, "-version"], check=True, capture_output=True, text=True)
    except (OSError, subprocess.CalledProcessError) as error:
        raise ConversionError(f"cannot run {executable!r}: {error}") from error
    first = result.stdout.splitlines()[0] if result.stdout else result.stderr.splitlines()[0]
    return first.strip()


def encode(ffmpeg: str, source: Path, destination: Path) -> None:
    command = [
        ffmpeg, "-nostdin", "-v", "error", "-i", str(source), "-map", "0:a:0",
        "-map_metadata", "-1", "-fflags", "+bitexact", "-flags:a", "+bitexact",
        "-c:a", "libopus", "-application", "audio", "-b:a", f"{BITRATE_KBPS}k",
        "-vbr", "on", "-compression_level", "10", "-ar", str(SAMPLE_RATE_HZ),
        "-ac", str(CHANNELS), "-f", "ogg", "-serial_offset", "0", "-y", str(destination),
    ]
    try:
        subprocess.run(command, check=True, capture_output=True, text=True)
    except (OSError, subprocess.CalledProcessError) as error:
        detail = getattr(error, "stderr", "") or str(error)
        raise ConversionError(f"ffmpeg failed for {source.name}: {detail.strip()}") from error


def probe(ffprobe: str, path: Path) -> dict[str, Any]:
    command = [ffprobe, "-v", "error", "-show_entries", "format=format_name,duration:stream=codec_name,sample_rate,channels", "-of", "json", str(path)]
    try:
        result = subprocess.run(command, check=True, capture_output=True, text=True)
        payload = json.loads(result.stdout)
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        raise ConversionError(f"ffprobe failed for {path.name}: {error}") from error
    streams = payload.get("streams")
    if not isinstance(streams, list) or len(streams) != 1 or not isinstance(streams[0], dict):
        raise ConversionError(f"{path.name} must contain exactly one audio stream")
    stream = streams[0]
    fmt = payload.get("format", {})
    if stream.get("codec_name") != "opus" or "ogg" not in str(fmt.get("format_name", "")).split(","):
        raise ConversionError(f"{path.name} is not Ogg Opus")
    try:
        rate = int(stream["sample_rate"])
        channels = int(stream["channels"])
        duration = float(fmt["duration"])
    except (KeyError, TypeError, ValueError) as error:
        raise ConversionError(f"{path.name} has incomplete technical metadata") from error
    if rate != SAMPLE_RATE_HZ or channels != CHANNELS or not duration > 0:
        raise ConversionError(f"{path.name} must be {SAMPLE_RATE_HZ} Hz stereo with a positive duration")
    return {"duration_seconds": round(duration, 6), "sample_rate_hz": rate, "channels": channels}


def update_item(item: dict[str, Any], output_path: Path, metadata: dict[str, Any]) -> list[dict[str, float]]:
    item_id = item["id"]
    item["variants"][0]["asset"] = {
        "path": f"assets/{item_id}.opus",
        "sha256": sha256(output_path),
        "bytes": output_path.stat().st_size,
        "codec": "ogg_opus",
        "sample_rate_hz": metadata["sample_rate_hz"],
        "channels": metadata["channels"],
        "bit_depth": None,
    }
    changes: list[dict[str, float]] = []
    for region in item["variants"][0].get("safe_regions", []):
        if isinstance(region, dict) and region.get("kind") == "loop":
            try:
                start = float(region["start_seconds"])
                end = float(region["end_seconds"])
            except (KeyError, TypeError, ValueError) as error:
                raise ConversionError(f"{item_id} has malformed loop bounds") from error
            if start < 0 or end <= start:
                raise ConversionError(f"{item_id} has invalid loop bounds")
            if end > metadata["duration_seconds"]:
                if start >= metadata["duration_seconds"]:
                    raise ConversionError(f"{item_id} loop begins beyond decoded Opus duration")
                region["end_seconds"] = metadata["duration_seconds"]
                changes.append({"start_seconds": start, "old_end_seconds": end, "new_end_seconds": metadata["duration_seconds"]})
    return changes


def require_candidate_pack_metadata(
    manifest: dict[str, Any], pack_version: str | None, app_version_requirement: str | None
) -> None:
    pack = manifest.get("pack")
    if not isinstance(pack, dict):
        raise ConversionError("manifest must contain pack metadata")
    if not pack_version or not isinstance(pack_version, str) or not pack_version.strip():
        raise ConversionError("--pack-version is required: an Opus candidate must have a new pack revision")
    if not app_version_requirement or not isinstance(app_version_requirement, str) or not app_version_requirement.strip():
        raise ConversionError("--app-version-requirement is required: format-version 2 needs an explicit compatible app range")
    pack["version"] = pack_version
    pack["app_version_requirement"] = app_version_requirement


def convert(
    source: Path,
    output: Path,
    *,
    ffmpeg: str = "ffmpeg",
    ffprobe: str = "ffprobe",
    max_total_bytes: int = DEFAULT_MAX_TOTAL_BYTES,
    pack_version: str | None = None,
    app_version_requirement: str | None = None,
) -> dict[str, Any]:
    source = source.resolve(strict=True)
    output = output.resolve(strict=False)
    if not (source / "manifest.json").is_file():
        raise ConversionError("source must be a content pack directory containing manifest.json")
    if output.exists() or within(output, source):
        raise ConversionError("destination must not exist and must be outside the source pack")
    private_beta = (Path.cwd() / PRIVATE_BETA_RELATIVE).resolve(strict=False)
    if output == private_beta or within(output, private_beta):
        raise ConversionError("refusing to write into the private-beta pack")
    if max_total_bytes <= 0:
        raise ConversionError("max total bytes must be positive")

    output.parent.mkdir(parents=True, exist_ok=True)
    if not output.parent.is_dir() or output.parent.is_symlink():
        raise ConversionError("destination parent must be an ordinary directory")

    manifest = strict_json(source / "manifest.json")
    assets = source_assets(source, manifest)
    stage = Path(tempfile.mkdtemp(prefix=f".{output.name}.opus-staging-", dir=output.parent))
    try:
        staged_assets = stage / "assets"
        staged_assets.mkdir()
        updated = copy.deepcopy(manifest)
        require_candidate_pack_metadata(updated, pack_version, app_version_requirement)
        updated_items = {item["id"]: item for item in updated["items"]}
        records: list[dict[str, Any]] = []
        for source_item, source_file in sorted(assets, key=lambda pair: pair[0]["id"]):
            item_id = source_item["id"]
            candidate = staged_assets / f"{item_id}.opus"
            encode(ffmpeg, source_file, candidate)
            metadata = probe(ffprobe, candidate)
            source_analysis = source_item.get("analysis")
            if not isinstance(source_analysis, dict):
                raise ConversionError(f"{item_id} has no source technical analysis for duration validation")
            try:
                source_duration = float(source_analysis["duration_seconds"])
            except (KeyError, TypeError, ValueError) as error:
                raise ConversionError(f"{item_id} has invalid source analysis duration") from error
            if source_duration <= 0:
                raise ConversionError(f"{item_id} has non-positive source analysis duration")
            duration_delta = abs(metadata["duration_seconds"] - source_duration)
            duration_tolerance = max(0.05, source_duration * 0.01)
            if duration_delta > duration_tolerance:
                raise ConversionError(
                    f"{item_id} decoded Opus duration differs from source analysis by {duration_delta:.6f}s "
                    f"(tolerance {duration_tolerance:.6f}s)"
                )
            safe_region_changes = update_item(updated_items[item_id], candidate, metadata)
            records.append({
                "id": item_id,
                "source": {"path": source_file.relative_to(source).as_posix(), "sha256": sha256(source_file)},
                "distribution": {"path": candidate.relative_to(stage).as_posix(), "sha256": sha256(candidate), "bytes": candidate.stat().st_size, **metadata},
                "validation": {
                    "status": "technical_metadata_validated",
                    "checks": ["single_ogg_opus_stream", "48khz_stereo", "positive_duration", "duration_within_source_tolerance"],
                    "source_duration_seconds": source_duration,
                    "duration_delta_seconds": round(duration_delta, 6),
                    "duration_tolerance_seconds": round(duration_tolerance, 6),
                    "analysis": {"status": "not_run", "reason": "the local audio analyzer currently has no Ogg Opus decoder; technical validation is fail-closed"},
                    "safe_region_clamps": safe_region_changes,
                },
            })
        for item in manifest["items"]:
            cover = item.get("cover")
            if not isinstance(cover, dict):
                continue
            cover_relative = safe_relative(cover["path"], f"{item['id']} cover asset")
            destination = stage / cover_relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source / cover_relative, destination)
        updated["format_version"] = 2
        (stage / "manifest.json").write_bytes(canonical_json(updated))
        canonicalize_manifest(stage / "manifest.json")
        expected = {"manifest.json", *(record["distribution"]["path"] for record in records)}
        expected.update(
            item["cover"]["path"].replace("\\", "/")
            for item in manifest["items"]
            if isinstance(item.get("cover"), dict)
        )
        actual = {path.relative_to(stage).as_posix() for path in stage.rglob("*") if path.is_file()}
        total = sum(path.stat().st_size for path in stage.rglob("*") if path.is_file())
        if actual != expected:
            raise ConversionError("candidate pack is not closed-world")
        if total > max_total_bytes:
            raise ConversionError(f"candidate pack is {total} bytes, over budget of {max_total_bytes}")
        report = {
            "schema_version": 1,
            "conversion": {"codec": "ogg_opus", "container": "ogg", "bitrate_kbps": BITRATE_KBPS, "vbr": True, "sample_rate_hz": SAMPLE_RATE_HZ, "channels": CHANNELS},
            "source": {"manifest_sha256": sha256(source / "manifest.json"), "items": len(records)},
            "tools": {"ffmpeg": tool_version(ffmpeg), "ffprobe": tool_version(ffprobe)},
            "total_bytes": total,
            "max_total_bytes": max_total_bytes,
            "items": records,
        }
        # The report is deliberately kept outside the closed-world pack so it
        # cannot be mistaken for a runtime asset.
        stage_report = stage.with_suffix(".conversion-report.json")
        stage_report.write_bytes(canonical_json(report))
        os.replace(stage, output)
        os.replace(stage_report, output.parent / f"{output.name}.conversion-report.json")
        return report
    except Exception:
        shutil.rmtree(stage, ignore_errors=True)
        raise


def dry_run(source: Path, output: Path, *, max_total_bytes: int = DEFAULT_MAX_TOTAL_BYTES, pack_version: str | None = None, app_version_requirement: str | None = None) -> dict[str, Any]:
    """Validate the immutable FLAC pack and print the deterministic output map.

    Encoding is intentionally not simulated: size and audio validity are only
    knowable after libopus has produced the bytes.  This mode is for checking
    that the chosen source/output pair is safe before a long full-library run.
    """
    source = source.resolve(strict=True)
    output = output.resolve(strict=False)
    if not (source / "manifest.json").is_file():
        raise ConversionError("source must be a content pack directory containing manifest.json")
    if output.exists() or within(output, source):
        raise ConversionError("destination must not exist and must be outside the source pack")
    private_beta = (Path.cwd() / PRIVATE_BETA_RELATIVE).resolve(strict=False)
    if output == private_beta or within(output, private_beta):
        raise ConversionError("refusing to write into the private-beta pack")
    if max_total_bytes <= 0:
        raise ConversionError("max total bytes must be positive")
    manifest = strict_json(source / "manifest.json")
    require_candidate_pack_metadata(copy.deepcopy(manifest), pack_version, app_version_requirement)
    assets = source_assets(source, manifest)
    return {
        "source": str(source), "output": str(output), "items": len(assets),
        "format_version": 2, "codec": "ogg_opus", "bitrate_kbps": BITRATE_KBPS,
        "max_total_bytes": max_total_bytes,
        "outputs": [f"assets/{item['id']}.opus" for item, _ in sorted(assets, key=lambda pair: pair[0]["id"])],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True, help="closed-world FLAC source pack")
    parser.add_argument("--output", type=Path, required=True, help="new Ogg Opus candidate pack directory")
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--ffprobe", default="ffprobe")
    parser.add_argument("--max-total-bytes", type=int, default=DEFAULT_MAX_TOTAL_BYTES)
    parser.add_argument("--pack-version", help="required new candidate pack version; source revision is never reused")
    parser.add_argument("--app-version-requirement", help="required compatible app range for format-version 2")
    parser.add_argument("--dry-run", action="store_true", help="validate source/output safety and show planned manifest paths without encoding")
    args = parser.parse_args()
    try:
        if args.dry_run:
            print(json.dumps(dry_run(args.source, args.output, max_total_bytes=args.max_total_bytes, pack_version=args.pack_version, app_version_requirement=args.app_version_requirement), indent=2))
            return 0
        report = convert(args.source, args.output, ffmpeg=args.ffmpeg, ffprobe=args.ffprobe, max_total_bytes=args.max_total_bytes, pack_version=args.pack_version, app_version_requirement=args.app_version_requirement)
    except ConversionError as error:
        print(f"conversion failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps({"output": str(args.output.resolve()), "items": len(report["items"]), "total_bytes": report["total_bytes"]}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
