"""Create the immutable two-track replacement plan for the v2 library.

The original generated candidates remain in their ledger unchanged. This makes
new identities and seeds for the two tracks rejected by the bundled-pack gate.
"""

from __future__ import annotations

import argparse
import copy
import json
from pathlib import Path


REPLACEMENTS = {
    "deep-work-downtempo-03": ("deep-work-downtempo-replacement-01", 1801103),
    "light-work-soft-electronic-05": ("light-work-soft-electronic-replacement-01", 1805105),
}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        raise RuntimeError(f"refusing to overwrite {args.output}")
    source = json.loads(args.source.read_text(encoding="utf-8"))
    candidates = []
    for candidate in source["candidates"]:
        replacement = REPLACEMENTS.get(candidate["id"])
        if replacement is None:
            continue
        value = copy.deepcopy(candidate)
        value["id"], value["seed"] = replacement
        candidates.append(value)
    if len(candidates) != len(REPLACEMENTS):
        raise RuntimeError("the source plan does not contain both rejected candidates")
    source["batch"]["id"] = "activity-library-replacements-v1"
    source["batch"]["created_at"] = "2026-07-13T06:00:00Z"
    source["batch"]["notes"] = (
        "Two deterministic 180-second replacements for v2 library candidates "
        "rejected by the discontinuity gate. Instrumental only; no lyrics or speech."
    )
    source["candidates"] = candidates
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(source, separators=(",", ":")), encoding="utf-8")
    print(json.dumps({"batch": source["batch"]["id"], "candidates": len(candidates)}))


if __name__ == "__main__":
    main()
