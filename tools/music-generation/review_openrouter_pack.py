#!/usr/bin/env python3
"""Record a human listening review for one staged OpenRouter pack item.

This command is deliberately explicit. It never invents approval, never
changes audio bytes, and never publishes a pack. A public pack still requires
two distinct human reviewers per item and the normal technical verifier.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


class ReviewError(ValueError):
    """A review cannot be safely applied to this manifest."""


REVIEWER_ID = re.compile(r"^[a-z0-9][a-z0-9_.-]{1,63}$")


def strict_json(path: Path) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise ReviewError(f"manifest is not a regular file: {path}")

    def pairs(values: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in values:
            if key in result:
                raise ReviewError(f"duplicate JSON key: {key}")
            result[key] = value
        return result

    try:
        value = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=pairs)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ReviewError(f"cannot read manifest: {error}") from error
    if not isinstance(value, dict):
        raise ReviewError("manifest root must be an object")
    return value


def canonical_json(value: Any) -> bytes:
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n").encode(
        "utf-8"
    )


def atomic_write(path: Path, value: Any) -> None:
    if path.is_symlink() or not path.parent.is_dir() or path.parent.is_symlink():
        raise ReviewError(f"refusing unsafe manifest path: {path}")
    payload = canonical_json(value)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="wb", prefix=f".{path.name}.review-", dir=path.parent, delete=False
        ) as stream:
            temporary = Path(stream.name)
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def now_utc() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def apply_review(
    manifest: dict[str, Any],
    *,
    item_id: str,
    reviewer_id: str,
    status: str,
    notes: str,
    representative_work_session: bool,
    session_minutes: int,
    reviewed_at: str,
) -> dict[str, Any]:
    if not REVIEWER_ID.fullmatch(reviewer_id):
        raise ReviewError("reviewer-id must be 2-64 lowercase characters using letters, numbers, _, -, or .")
    if status not in {"approved", "rejected"}:
        raise ReviewError("status must be approved or rejected")
    notes = notes.strip()
    if len(notes) < 10:
        raise ReviewError("notes must contain at least 10 characters")
    if session_minutes < 0:
        raise ReviewError("session-minutes cannot be negative")
    if representative_work_session and session_minutes <= 0:
        raise ReviewError("a representative work session must include session-minutes")

    items = manifest.get("items")
    if not isinstance(items, list):
        raise ReviewError("manifest has no item list")
    item = next((candidate for candidate in items if candidate.get("id") == item_id), None)
    if not isinstance(item, dict):
        raise ReviewError(f"item not found: {item_id}")
    qa = item.setdefault("human_qa", {"status": "draft", "reviews": []})
    if not isinstance(qa, dict):
        raise ReviewError(f"{item_id} human_qa is not an object")
    reviews = qa.setdefault("reviews", [])
    if not isinstance(reviews, list):
        raise ReviewError(f"{item_id} human_qa.reviews is not a list")
    if any(isinstance(review, dict) and review.get("reviewer_id") == reviewer_id for review in reviews):
        raise ReviewError(f"reviewer {reviewer_id} has already reviewed {item_id}")

    reviews.append(
        {
            "reviewer_id": reviewer_id,
            "reviewed_at": reviewed_at,
            "status": status,
            "notes": notes,
            "representative_work_session": representative_work_session,
            "session_minutes": session_minutes,
        }
    )
    approved_reviews = [
        review
        for review in reviews
        if isinstance(review, dict) and review.get("status") == "approved"
    ]
    distinct_approvers = {review.get("reviewer_id") for review in approved_reviews}
    if any(isinstance(review, dict) and review.get("status") == "rejected" for review in reviews):
        qa["status"] = "rejected"
    elif len(distinct_approvers) >= 2:
        qa["status"] = "approved"
    else:
        qa["status"] = "draft"
    qa["protocol_version"] = "1"
    qa["last_reviewed_at"] = reviewed_at
    return item


def review_pack(
    root: Path,
    *,
    item_id: str,
    reviewer_id: str,
    status: str,
    notes: str,
    representative_work_session: bool,
    session_minutes: int,
    reviewed_at: str | None,
) -> dict[str, Any]:
    root = root.resolve(strict=True)
    if root.is_symlink() or not root.is_dir():
        raise ReviewError("pack root must be a regular directory")
    manifest_path = root / "manifest.json"
    manifest = strict_json(manifest_path)
    apply_review(
        manifest,
        item_id=item_id,
        reviewer_id=reviewer_id,
        status=status,
        notes=notes,
        representative_work_session=representative_work_session,
        session_minutes=session_minutes,
        reviewed_at=reviewed_at or now_utc(),
    )
    atomic_write(manifest_path, manifest)
    return manifest


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pack-root", type=Path, required=True)
    parser.add_argument("--item-id", required=True)
    parser.add_argument("--reviewer-id", required=True)
    parser.add_argument("--status", choices=("approved", "rejected"), required=True)
    parser.add_argument("--notes", required=True)
    parser.add_argument("--representative-work-session", action="store_true")
    parser.add_argument("--session-minutes", type=int, default=0)
    parser.add_argument("--reviewed-at", help="UTC ISO-8601 timestamp; defaults to now")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        manifest = review_pack(
            args.pack_root,
            item_id=args.item_id,
            reviewer_id=args.reviewer_id,
            status=args.status,
            notes=args.notes,
            representative_work_session=args.representative_work_session,
            session_minutes=args.session_minutes,
            reviewed_at=args.reviewed_at,
        )
    except (OSError, ReviewError) as error:
        print(f"review failed: {error}")
        return 2
    item = next(item for item in manifest["items"] if item.get("id") == args.item_id)
    print(f"review recorded: {args.item_id} → {item['human_qa']['status']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
