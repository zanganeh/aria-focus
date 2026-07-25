"""Canonicalize reviewer IDs in an approved library manifest.

This is a narrow migration tool for review records created before the runtime
manifest enforced lowercase stable IDs. It changes review identity fields only;
audio, covers, prompts, hashes, and approval status are left untouched.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import tempfile
from pathlib import Path


STABLE_ID = re.compile(r"^[a-z0-9][a-z0-9_.-]{1,63}$")


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")


def normalize(root: Path, source_id: str, target_id: str) -> int:
    if not STABLE_ID.fullmatch(target_id):
        raise ValueError("target reviewer ID must be a lowercase stable ID")
    manifest_path = root.resolve(strict=True) / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    changed = 0
    for item in manifest.get("items", []):
        reviews = item.get("human_qa", {}).get("reviews", [])
        ids = [review.get("reviewer_id") for review in reviews]
        if source_id in ids and target_id in ids:
            raise ValueError(f"{item.get('id')} already contains both reviewer IDs")
        for review in reviews:
            if review.get("reviewer_id") == source_id:
                review["reviewer_id"] = target_id
                changed += 1
    if changed == 0:
        raise ValueError(f"reviewer ID {source_id!r} was not found")
    fd, temporary_name = tempfile.mkstemp(prefix=".manifest-", suffix=".tmp", dir=manifest_path.parent)
    os.close(fd)
    temporary = Path(temporary_name)
    try:
        temporary.write_bytes(canonical_json(manifest))
        os.replace(temporary, manifest_path)
    finally:
        temporary.unlink(missing_ok=True)
    return changed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pack-root", type=Path, required=True)
    parser.add_argument("--from-id", required=True)
    parser.add_argument("--to-id", required=True)
    args = parser.parse_args()
    print(f"normalized {normalize(args.pack_root, args.from_id, args.to_id)} review records")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
