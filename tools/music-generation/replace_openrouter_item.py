"""Replace one quality-rejected item in an OpenRouter FLAC draft pack.

The replacement is intentionally explicit and closed-world: it copies the
base pack, requires one replacement item with the same activity, copies only
the replacement media into the original asset locations, and records the
replacement provider provenance in the resulting manifest. It never edits an
existing pack in place.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
from copy import deepcopy
from pathlib import Path
from typing import Any


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_manifest(root: Path) -> dict[str, Any]:
    path = root / "manifest.json"
    if not path.is_file():
        raise ValueError(f"manifest.json is missing from {root}")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise ValueError(f"manifest.json is invalid: {error}") from error
    if not isinstance(value, dict) or not isinstance(value.get("items"), list):
        raise ValueError(f"manifest.json in {root} is not a supported pack")
    return value


def item_by_id(manifest: dict[str, Any], item_id: str) -> dict[str, Any]:
    matches = [item for item in manifest["items"] if item.get("id") == item_id]
    if len(matches) != 1:
        raise ValueError(f"expected exactly one manifest item with id {item_id!r}")
    return matches[0]


def primary_activity(item: dict[str, Any]) -> str:
    activities = [
        entry.get("activity")
        for entry in item.get("activity_suitability", [])
        if isinstance(entry, dict) and entry.get("suitability", 0) > 0
    ]
    if len(activities) != 1 or not isinstance(activities[0], str):
        raise ValueError(f"item {item.get('id')!r} must have exactly one primary activity")
    return activities[0]


def asset_path(root: Path, item: dict[str, Any], kind: str) -> tuple[Path, dict[str, Any]]:
    if kind == "audio":
        variants = item.get("variants")
        if not isinstance(variants, list) or len(variants) != 1:
            raise ValueError(f"item {item.get('id')!r} must have exactly one audio variant")
        asset = variants[0].get("asset")
    else:
        asset = item.get("cover")
    if not isinstance(asset, dict) or not isinstance(asset.get("path"), str):
        raise ValueError(f"item {item.get('id')!r} has no {kind} asset")
    path = root / asset["path"]
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"item {item.get('id')!r} has an invalid {kind} path")
    expected_hash = asset.get("sha256")
    if not isinstance(expected_hash, str) or sha256_file(path) != expected_hash:
        raise ValueError(f"item {item.get('id')!r} has a {kind} hash mismatch")
    return path, asset


def validate_replacement(root: Path, item: dict[str, Any], expected_activity: str) -> None:
    if primary_activity(item) != expected_activity:
        raise ValueError(
            f"replacement activity {primary_activity(item)!r} does not match {expected_activity!r}"
        )
    if item.get("human_qa", {}).get("status") != "draft":
        raise ValueError("replacement item must still be in draft QA status")
    if item.get("analysis", {}).get("hard_rejections"):
        raise ValueError("replacement item still has analyzer hard rejections")
    generator = item.get("provenance", {}).get("generator", {})
    if generator.get("provider") != "OpenRouter":
        raise ValueError("replacement item is not OpenRouter provenance")
    asset_path(root, item, "audio")
    asset_path(root, item, "cover")


def replace_item(
    base_root: Path,
    replacement_root: Path,
    output_root: Path,
    base_item_id: str,
    replacement_item_id: str,
    pack_version: str | None,
    base_title: str | None = None,
) -> None:
    if output_root.exists():
        raise ValueError(f"output already exists: {output_root}")
    base_manifest = load_manifest(base_root)
    replacement_manifest = load_manifest(replacement_root)
    if not replacement_manifest["items"]:
        raise ValueError("replacement pack must contain at least one item")
    if "/" in base_item_id or "\\" in base_item_id:
        raise ValueError("base item id cannot contain a path separator")
    base_item = next((item for item in base_manifest["items"] if item.get("id") == base_item_id), None)
    replacement_item = item_by_id(replacement_manifest, replacement_item_id)
    activity = primary_activity(base_item or replacement_item)
    validate_replacement(replacement_root, replacement_item, activity)

    if base_item:
        base_audio, base_audio_asset = asset_path(base_root, base_item, "audio")
        base_cover, base_cover_asset = asset_path(base_root, base_item, "cover")
        target_audio_relative = base_audio.relative_to(base_root)
        target_cover_relative = base_cover.relative_to(base_root)
        title = base_item["title"]
        activity_suitability = deepcopy(base_item["activity_suitability"])
    else:
        if not base_title:
            raise ValueError("base_title is required when inserting a missing base item")
        target_audio_relative = Path("assets") / f"{base_item_id}.flac"
        target_cover_relative = Path("assets") / f"{base_item_id}.png"
        base_audio_asset = {"path": target_audio_relative.as_posix()}
        base_cover_asset = {"path": target_cover_relative.as_posix()}
        title = base_title
        activity_suitability = deepcopy(replacement_item["activity_suitability"])
    replacement_audio, _ = asset_path(replacement_root, replacement_item, "audio")
    replacement_cover, _ = asset_path(replacement_root, replacement_item, "cover")

    shutil.copytree(base_root, output_root)
    output_audio = output_root / target_audio_relative
    output_cover = output_root / target_cover_relative
    output_audio.parent.mkdir(parents=True, exist_ok=True)
    output_cover.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(replacement_audio, output_audio)
    shutil.copy2(replacement_cover, output_cover)

    merged = deepcopy(replacement_item)
    merged["id"] = base_item_id
    merged["title"] = title
    merged["activity_suitability"] = activity_suitability
    merged["variants"][0]["asset"]["path"] = base_audio_asset["path"]
    merged["cover"]["path"] = base_cover_asset["path"]
    merged["variants"][0]["asset"]["sha256"] = sha256_file(output_audio)
    merged["variants"][0]["asset"]["bytes"] = output_audio.stat().st_size
    merged["cover"]["sha256"] = sha256_file(output_cover)
    merged["cover"]["bytes"] = output_cover.stat().st_size
    merged["provenance"]["source"] = (
        f"OpenRouter replacement for {base_item_id} from "
        f"{replacement_manifest['pack']['id']}"
    )
    if pack_version:
        base_manifest["pack"]["version"] = pack_version
    if base_item:
        base_manifest["items"] = [
            merged if item.get("id") == base_item_id else item for item in base_manifest["items"]
        ]
    else:
        base_manifest["items"].append(merged)
        taxonomy = base_manifest.setdefault("taxonomy", {})
        for field in ("genres", "moods"):
            existing = {entry.get("id"): entry for entry in taxonomy.get(field, [])}
            for entry in replacement_manifest.get("taxonomy", {}).get(field, []):
                existing.setdefault(entry.get("id"), entry)
            taxonomy[field] = [existing[key] for key in sorted(existing)]
    base_manifest["items"].sort(key=lambda item: item.get("id", ""))
    (output_root / "manifest.json").write_text(
        json.dumps(base_manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-pack", type=Path, required=True)
    parser.add_argument("--replacement-pack", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--base-item-id", required=True)
    parser.add_argument("--replacement-item-id", required=True)
    parser.add_argument("--pack-version")
    parser.add_argument("--base-title", help="stable title when inserting a missing base item")
    args = parser.parse_args()
    try:
        replace_item(
            args.base_pack,
            args.replacement_pack,
            args.output,
            args.base_item_id,
            args.replacement_item_id,
            args.pack_version,
            args.base_title,
        )
    except (OSError, ValueError) as error:
        parser.error(str(error))
    print(json.dumps({"output": str(args.output), "replaced": args.base_item_id}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
