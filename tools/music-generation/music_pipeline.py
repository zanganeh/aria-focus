"""Internal end-to-end music production pipeline.

This command composes the existing pinned ACE-Step generator, analyzer/candidate
ledger, FLAC candidate-pack builder, and Ogg Opus converter.  It deliberately
keeps FLAC masters and compressed distribution assets in separate directories
and never publishes, approves, or overwrites either one.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import tempfile
from pathlib import Path
from typing import Any

import build_local_test_library as library
import convert_library_to_opus as opus
import production


class PipelineError(RuntimeError):
    """A generation, evidence, pack, or conversion contract failed."""


def sha256(path: Path) -> str:
    return opus.sha256(path)


def plain_file(path: Path, label: str) -> Path:
    if not path.is_file() or path.is_symlink() or path.stat().st_size <= 0:
        raise PipelineError(f"{label} must be a nonempty ordinary file: {path}")
    return path


def load_record(path: Path, label: str) -> dict[str, Any]:
    plain_file(path, label)
    try:
        return opus.strict_json(path)
    except Exception as error:
        raise PipelineError(f"{label} is invalid: {error}") from error


def validate_generated_candidate(
    candidate: dict[str, Any],
    batch: dict[str, Any],
    asset: Path,
    report: Path,
    evidence: Path,
    record_path: Path,
) -> tuple[dict[str, Any], dict[str, Any]]:
    record = load_record(record_path, f"{candidate['id']} generated record")
    analysis = load_record(report, f"{candidate['id']} analyzer report")
    plain_file(asset, f"{candidate['id']} FLAC master")
    plain_file(evidence, f"{candidate['id']} generation evidence")

    verified = record.get("verified")
    evidence_record = record.get("evidence")
    if (
        record.get("schema") != "adhd-music.candidate-ledger.generated"
        or record.get("schema_version") != 1
        or record.get("lifecycle") != "generated"
        or record.get("candidate") != candidate
        or record.get("batch") != batch
        or not isinstance(verified, dict)
        or not isinstance(evidence_record, dict)
        or verified.get("file_name") != asset.name
        or verified.get("analyzer_file_name") != report.name
        or verified.get("codec") != "flac"
        or verified.get("sample_rate_hz") != 48_000
        or verified.get("channels") != 2
        or verified.get("bytes") != asset.stat().st_size
        or verified.get("sha256") != sha256(asset)
        or verified.get("analyzer_sha256") != sha256(report)
        or evidence_record.get("file_name") != evidence.name
        or evidence_record.get("sha256") != sha256(evidence)
    ):
        raise PipelineError(f"{candidate['id']} generated evidence differs from the selected plan or files")
    return record, analysis


def safe_output(output: Path, run_root: Path, private_pack: Path) -> Path:
    output = output.resolve(strict=False)
    if output.exists():
        raise PipelineError(f"refusing to overwrite existing output: {output}")
    if opus.within(output, run_root) or output == private_pack or opus.within(output, private_pack):
        raise PipelineError("output must stay outside generation runs and the desktop private-beta pack")
    output.parent.mkdir(parents=True, exist_ok=True)
    if not output.parent.is_dir() or output.parent.is_symlink():
        raise PipelineError("output parent must be an ordinary directory")
    return output


def build_flac_pack(
    root: Path,
    plan_path: Path,
    run_id: str,
    output: Path,
    *,
    pack_id: str,
    pack_title: str,
    pack_version: str,
    app_version_requirement: str,
) -> dict[str, Any]:
    root = root.resolve(strict=True)
    context = production.validate(root, plan_path)
    run_id = production.validate_run_id(run_id)
    run_root = root / ".local" / "music-generation" / "runs" / run_id
    if not run_root.is_dir() or run_root.is_symlink():
        raise PipelineError(f"generation run is unavailable: {run_id}")
    production.verify_run_identity(run_root, context, run_id, allow_legacy=True)

    try:
        production.validate_run_id(pack_id)
    except RuntimeError as error:
        raise PipelineError("pack ID must be one safe stable identifier") from error
    for label, value in (
        ("pack title", pack_title),
        ("FLAC pack version", pack_version),
        ("app version requirement", app_version_requirement),
    ):
        if not value or not value.strip():
            raise PipelineError(f"{label} is required")

    private_pack = (root / "apps" / "desktop" / "src-tauri" / "private-beta-pack").resolve(
        strict=False
    )
    output = safe_output(output, run_root.resolve(), private_pack)
    stage = Path(tempfile.mkdtemp(prefix=f".{output.name}.flac-staging-", dir=output.parent))
    try:
        assets = stage / "assets"
        assets.mkdir()
        items: list[dict[str, Any]] = []
        expected_files = {"manifest.json"}
        names: set[str] = set()
        for candidate in context.plan["candidates"]:
            asset, report, evidence, record_path, _, _ = production.candidate_paths(
                run_root, candidate["id"]
            )
            record, analysis = validate_generated_candidate(
                candidate,
                context.plan["batch"],
                asset,
                report,
                evidence,
                record_path,
            )
            casefolded = asset.name.casefold()
            if casefolded in names:
                raise PipelineError(f"case-ambiguous asset name: {asset.name}")
            names.add(casefolded)
            shutil.copy2(asset, assets / asset.name)
            expected_files.add(f"assets/{asset.name}")
            items.append(
                library.item(
                    record["candidate"],
                    record["batch"],
                    asset,
                    analysis,
                    f"{pack_id}.{candidate['id']}",
                )
            )

        taxonomy = context.plan.get("taxonomy")
        if not isinstance(taxonomy, dict):
            raise PipelineError("selected plan has no taxonomy")
        manifest = {
            "format": "adhdpack",
            "format_version": 1,
            "pack": {
                "id": pack_id,
                "title": pack_title,
                "description": (
                    f"Internal ACE-Step candidate pack from {context.plan['batch']['id']}; "
                    "draft listening material, not approved for publication."
                ),
                "version": pack_version,
                "app_version_requirement": app_version_requirement,
            },
            "taxonomy": {
                "genres": sorted(taxonomy.get("genres", []), key=lambda value: value["id"]),
                "moods": sorted(taxonomy.get("moods", []), key=lambda value: value["id"]),
            },
            "items": sorted(items, key=lambda value: value["id"]),
        }
        (stage / "manifest.json").write_text(
            json.dumps(manifest, separators=(",", ":")), encoding="utf-8"
        )
        opus.canonicalize_manifest(stage / "manifest.json")
        actual_files = {
            path.relative_to(stage).as_posix() for path in stage.rglob("*") if path.is_file()
        }
        if actual_files != expected_files:
            raise PipelineError("FLAC candidate pack is not closed-world")
        os.replace(stage, output)
        return {
            "path": str(output),
            "items": len(items),
            "bytes": sum(path.stat().st_size for path in output.rglob("*") if path.is_file()),
            "manifest_sha256": sha256(output / "manifest.json"),
        }
    except Exception:
        shutil.rmtree(stage, ignore_errors=True)
        raise


def package_run(
    root: Path,
    plan: Path,
    run_id: str,
    flac_output: Path,
    opus_output: Path,
    *,
    pack_id: str,
    pack_title: str,
    flac_version: str,
    opus_version: str,
    app_version_requirement: str,
    ffmpeg: str,
    ffprobe: str,
    max_total_bytes: int,
) -> dict[str, Any]:
    root = root.resolve(strict=True)
    flac_output = flac_output.resolve(strict=False)
    opus_output = opus_output.resolve(strict=False)
    if (
        flac_output == opus_output
        or opus.within(opus_output, flac_output)
        or opus.within(flac_output, opus_output)
    ):
        raise PipelineError("FLAC and Opus outputs must be separate, non-nested directories")
    reports = (
        opus_output.parent / f"{opus_output.name}.conversion-report.json",
        opus_output.parent / f"{opus_output.name}.pipeline-report.json",
    )
    if any(path.exists() for path in reports):
        raise PipelineError("refusing to overwrite an existing conversion or pipeline report")
    flac = build_flac_pack(
        root,
        plan,
        run_id,
        flac_output,
        pack_id=pack_id,
        pack_title=pack_title,
        pack_version=flac_version,
        app_version_requirement=app_version_requirement,
    )
    compressed = opus.convert(
        Path(flac["path"]),
        opus_output,
        ffmpeg=ffmpeg,
        ffprobe=ffprobe,
        max_total_bytes=max_total_bytes,
        pack_version=opus_version,
        app_version_requirement=app_version_requirement,
    )
    result = {
        "schema": "aria-focus.internal-music-pipeline",
        "schema_version": 1,
        "run_id": run_id,
        "flac": flac,
        "opus": {
            "path": str(opus_output.resolve()),
            "items": len(compressed["items"]),
            "bytes": compressed["total_bytes"],
            "bitrate_kbps": compressed["conversion"]["bitrate_kbps"],
        },
    }
    report = reports[1]
    try:
        with report.open("xb") as destination:
            destination.write(opus.canonical_json(result))
    except FileExistsError as error:
        raise PipelineError(f"refusing to overwrite existing pipeline report: {report}") from error
    result["report"] = str(report)
    return result


def require_packaging_arguments(arguments: argparse.Namespace) -> None:
    missing = [
        name
        for name in (
            "flac_output",
            "opus_output",
            "pack_id",
            "pack_title",
            "flac_version",
            "opus_version",
            "app_version_requirement",
        )
        if not getattr(arguments, name, None)
    ]
    if missing:
        raise PipelineError(f"packaging arguments are required: {', '.join(missing)}")


def preflight(root: Path, plan: Path, ffmpeg: str, ffprobe: str) -> None:
    production.preflight(root, plan=plan)
    opus.tool_version(ffmpeg)
    opus.tool_version(ffprobe)


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description="Generate, validate, package, and compress music")
    commands = value.add_subparsers(dest="action", required=True)
    for action in ("preflight", "generate", "package", "all"):
        command = commands.add_parser(action)
        command.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
        command.add_argument("--plan", type=Path, required=True)
        command.add_argument("--run-id")
        command.add_argument("--ffmpeg", default="ffmpeg")
        command.add_argument("--ffprobe", default="ffprobe")
        if action in ("package", "all"):
            command.add_argument("--flac-output", type=Path, required=True)
            command.add_argument("--opus-output", type=Path, required=True)
            command.add_argument("--pack-id", required=True)
            command.add_argument("--pack-title", required=True)
            command.add_argument("--flac-version", required=True)
            command.add_argument("--opus-version", required=True)
            command.add_argument("--app-version-requirement", required=True)
            command.add_argument(
                "--max-total-bytes", type=int, default=opus.DEFAULT_MAX_TOTAL_BYTES
            )
    return value


def main() -> int:
    arguments = parser().parse_args()
    root = arguments.root.resolve(strict=True)
    context = production.load_plan(root, arguments.plan)
    run_id = production.validate_run_id(arguments.run_id or context.plan["batch"]["id"])
    if arguments.action == "preflight":
        preflight(root, arguments.plan, arguments.ffmpeg, arguments.ffprobe)
        return 0
    if arguments.action in ("generate", "all"):
        production.run(root, plan=arguments.plan, run_id=run_id)
    if arguments.action in ("package", "all"):
        require_packaging_arguments(arguments)
        result = package_run(
            root,
            arguments.plan,
            run_id,
            arguments.flac_output,
            arguments.opus_output,
            pack_id=arguments.pack_id,
            pack_title=arguments.pack_title,
            flac_version=arguments.flac_version,
            opus_version=arguments.opus_version,
            app_version_requirement=arguments.app_version_requirement,
            ffmpeg=arguments.ffmpeg,
            ffprobe=arguments.ffprobe,
            max_total_bytes=arguments.max_total_bytes,
        )
        print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
