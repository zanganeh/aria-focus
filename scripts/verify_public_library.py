from __future__ import annotations

import hashlib
import json
import sys
from collections import Counter
from pathlib import Path

ACTIVITIES = ("deep_work", "motivation", "creativity", "learning", "light_work")
MAX_BUNDLED_BYTES = 1_650_000_000


def load_strict(path: Path) -> dict:
    def pairs(values):
        result = {}
        for key, value in values:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result

    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=pairs)


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def verify(root: Path) -> None:
    manifest_path = root / "manifest.json"
    manifest = load_strict(manifest_path)
    pack = manifest.get("pack", {})
    if pack.get("title") != "Aria Focus Library":
        raise ValueError("public pack title must be Aria Focus Library")
    forbidden = " ".join(str(value) for value in pack.values()).lower()
    if any(term in forbidden for term in ("private beta", "owner waived", "listening test")):
        raise ValueError("public pack metadata contains private-review wording")
    items = manifest.get("items")
    if not isinstance(items, list) or len(items) != 100:
        raise ValueError("public pack must contain exactly 100 items")
    counts: Counter[str] = Counter()
    expected_files = {"manifest.json"}
    total = manifest_path.stat().st_size
    for item in items:
        qa = item.get("human_qa", {})
        reviews = qa.get("reviews", [])
        if qa.get("status") != "approved" or len(reviews) < 2:
            raise ValueError(f"{item.get('id')} lacks two-reviewer approval")
        provenance = item.get("provenance", {})
        if not provenance.get("licence_id") or not provenance.get("licence_url"):
            raise ValueError(f"{item.get('id')} lacks licence evidence")
        if provenance.get("contains_lyrics") or provenance.get("contains_speech"):
            raise ValueError(f"{item.get('id')} is not instrumental")
        suitable = [
            entry["activity"]
            for entry in item.get("activity_suitability", [])
            if entry.get("suitability", 0) > 0
        ]
        if len(suitable) != 1 or suitable[0] not in ACTIVITIES:
            raise ValueError(f"{item.get('id')} must target exactly one public activity")
        counts[suitable[0]] += 1
        variants = item.get("variants", [])
        if len(variants) != 1:
            raise ValueError(f"{item.get('id')} must have one release variant")
        asset = variants[0].get("asset", {})
        relative = asset.get("path", "")
        path = root / relative
        if not relative or not path.is_file() or path.is_symlink():
            raise ValueError(f"{item.get('id')} asset is missing")
        if path.stat().st_size != asset.get("bytes") or digest(path) != asset.get("sha256"):
            raise ValueError(f"{item.get('id')} asset integrity differs")
        expected_files.add(relative.replace("\\", "/"))
        total += path.stat().st_size
    if counts != Counter({activity: 20 for activity in ACTIVITIES}):
        raise ValueError(f"public activity counts differ: {dict(counts)}")
    actual_files = {
        path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_file()
    }
    if actual_files != expected_files:
        raise ValueError("public pack root is not closed-world")
    if total > MAX_BUNDLED_BYTES:
        raise ValueError("public pack exceeds the installer release budget")


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "apps/desktop/src-tauri/private-beta-pack")
    verify(root.resolve())
    print("public library release gate: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
